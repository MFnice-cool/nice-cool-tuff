mod auth;
mod cosmetics_local;
mod cpu_affinity;
mod gpu;
mod resource_mode;
mod create_mode_api;
mod deep_link;
mod file_manager;
mod github_auth;
mod github_pack_commands;
mod helpers;
mod home_bootstrap;
mod integrations;
mod launch_events;
mod launcher_presence;
mod launcher_settings;
mod listing_api;
mod mca_selector;
mod mod_presets;
mod overlay_hook;
mod pack_diff_api;
mod taurpc_api;
mod window_glass;
mod pack_events;
mod presence;
mod quest_chat_api;
mod snbt_parser;
mod speculative;
mod superseded_cleanup;
mod swarm_api;
mod swarm_node;
mod task_progress_api;
mod tune_config_api;
mod types;
mod web_research;
mod worlds;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use tuffbox_core::{
    ContentProvider, DependencyGraph, ModSource, ModSpec, PackBrief, ProjectManifest,
    ProviderFileInfo, ProviderSearchQuery, Resolver, Side, SnapshotStore, SourceKind,
    TuffboxLockfile,
};
use tuffbox_core::crash::FixAction;
use tuffbox_core::launch_error::{LaunchErrorInfo, LaunchErrorKind};
use tuffbox_core::process::{OnExit, ProcessExit};
use tauri::Emitter;

/// Serializes manifest + mods-folder mutations so background `sync_mods_folder`
/// cannot overwrite an in-flight Update All / single update.
static MODS_IO_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));
/// Single-flight guard for the short diagnostics cache miss path. Without it,
/// Badge, counts and Pack Health could all miss the cache concurrently and
/// rebuild the same graph three times.
static DIAGNOSTICS_IO_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));
static PACK_HEALTH_IO_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

use types::*;

pub(crate) use helpers::{
    auto_snapshot, auto_snapshot_before_mod_op, auto_snapshot_detailed,
    auto_snapshot_with_changed_files, backup_dir, copy_dir_recursive,
    find_manifest_in_project_dir, is_editable_config_path,
    load_backup_index, load_launcher_data, load_stats,
    manifest_parent, resolve_manifest_path, safe_project_file,
    save_backup_index, save_manifest, save_stats, save_launcher_data,
    persist_lockfile_for_manifest,
    slugify_project_name,
    unified_text_diff, read_small_text_file, validate_relative_snapshot_path,
    QUEST_IO_LOCK,
};

#[tauri::command(rename_all = "camelCase")]
fn get_project_schema_status(path: String) -> Result<SchemaStatus, String> {
    let raw = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let detected = value
        .get("schemaVersion")
        .and_then(|v| v.as_str())
        .unwrap_or("0.1.0")
        .to_string();
    let supported = tuffbox_core::manifest::SUPPORTED_PROJECT_SCHEMA_VERSIONS
        .iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>();
    if !supported.iter().any(|v| v == &detected) {
        return Err(format!(
            "unsupported project schema version {detected}; supported versions: {}",
            supported.join(", ")
        ));
    }
    Ok(SchemaStatus {
        current: tuffbox_core::manifest::CURRENT_PROJECT_SCHEMA_VERSION.to_string(),
        needs_migration: detected != tuffbox_core::manifest::CURRENT_PROJECT_SCHEMA_VERSION,
        detected,
        supported,
    })
}

#[tauri::command(rename_all = "camelCase")]
fn migrate_project_schema(path: String) -> Result<SchemaStatus, String> {
    auto_snapshot(&PathBuf::from(&path), "migrate-schema").map_err(|e| e.to_string())?;
    let mut manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
    manifest.migrate_to_current_schema();
    save_manifest(&PathBuf::from(&path), &manifest).map_err(|e| e.to_string())?;
    get_project_schema_status(path)
}

#[tauri::command]
fn validate_project(app: tauri::AppHandle, path: String) -> Result<ProjectSummary, String> {
    let manifest_path = resolve_manifest_path(&path)?;
    let manifest =
        ProjectManifest::load_from_path(&manifest_path).map_err(|e| e.to_string())?;
    manifest.validate_basic().map_err(|e| e.to_string())?;
    let profile = manifest
        .profiles
        .iter()
        .find(|p| p.id == "client")
        .or_else(|| manifest.profiles.first())
        .ok_or_else(|| "project has no profiles".to_string())?;

    // Auto-report pack basket (deduped) so import/open alone feeds Supabase co-occurrence.
    swarm_api::spawn_pack_cooccurrence(manifest_path.to_string_lossy().to_string(), "pack_open");
    spawn_warm_graph_cache(app, manifest_path.clone());

    Ok(project_summary_from_manifest(&manifest_path, &manifest, profile))
}

#[tauri::command(rename_all = "camelCase")]
fn resolve_project_path(path: String) -> Result<String, String> {
    Ok(resolve_manifest_path(&path)?
        .to_string_lossy()
        .to_string())
}

fn spawn_warm_graph_cache(app: tauri::AppHandle, manifest_path: PathBuf) {
    tauri::async_runtime::spawn(async move {
        let path = manifest_path;
        let warmed = tokio::task::spawn_blocking(move || {
            let manifest = ProjectManifest::load_from_path(&path).ok()?;
            let graph_wrote = tuffbox_core::warm_graph_cache(&path, &manifest).ok();
            let _ = collect_catalog_item_ids(&path);
            Some(graph_wrote == Some(true))
        })
        .await
        .ok()
        .flatten();
        if warmed == Some(true) {
            let _ = app.emit(
                "graph-refresh-progress",
                serde_json::json!({
                    "phase": "done",
                    "message": "Dependency graph cache warmed",
                }),
            );
        }
        let _ = app.emit("catalog-ready", serde_json::json!({ "ok": true }));
    });
}

fn project_summary_from_manifest(
    manifest_path: &Path,
    manifest: &ProjectManifest,
    profile: &tuffbox_core::manifest::ProfileSpec,
) -> ProjectSummary {
    ProjectSummary {
        id: manifest.project.id.clone(),
        name: manifest.project.name.clone(),
        version: manifest.project.version.clone(),
        minecraft_version: manifest.minecraft.version.clone(),
        loader_kind: tuffbox_core::graph::loader_kind_slug(&manifest.loader.kind).to_string(),
        loader_version: manifest.loader.version.clone(),
        java_path: manifest.java.as_ref().and_then(|j| j.path.clone()),
        memory_mb: profile.memory_mb.unwrap_or(4096),
        jvm_args: profile.jvm_args.clone(),
        player_name: profile
            .player_name
            .clone()
            .unwrap_or_else(|| "Player".to_string()),
        manifest_path: manifest_path.to_string_lossy().to_string(),
    }
}

#[tauri::command(rename_all = "camelCase")]
fn get_project_brief(path: String) -> Result<PackBrief, String> {
    let manifest_path = resolve_manifest_path(&path)?;
    let manifest = ProjectManifest::load_from_path(&manifest_path).map_err(|e| e.to_string())?;
    Ok(manifest.brief.unwrap_or_default())
}

#[tauri::command(rename_all = "camelCase")]
fn update_project_brief(path: String, brief: PackBrief) -> Result<(), String> {
    let manifest_path = resolve_manifest_path(&path)?;
    auto_snapshot(&manifest_path, "update-brief").map_err(|e| e.to_string())?;
    let mut manifest =
        ProjectManifest::load_from_path(&manifest_path).map_err(|e| e.to_string())?;
    manifest.brief = Some(brief);
    save_manifest(&manifest_path, &manifest).map_err(|e| e.to_string())
}

/// Renames an instance in place (manifest `project.name`). The folder path is
/// the identity everywhere (recents, groups, running processes), so only the
/// display name changes. Keeps the storefront listing name in sync when it
/// matched the old display name. Returns the applied (trimmed) name.
#[tauri::command(rename_all = "camelCase")]
fn rename_project(path: String, new_name: String) -> Result<String, String> {
    let new_name = new_name.trim().to_string();
    if new_name.is_empty() {
        return Err("Instance name cannot be empty".into());
    }
    if new_name.len() > 80 {
        return Err("Instance name is too long (max 80 characters)".into());
    }
    let manifest_path = resolve_manifest_path(&path)?;
    auto_snapshot(&manifest_path, "rename-project").map_err(|e| e.to_string())?;
    let mut manifest =
        ProjectManifest::load_from_path(&manifest_path).map_err(|e| e.to_string())?;
    let old_name = manifest.project.name.clone();
    manifest.project.name = new_name.clone();
    if let Some(listing) = manifest.listing.as_mut() {
        if listing.name.trim().is_empty() || listing.name == old_name {
            listing.name = new_name.clone();
        }
    }
    save_manifest(&manifest_path, &manifest).map_err(|e| e.to_string())?;
    Ok(new_name)
}

#[tauri::command]
fn list_profiles(path: String) -> Result<Vec<ProfileSummary>, String> {
    let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
    Ok(manifest
        .profiles
        .into_iter()
        .map(|p| ProfileSummary {
            id: p.id,
            name: p.name,
            side: format!("{:?}", p.side).to_lowercase(),
            memory_mb: p.memory_mb,
            jvm_args: p.jvm_args,
        })
        .collect())
}

#[tauri::command(rename_all = "camelCase")]
async fn sync_mods_folder(path: String) -> Result<Vec<serde_json::Value>, String> {
    tokio::task::spawn_blocking(move || {
        let _guard = MODS_IO_LOCK
            .lock()
            .map_err(|_| "mods I/O lock poisoned".to_string())?;
        let manifest_path = std::path::PathBuf::from(&path);
        let mut manifest =
            ProjectManifest::load_from_path(&manifest_path).map_err(|e| e.to_string())?;

        let mut project_dir = manifest_path.clone();
        if manifest_path.is_file() {
            project_dir.pop();
        }

        // Scan all content folders: mods/, resourcepacks/, shaderpacks/, datapacks/
        let content_dirs: &[(&str, &str, tuffbox_core::manifest::ContentType)] = &[
            ("mods", "jar", tuffbox_core::manifest::ContentType::Mod),
            (
                "resourcepacks",
                "zip",
                tuffbox_core::manifest::ContentType::Resourcepack,
            ),
            (
                "shaderpacks",
                "zip",
                tuffbox_core::manifest::ContentType::Shaderpack,
            ),
            (
                "datapacks",
                "zip",
                tuffbox_core::manifest::ContentType::Datapack,
            ),
        ];

        let provider = tuffbox_core::ModrinthProvider::new();
        let mut hash_index = tuffbox_core::ModHashIndex::load(&project_dir);
        let mut index_dirty = false;
        let mut any_changes = false;

        for &(dir_name, ext, default_content_type) in content_dirs {
            let dir = project_dir.join(dir_name);
            let entries = match std::fs::read_dir(&dir) {
                Ok(e) => e,
                Err(_) => continue,
            };

            for entry in entries.flatten() {
                let file_type = match entry.file_type() {
                    Ok(ft) => ft,
                    Err(_) => continue,
                };
                if !file_type.is_file() {
                    continue;
                }
                if !entry.path().extension().map_or(false, |e| e == ext) {
                    continue;
                }

                let file_name = entry.file_name().to_string_lossy().to_string();
                if let Some(idx) = manifest
                    .mods
                    .iter()
                    .position(|m| m.file_name.as_deref() == Some(file_name.as_str()))
                {
                    // Re-canonicalize Local drop-ins that were previously keyed by
                    // filename stem so Requires edges and the change plan resolve.
                    if manifest.mods[idx].source.kind == SourceKind::Local {
                        if let Ok(scan) = tuffbox_core::scan_mod_jar(&entry.path()) {
                            if let Some(mod_id) = scan
                                .mod_id
                                .as_ref()
                                .map(|s| s.trim().to_string())
                                .filter(|s| !s.is_empty())
                            {
                                let id_taken = manifest
                                    .mods
                                    .iter()
                                    .enumerate()
                                    .any(|(i, m)| i != idx && m.id == mod_id);
                                if !id_taken && manifest.mods[idx].id != mod_id {
                                    manifest.mods[idx].id = mod_id.clone();
                                    if manifest.mods[idx].name.ends_with(".jar")
                                        || manifest.mods[idx].name == file_name
                                    {
                                        manifest.mods[idx].name = mod_id;
                                    }
                                    any_changes = true;
                                }
                            }
                            if manifest.mods[idx].authors.is_empty() && !scan.authors.is_empty() {
                                manifest.mods[idx].authors = scan.authors;
                                any_changes = true;
                            }
                        }
                    }
                    continue;
                }

                let Some(sha1) = cached_sha1_file(&entry.path()) else {
                    continue;
                };

                let identified = resolve_mod_from_hash_or_modrinth(
                    &provider,
                    &mut hash_index,
                    &sha1,
                    &entry.path(),
                    file_name.clone(),
                    &mut index_dirty,
                );

                if let Some(mut identified) = identified {
                    identified.file_name = Some(file_name.clone());
                    identified.content_type = default_content_type;
                    // A leftover jar from a prior update (different filename,
                    // same Modrinth project) must not become a second manifest
                    // entry — that is how "updates duplicate mods".
                    let existing_idx = manifest.mods.iter().position(|m| {
                        identified
                            .source
                            .project_id
                            .as_ref()
                            .is_some_and(|pid| m.source.project_id.as_ref() == Some(pid))
                            || m.id == identified.id
                    });
                    if let Some(idx) = existing_idx {
                        let tracked = existing_mod_file_path(&manifest_path, &manifest.mods[idx]);
                        if tracked
                            .as_ref()
                            .is_some_and(|tracked_path| tracked_path != &entry.path())
                        {
                            let _ = std::fs::remove_file(entry.path());
                            continue;
                        }
                        if tracked.is_none() {
                            let keep_id = manifest.mods[idx].id.clone();
                            identified.id = keep_id;
                            manifest.mods[idx] = identified;
                            any_changes = true;
                        }
                        continue;
                    }
                    manifest.mods.push(identified);
                    any_changes = true;
                    continue;
                }

                // Unidentified jar: often a leftover after Update All when the
                // old filename no longer matches the manifest. If another
                // tracked mod already owns a live jar and this file looks like
                // the same slug, delete instead of creating a Local duplicate.
                let stem = file_name.trim_end_matches(&format!(".{}", ext)).to_lowercase();
                let superseded = manifest.mods.iter().any(|m| {
                    if m.source.kind == SourceKind::Local {
                        return false;
                    }
                    let Some(tracked_name) = m.file_name.as_deref() else {
                        return false;
                    };
                    if tracked_name == file_name {
                        return false;
                    }
                    let tracked_path = existing_mod_file_path(&manifest_path, m);
                    if !tracked_path.as_ref().is_some_and(|p| p.is_file()) {
                        return false;
                    }
                    let id = m.id.to_lowercase().replace('_', "-");
                    stem.starts_with(&id) || stem.split('-').next() == Some(id.as_str())
                });
                if superseded {
                    let _ = std::fs::remove_file(entry.path());
                    continue;
                }

                let scan = tuffbox_core::scan_mod_jar(&entry.path()).ok();
                let local_side = scan
                    .as_ref()
                    .map(|r| r.side)
                    .unwrap_or(tuffbox_core::manifest::Side::Unknown);
                // Prefer fabric/quilt/forge mod id so Requires edges match other mods'
                // dependency targets (filename stems like meteor-client-0.5.8 do not).
                let id = scan
                    .as_ref()
                    .and_then(|r| r.mod_id.as_ref())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| {
                        file_name
                            .trim_end_matches(&format!(".{}", ext))
                            .to_string()
                    });
                let name = scan
                    .as_ref()
                    .and_then(|r| r.mod_id.as_ref())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| file_name.clone());
                // Avoid colliding with an already-tracked Modrinth/CF mod of the same id.
                if manifest.mods.iter().any(|m| m.id == id) {
                    continue;
                }
                manifest.mods.push(tuffbox_core::manifest::ModSpec {
                    id,
                    name,
                    version: "unknown".to_string(),
                    side: local_side,
                    source: tuffbox_core::manifest::ModSource {
                        kind: tuffbox_core::manifest::SourceKind::Local,
                        project_id: None,
                        file_id: None,
                        url: None,
                        path: Some(format!("{}/{}", dir_name, file_name)),
                        icon_url: None,
                        categories: Vec::new(),
                    },
                    file_name: Some(file_name),
                    hashes: Some(tuffbox_core::FileHashes {
                        sha1: Some(sha1),
                        sha512: None,
                    }),
                    dependencies: vec![],
                    status: vec![],
                    content_type: default_content_type,
                    authors: scan.map(|r| r.authors).unwrap_or_default(),
                    option: None,
                });
                any_changes = true;
            }
        }

        // Re-identify local-only manifest entries once and cache the result.
        // Already-indexed Modrinth/CurseForge mods are never re-queried for identity.
        for idx in 0..manifest.mods.len() {
            if manifest.mods[idx].source.project_id.is_some() {
                continue;
            }
            let Some(file_name) = manifest.mods[idx].file_name.clone() else {
                continue;
            };
            let file_path =
                tuffbox_core::content_dir_for(&project_dir, manifest.mods[idx].content_type)
                    .join(&file_name);
            if !file_path.is_file() {
                continue;
            }
            let Some(sha1) = cached_sha1_file(&file_path) else {
                continue;
            };
            if let Some(spec) = resolve_mod_from_hash_or_modrinth(
                &provider,
                &mut hash_index,
                &sha1,
                &file_path,
                file_name.clone(),
                &mut index_dirty,
            ) {
                let mut spec = spec;
                spec.file_name = Some(file_name);
                // Keep jar-scan side only when Modrinth still reported Unknown.
                if spec.side == tuffbox_core::manifest::Side::Unknown {
                    spec.side = manifest.mods[idx].side;
                }
                manifest.mods[idx] = spec;
                any_changes = true;
            }
        }

        // One-time (then cached) Modrinth side backfill for already-tracked mods.
        // Old installs defaulted everything to `both`; refresh until client_side /
        // server_side are stored in the hash index.
        for idx in 0..manifest.mods.len() {
            if manifest.mods[idx].source.kind != SourceKind::Modrinth {
                continue;
            }
            let Some(project_id) = manifest.mods[idx].source.project_id.clone() else {
                continue;
            };
            let sha1 = manifest.mods[idx]
                .hashes
                .as_ref()
                .and_then(|h| h.sha1.clone());
            if let Some(ref sha1) = sha1 {
                if let Some(cached) = hash_index.get(sha1) {
                    if cached.client_side.is_some() || cached.server_side.is_some() {
                        let side = tuffbox_core::manifest::Side::from_modrinth(
                            cached.client_side.as_deref(),
                            cached.server_side.as_deref(),
                        );
                        if manifest.mods[idx].side != side {
                            manifest.mods[idx].side = side;
                            any_changes = true;
                        }
                        continue;
                    }
                }
            }
            let Ok(project) = provider.get_project(&project_id) else {
                continue;
            };
            let side = tuffbox_core::manifest::Side::from_modrinth(
                project.client_side.as_deref(),
                project.server_side.as_deref(),
            );
            if manifest.mods[idx].side != side {
                manifest.mods[idx].side = side;
                any_changes = true;
            }
            if let Some(ref sha1) = sha1 {
                hash_index.put_sides(
                    sha1,
                    project.client_side.as_deref(),
                    project.server_side.as_deref(),
                );
                index_dirty = true;
            }
        }

        if any_changes {
            // Re-load disk before save so we don't clobber concurrent updates
            // that finished after our initial load (Update All race).
            if let Ok(disk) = ProjectManifest::load_from_path(&manifest_path) {
                for disk_mod in &disk.mods {
                    if let Some(pid) = disk_mod.source.project_id.as_ref() {
                        if let Some(idx) = manifest.mods.iter().position(|m| {
                            m.source.project_id.as_ref() == Some(pid) || m.id == disk_mod.id
                        }) {
                            // Prefer the newer file_id / version from disk when present.
                            if disk_mod.source.file_id != manifest.mods[idx].source.file_id
                                || disk_mod.version != manifest.mods[idx].version
                            {
                                manifest.mods[idx] = disk_mod.clone();
                            }
                        }
                    }
                }
            }
            save_manifest(&manifest_path, &manifest).map_err(|e| e.to_string())?;
        }
        if index_dirty {
            let _ = hash_index.save(&project_dir);
        }

        list_mods_impl(&path)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Copy local jars/zips into the project content folders, then register via sync.
#[tauri::command(rename_all = "camelCase")]
async fn import_local_content_files(
    path: String,
    source_paths: Vec<String>,
    content_type: Option<String>,
) -> Result<serde_json::Value, String> {
    let copy_result = tokio::task::spawn_blocking({
        let path = path.clone();
        move || {
            let _guard = MODS_IO_LOCK
                .lock()
                .map_err(|_| "mods I/O lock poisoned".to_string())?;
            let manifest_path = PathBuf::from(&path);
            let project_dir = if manifest_path.is_file() {
                manifest_path
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| PathBuf::from("."))
            } else {
                manifest_path.clone()
            };

            let ct = content_type
                .as_deref()
                .unwrap_or("mod")
                .to_lowercase();
            let default_route = match ct.as_str() {
                "resourcepack" | "resourcepacks" => ("resourcepacks", "zip"),
                "shader" | "shaderpack" | "shaderpacks" => ("shaderpacks", "zip"),
                "datapack" | "datapacks" => ("datapacks", "zip"),
                _ => ("mods", "jar"),
            };

            auto_snapshot(&manifest_path, "import-local-content").map_err(|e| e.to_string())?;

            let mut imported = Vec::new();
            let mut skipped = Vec::new();

            for src_raw in source_paths {
                let src = PathBuf::from(&src_raw);
                if !src.is_file() {
                    skipped.push(format!("{}: not a file", src_raw));
                    continue;
                }
                let ext = src
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                let base_name = src
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| format!("import.{}", default_route.1));

                // Filename heuristics (e.g. VanillaTweaks_*.zip) override a generic
                // "mod" import so custom texture packs land in resourcepacks/.
                let (dir_name, expected_ext) = if ct == "mod" && ext == "zip" {
                    match tuffbox_core::manifest::ContentType::from_filename(&base_name) {
                        tuffbox_core::manifest::ContentType::Resourcepack => {
                            ("resourcepacks", "zip")
                        }
                        tuffbox_core::manifest::ContentType::Shaderpack => ("shaderpacks", "zip"),
                        tuffbox_core::manifest::ContentType::Datapack => ("datapacks", "zip"),
                        tuffbox_core::manifest::ContentType::Mod => default_route,
                    }
                } else {
                    default_route
                };

                if ext != expected_ext {
                    skipped.push(format!(
                        "{}: expected .{}, got .{}",
                        base_name, expected_ext, ext
                    ));
                    continue;
                }
                let dest_dir = project_dir.join(dir_name);
                std::fs::create_dir_all(&dest_dir).map_err(|e| e.to_string())?;
                let mut dest_name = base_name.clone();
                let mut dest = dest_dir.join(&dest_name);
                if dest.exists() {
                    // Same path already in folder — skip duplicate copy.
                    if std::fs::canonicalize(&src).ok() == std::fs::canonicalize(&dest).ok() {
                        skipped.push(format!("{}: already in place", base_name));
                        continue;
                    }
                    let stem = base_name
                        .trim_end_matches(&format!(".{}", expected_ext))
                        .to_string();
                    let mut n = 2u32;
                    loop {
                        dest_name = format!("{}-{}.{}", stem, n, expected_ext);
                        dest = dest_dir.join(&dest_name);
                        if !dest.exists() {
                            break;
                        }
                        n += 1;
                        if n > 1000 {
                            skipped.push(format!("{}: could not pick unique name", base_name));
                            break;
                        }
                    }
                    if dest.exists() {
                        continue;
                    }
                }
                std::fs::copy(&src, &dest).map_err(|e| {
                    format!("copy {}: {}", src.display(), e)
                })?;
                imported.push(format!("{}/{}", dir_name, dest_name));
            }

            Ok::<(Vec<String>, Vec<String>), String>((imported, skipped))
        }
    })
    .await
    .map_err(|e| e.to_string())??;

    let (imported, skipped) = copy_result;
    // Register new files (Modrinth identify or Local) via existing sync path.
    let before_ids: std::collections::HashSet<String> = {
        let listed = list_mods(path.clone()).await.unwrap_or_default();
        listed
            .into_iter()
            .filter_map(|v| {
                v.get("id")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string())
            })
            .collect()
    };
    let _mods = sync_mods_folder(path.clone()).await?;
    let after = list_mods(path).await.unwrap_or_default();
    let identified: Vec<String> = after
        .iter()
        .filter_map(|v| {
            let id = v.get("id")?.as_str()?;
            if before_ids.contains(id) {
                return None;
            }
            let source = v.get("source")?.as_str().unwrap_or("local");
            if source != "local" {
                Some(id.to_string())
            } else {
                None
            }
        })
        .collect();

    Ok(serde_json::json!({
        "imported": imported,
        "identified": identified,
        "skipped": skipped,
        "baselineUpdated": true,
    }))
}

/// Resolve a jar to a ModSpec via the local hash index, refreshing Modrinth
/// side fields once if the cache predates client_side/server_side storage.
fn resolve_mod_from_hash_or_modrinth(
    provider: &tuffbox_core::ModrinthProvider,
    hash_index: &mut tuffbox_core::ModHashIndex,
    sha1: &str,
    jar_path: &Path,
    file_name: String,
    index_dirty: &mut bool,
) -> Option<tuffbox_core::manifest::ModSpec> {
    let cached_status = hash_index.get(sha1).map(|c| {
        (
            c.status.clone(),
            c.client_side.is_none() && c.server_side.is_none(),
            c.project_id.clone(),
        )
    });
    if let Some((status, needs_sides, project_id)) = cached_status {
        if status == "miss" {
            return None;
        }
        if status == "modrinth" {
            if needs_sides {
                if let Some(pid) = project_id {
                    if let Ok(project) = provider.get_project(&pid) {
                        hash_index.put_sides(
                            sha1,
                            project.client_side.as_deref(),
                            project.server_side.as_deref(),
                        );
                        *index_dirty = true;
                    }
                }
            }
            return hash_index
                .get(sha1)
                .and_then(|c| c.to_mod_spec(file_name, tuffbox_core::manifest::Side::Unknown));
        }
    }

    match tuffbox_core::identify_local_jar_via_modrinth(provider, jar_path) {
        Ok(Some((spec, client_side, server_side))) => {
            hash_index.put_modrinth(
                sha1,
                &spec,
                client_side.as_deref(),
                server_side.as_deref(),
            );
            *index_dirty = true;
            Some(spec)
        }
        Ok(None) => {
            hash_index.put_miss(sha1);
            *index_dirty = true;
            None
        }
        Err(_) => None,
    }
}

fn list_mods_impl(path: &str) -> Result<Vec<serde_json::Value>, String> {
    let manifest = ProjectManifest::load_from_path(path).map_err(|e| e.to_string())?;
    let mods = manifest
        .mods
        .into_iter()
        .map(|m| {
            let content_type = match m.content_type {
                tuffbox_core::manifest::ContentType::Mod => "mod",
                tuffbox_core::manifest::ContentType::Resourcepack => "resourcepack",
                tuffbox_core::manifest::ContentType::Shaderpack => "shader",
                tuffbox_core::manifest::ContentType::Datapack => "datapack",
            };
            let icon_url: Option<String> = match &m.source.kind {
                tuffbox_core::manifest::SourceKind::Modrinth => {
                    m.source.icon_url.clone().or_else(|| {
                        m.source
                            .project_id
                            .as_ref()
                            .map(|pid| format!("https://cdn.modrinth.com/data/{pid}/icon.png"))
                    })
                }
                tuffbox_core::manifest::SourceKind::Curseforge => m.source.icon_url.clone(),
                _ => m.source.icon_url.clone(),
            };
            let disabled = m
                .status
                .iter()
                .any(|s| s.eq_ignore_ascii_case("disabled"));
            serde_json::json!({
                "id": m.id,
                "name": m.name,
                "version": m.version,
                "side": format!("{:?}", m.side).to_lowercase(),
                "source": m.source.kind.as_str(),
                "projectId": m.source.project_id,
                "fileName": m.file_name,
                "iconUrl": icon_url,
                "contentType": content_type,
                "disabled": disabled,
                "status": m.status,
            })
        })
        .collect();
    Ok(mods)
}

#[tauri::command(rename_all = "camelCase")]
async fn list_mods(path: String) -> Result<Vec<serde_json::Value>, String> {
    tokio::task::spawn_blocking(move || list_mods_impl(&path))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
async fn search_unified_mods(
    path: String,
    query: String,
    game_version: Option<String>,
    loader: Option<String>,
    content_type: Option<String>,
    page: Option<u32>,
    page_size: Option<u32>,
) -> Result<PagedCatalog, String> {
    tokio::task::spawn_blocking(move || {
        let path = resolve_manifest_path(&path)?;
        let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
        let page_size = page_size.unwrap_or(30).clamp(1, 100);
        let page = page.unwrap_or(1).max(1);
        let offset = (page - 1) * page_size;
        let per = (page_size / 2).max(1);

        let mr = tuffbox_core::ModrinthProvider::new();
        let default_loader =
            tuffbox_core::graph::loader_kind_slug(&manifest.loader.kind).to_string();
        let mr_total;
        let mut mr_hits: Vec<serde_json::Value> = Vec::new();
        if let Ok(page_result) = mr.search(&ProviderSearchQuery {
            query: Some(query.clone()),
            minecraft_version: game_version
                .clone()
                .or_else(|| Some(manifest.minecraft.version.clone())),
            loader: loader.clone().or_else(|| Some(default_loader)),
            limit: Some(per),
            project_type: content_type.clone(),
            offset: Some(offset / 2),
            ..Default::default()
        }) {
            mr_total = page_result.total;
            for p in page_result.results {
                let value = serde_json::to_value(&p).unwrap_or(serde_json::Value::Null);
                let mut obj = match value {
                    serde_json::Value::Object(m) => m,
                    _ => serde_json::Map::new(),
                };
                obj.insert("provider".into(), serde_json::json!("modrinth"));
                mr_hits.push(serde_json::Value::Object(obj));
            }
        } else {
            mr_total = 0;
        }

        let mut cf_hits: Vec<serde_json::Value> = Vec::new();
        let cf_total;
        let cf_provider = tuffbox_core::CurseForgeProvider::new();
        if cf_provider.is_configured() {
            let project_type = content_type.clone().unwrap_or_else(|| "mod".into());
            let class_id = tuffbox_core::CurseForgeProvider::class_id_for_project_type(&project_type);
            let gv = game_version.unwrap_or_else(|| manifest.minecraft.version.clone());
            let loader_slug = loader
                .clone()
                .unwrap_or_else(|| tuffbox_core::graph::loader_kind_slug(&manifest.loader.kind).to_string());
            let mod_loader = if project_type == "mod" {
                tuffbox_core::CurseForgeProvider::mod_loader_type(&loader_slug)
            } else {
                None
            };
            if let Ok(page_result) = cf_provider.search_content(
                class_id,
                &query,
                Some(&gv),
                mod_loader,
                offset / 2,
                per,
                None,
                None,
            ) {
                cf_total = page_result.total;
                for hit in page_result.hits {
                    let mapped_type = match hit.class_id.unwrap_or(class_id) {
                        12 => "resourcepack",
                        6552 => "shader",
                        6945 => "datapack",
                        4471 => "modpack",
                        _ => "mod",
                    };
                    cf_hits.push(serde_json::json!({
                        "id": hit.id.to_string(),
                        "slug": hit.slug,
                        "name": hit.name,
                        "description": hit.summary,
                        "projectType": mapped_type,
                        "iconUrl": hit.icon_url,
                        "author": hit.authors.first().cloned(),
                        "downloads": hit.download_count,
                        "follows": hit.thumbs_up_count,
                        "dateModified": hit.date_modified.clone().or(hit.date_created.clone()),
                        "categories": hit.categories,
                        "provider": "curseforge",
                    }));
                }
            } else {
                cf_total = 0;
            }
        } else {
            cf_total = 0;
        }

        let mut results: Vec<serde_json::Value> = Vec::with_capacity(page_size as usize);
        let max = mr_hits.len().max(cf_hits.len());
        for i in 0..max {
            if i < mr_hits.len() {
                results.push(mr_hits[i].clone());
            }
            if i < cf_hits.len() {
                results.push(cf_hits[i].clone());
            }
        }
        results.truncate(page_size as usize);

        Ok(PagedCatalog {
            results,
            total: mr_total.saturating_add(cf_total),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Provider-neutral catalog search used by the content browser. The unified provider
/// keeps provider selection in the UI while preserving the existing search adapters.
#[tauri::command(rename_all = "camelCase")]
async fn search_content(
    path: String,
    query: String,
    provider: Option<String>,
    game_version: Option<String>,
    loader: Option<String>,
    content_type: Option<String>,
    page: Option<u32>,
    page_size: Option<u32>,
) -> Result<PagedCatalog, String> {
    let _provider = provider.unwrap_or_else(|| "both".to_string());
    search_unified_mods(path, query, game_version, loader, content_type, page, page_size).await
}

#[tauri::command(rename_all = "camelCase")]
async fn search_modrinth_mods(
    path: String,
    query: String,
    game_version: Option<String>,
    loader: Option<String>,
    category: Option<String>,
    environment: Option<String>,
    license: Option<String>,
    sort: Option<String>,
    content_type: Option<String>,
    page: Option<u32>,
    page_size: Option<u32>,
) -> Result<PagedCatalog, String> {
    tokio::task::spawn_blocking(move || {
        // A manifest is only needed to infer a default loader / game version.
        // When no project is open (empty path) we still allow browsing the
        // Modrinth catalog with the caller-supplied filters.
        let path = resolve_manifest_path(&path).unwrap_or_else(|_| PathBuf::from(&path));
        let manifest = ProjectManifest::load_from_path(&path).ok();
        let provider = tuffbox_core::ModrinthProvider::new();
        let default_loader = manifest
            .as_ref()
            .map(|m| tuffbox_core::graph::loader_kind_slug(&m.loader.kind).to_string());
        let page_size = page_size.unwrap_or(30).clamp(1, 100);
        let offset = (page.unwrap_or(1).saturating_sub(1)) * page_size;
        let page_result = provider
            .search(&ProviderSearchQuery {
                query: Some(query),
                minecraft_version: game_version
                    .or_else(|| manifest.as_ref().map(|m| m.minecraft.version.clone())),
                loader: loader.or(default_loader),
                category,
                environment,
                license,
                sort,
                limit: Some(page_size),
                project_type: content_type,
                offset: Some(offset),
            })
            .map_err(|e| e.to_string())?;
        let results = page_result
            .results
            .into_iter()
            .map(|p| {
                let value = serde_json::to_value(&p).unwrap_or(serde_json::Value::Null);
                let mut obj = match value {
                    serde_json::Value::Object(m) => m,
                    _ => serde_json::Map::new(),
                };
                obj.insert("provider".into(), serde_json::json!("modrinth"));
                serde_json::Value::Object(obj)
            })
            .collect();
        Ok(PagedCatalog {
            results,
            total: page_result.total,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Search modpacks via hub (preferred) or direct Modpack Index fallback.
/// Hub uses TuffSwarm-Analytics UA so end-user IPs are not sent to MPI.
#[tauri::command(rename_all = "camelCase")]
async fn search_modpack_index(
    query: Option<String>,
    page: Option<u32>,
    limit: Option<u32>,
    category_id: Option<u32>,
    version: Option<String>,
    launcher_id: Option<u32>,
) -> Result<PagedCatalog, String> {
    let limit = limit.unwrap_or(12).clamp(1, 100);
    let page = page.unwrap_or(1).max(1);

    // Prefer hub proxy when TuffSwarm hub URL is configured.
    if let Some(endpoint) = integrations::swarm_network_base() {
        let token = integrations::secret_optional("crash_kb");
        if let Ok(body) = tuffbox_core::crash_remote::fetch_modpacks_async(
            &endpoint,
            token.as_deref(),
            query.as_deref(),
            page,
            limit,
            category_id,
            version.as_deref(),
        )
        .await
        {
            let results = body
                .get("results")
                .cloned()
                .unwrap_or(serde_json::json!([]));
            let total = body.get("total").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let results: Vec<serde_json::Value> =
                serde_json::from_value(results).unwrap_or_default();
            return Ok(PagedCatalog { results, total });
        }
    }

    let q = query.clone();
    let version_s = version.clone();
    tokio::task::spawn_blocking(move || {
        let version_id = version_s
            .as_deref()
            .and_then(tuffbox_core::modpack_index::resolve_mc_version_id);
        let (results, total) = tuffbox_core::modpack_index::search_modpacks(
            q.as_deref(),
            page,
            limit,
            category_id,
            version_id,
            launcher_id,
        )?;
        Ok(PagedCatalog { results, total })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
async fn list_modpack_index_categories() -> Result<Vec<tuffbox_core::modpack_index::MpiCategory>, String>
{
    // Prefer hub (MPI behind analytics UA). Offline fallback: static pack themes only — no client MPI scrape.
    if let Some(endpoint) = integrations::swarm_network_base() {
        let token = integrations::secret_optional("crash_kb");
        if let Ok(body) =
            tuffbox_core::crash_remote::fetch_modpack_categories_async(&endpoint, token.as_deref())
                .await
        {
            if let Some(arr) = body.get("categories").cloned() {
                if let Ok(cats) = serde_json::from_value::<
                    Vec<tuffbox_core::modpack_index::MpiCategory>,
                >(arr)
                {
                    if !cats.is_empty() {
                        return Ok(cats);
                    }
                }
            }
        }
    }
    Ok(tuffbox_core::modpack_index::list_pack_theme_categories())
}

#[tauri::command(rename_all = "camelCase")]
async fn search_modpack_index_mods(
    query: Option<String>,
    page: Option<u32>,
    limit: Option<u32>,
    category_id: Option<u32>,
) -> Result<PagedCatalog, String> {
    // Rare path — keep direct MPI for now; prefer Modrinth catalog for mod discovery in UI.
    let limit = limit.unwrap_or(12).clamp(1, 100);
    let page = page.unwrap_or(1).max(1);
    let q = query.clone();
    tokio::task::spawn_blocking(move || {
        let (results, total) =
            tuffbox_core::modpack_index::search_mods(q.as_deref(), page, limit, category_id)?;
        Ok(PagedCatalog { results, total })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
async fn search_curseforge_mods(
    path: String,
    query: String,
    game_version: Option<String>,
    loader: Option<String>,
    content_type: Option<String>,
    category_id: Option<u32>,
    page: Option<u32>,
    page_size: Option<u32>,
    sort_field: Option<u32>,
) -> Result<PagedCatalog, String> {
    tokio::task::spawn_blocking(move || {
        let path = resolve_manifest_path(&path)?;
        let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
        let provider = tuffbox_core::CurseForgeProvider::new();
        if !provider.is_configured() {
            return Err("CurseForge API key is not configured".to_string());
        }
        let project_type = content_type.unwrap_or_else(|| "mod".into());
        let class_id = tuffbox_core::CurseForgeProvider::class_id_for_project_type(&project_type);
        let gv = game_version.unwrap_or_else(|| manifest.minecraft.version.clone());
        let loader_slug = loader
            .clone()
            .unwrap_or_else(|| tuffbox_core::graph::loader_kind_slug(&manifest.loader.kind).to_string());
        let mod_loader = if project_type == "mod" {
            tuffbox_core::CurseForgeProvider::mod_loader_type(&loader_slug)
        } else {
            None
        };
        let page_size = page_size.unwrap_or(30).clamp(1, 50);
        let offset = (page.unwrap_or(1).saturating_sub(1)) * page_size;
        let sort_field = sort_field.unwrap_or(2);
        let page_result = provider
            .search_content(
                class_id,
                &query,
                Some(&gv),
                mod_loader,
                offset,
                page_size,
                Some(sort_field),
                category_id,
            )
            .map_err(|e| e.to_string())?;
        let results = page_result
            .hits
            .into_iter()
            .map(|hit| {
                let mapped_type = match hit.class_id.unwrap_or(class_id) {
                    12 => "resourcepack",
                    6552 => "shader",
                    6945 => "datapack",
                    4471 => "modpack",
                    _ => "mod",
                };
                serde_json::json!({
                    "id": hit.id.to_string(),
                    "slug": hit.slug,
                    "name": hit.name,
                    "description": hit.summary,
                    "projectType": mapped_type,
                    "iconUrl": hit.icon_url,
                    "author": hit.authors.first().cloned(),
                    "downloads": hit.download_count,
                    "follows": hit.thumbs_up_count,
                    "dateModified": hit.date_modified.clone().or(hit.date_created.clone()),
                    "categories": hit.categories,
                    "provider": "curseforge",
                })
            })
            .collect();
        Ok(PagedCatalog {
            results,
            total: page_result.total,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
async fn add_curseforge_mod(
    app: tauri::AppHandle,
    path: String,
    mod_id: String,
    side: String,
) -> Result<(), String> {
    let path_for_stats = path.clone();
    tokio::task::spawn_blocking(move || {
        let manifest_path = PathBuf::from(&path);
        let mut snapshot = auto_snapshot_before_mod_op(&manifest_path, "add-curseforge-mod")
            .map_err(|e| e.to_string())?;
        let mut manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
        let requested = vec![mod_id];
        let installed =
            install_curseforge_with_dependencies_rounds(&mut manifest, &requested, &side, 50)
                .map_err(|e| e.to_string())?;
        save_manifest(&manifest_path, &manifest).map_err(|e| e.to_string())?;
        download_project_mods_tracked(&app, &manifest_path, &manifest, None, true);
        let related: Vec<&ModSpec> = installed
            .iter()
            .filter_map(|id| {
                manifest.mods.iter().find(|m| {
                    m.id == *id || m.source.project_id.as_deref() == Some(id.as_str())
                })
            })
            .collect();
        let lines = mod_install_history_lines(&related, &requested);
        finalize_mod_history(
            &manifest_path,
            &mut snapshot,
            "add-curseforge-mod",
            &lines,
            &related,
            &[],
        );
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| e.to_string())??;
    swarm_api::spawn_pack_cooccurrence(path_for_stats, "mod_install");
    Ok(())
}

#[tauri::command(rename_all = "camelCase")]
async fn add_curseforge_mods_with_dependencies(
    app: tauri::AppHandle,
    path: String,
    mod_ids: Vec<String>,
    side: String,
) -> Result<Vec<String>, String> {
    let path_for_stats = path.clone();
    let installed = tokio::task::spawn_blocking(move || {
        let manifest_path = PathBuf::from(&path);
        let mut snapshot = auto_snapshot_before_mod_op(
            &manifest_path,
            "bulk-add-curseforge-mods-with-dependencies",
        )
        .map_err(|e| e.to_string())?;
        let mut manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
        let installed =
            install_curseforge_with_dependencies_rounds(&mut manifest, &mod_ids, &side, 50)
                .map_err(|e| e.to_string())?;
        save_manifest(&manifest_path, &manifest).map_err(|e| e.to_string())?;
        download_project_mods_tracked(&app, &manifest_path, &manifest, None, true);
        let related: Vec<&ModSpec> = installed
            .iter()
            .filter_map(|id| {
                manifest.mods.iter().find(|m| {
                    m.id == *id || m.source.project_id.as_deref() == Some(id.as_str())
                })
            })
            .collect();
        let lines = mod_install_history_lines(&related, &mod_ids);
        finalize_mod_history(
            &manifest_path,
            &mut snapshot,
            "bulk-add-curseforge-mods-with-dependencies",
            &lines,
            &related,
            &[],
        );
        Ok::<Vec<String>, String>(installed)
    })
    .await
    .map_err(|e| e.to_string())??;
    swarm_api::spawn_pack_cooccurrence(path_for_stats, "mod_install");
    Ok(installed)
}

/// Whether `target` (slug or provider project id) is already in the manifest.
fn manifest_has_dependency_target(manifest: &ProjectManifest, target: &str) -> bool {
    manifest.mods.iter().any(|m| {
        m.id == target || m.source.project_id.as_deref() == Some(target)
    })
}

fn installed_dependency_targets(
    manifest: &ProjectManifest,
    dependencies: &[tuffbox_core::ModDependencySpec],
) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for dep in dependencies {
        if !seen.insert(dep.target.as_str()) {
            continue;
        }
        if manifest_has_dependency_target(manifest, &dep.target) {
            out.push(dep.target.clone());
        }
    }
    out
}

#[tauri::command(rename_all = "camelCase")]
async fn preview_modrinth_install(
    path: String,
    mod_id: String,
) -> Result<ModInstallPreview, String> {
    tokio::task::spawn_blocking(move || {
        let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
        let provider = tuffbox_core::ModrinthProvider::new();
        let project = provider.get_project(&mod_id).map_err(|e| e.to_string())?;
        let query = ProviderSearchQuery {
            query: None,
            minecraft_version: Some(manifest.minecraft.version.clone()),
            loader: Some(tuffbox_core::graph::loader_kind_slug(&manifest.loader.kind).to_string()),
            ..Default::default()
        };
        let version = provider
            .get_versions(&mod_id, &query)
            .map_err(|e| e.to_string())?
            .into_iter()
            .next()
            .ok_or_else(|| format!("no compatible version found for {mod_id}"))?;
        let file_name = ProviderFileInfo::select_file_for_loader(
            &version,
            &tuffbox_core::graph::loader_kind_slug(&manifest.loader.kind),
        )
        .map(|file| file.filename.clone());
        let dependencies = provider
            .resolve_dependencies(&version.id)
            .unwrap_or_default();
        let installed_dependencies = installed_dependency_targets(&manifest, &dependencies);
        let dependents = provider
            .search_dependents(&project.id, 8)
            .into_iter()
            .map(|p| ModInstallDependent {
                id: p.id,
                slug: p.slug,
                name: p.name,
            })
            .collect();
        let side = format!("{:?}", infer_project_side(Some(&project))).to_lowercase();
        Ok(ModInstallPreview {
            project_id: project.id,
            slug: project.slug,
            name: project.name,
            version: version.version_number,
            file_name,
            side,
            dependencies,
            installed_dependencies,
            dependents,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
async fn preview_curseforge_install(
    path: String,
    mod_id: String,
) -> Result<ModInstallPreview, String> {
    tokio::task::spawn_blocking(move || {
        let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
        let project_id: u64 = mod_id
            .parse()
            .map_err(|_| format!("invalid CurseForge project id: {mod_id}"))?;
        let provider = tuffbox_core::CurseForgeProvider::new();
        if !provider.is_configured() {
            return Err("CurseForge API key is not configured".into());
        }
        let hit = provider.get_mod(project_id).map_err(|e| e.to_string())?;
        let loader = tuffbox_core::graph::loader_kind_slug(&manifest.loader.kind).to_string();
        let mc = manifest.minecraft.version.clone();
        let mut files = provider
            .get_mod_files(project_id, Some(&mc))
            .map_err(|e| e.to_string())?;
        if files.is_empty() {
            files = provider
                .get_mod_files(project_id, None)
                .map_err(|e| e.to_string())?;
        }
        let chosen = tuffbox_core::CurseForgeProvider::pick_best_file(&files, &mc, &loader)
            .cloned()
            .ok_or_else(|| format!("no compatible CurseForge file for {mod_id}"))?;
        let file = provider.get_file(project_id, chosen.id).unwrap_or(chosen);
        let slug = if hit.slug.is_empty() {
            format!("cf-{project_id}")
        } else {
            hit.slug.clone()
        };
        let dependencies =
            tuffbox_core::provider::curseforge::cf_deps_to_specs(&file.dependencies);
        let installed_dependencies = installed_dependency_targets(&manifest, &dependencies);
        Ok(ModInstallPreview {
            project_id: project_id.to_string(),
            slug,
            name: hit.name,
            version: file.display_name,
            file_name: Some(file.file_name),
            side: "both".into(),
            dependencies,
            installed_dependencies,
            dependents: Vec::new(),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
async fn get_modrinth_project_icon(project_id: String) -> Result<Option<String>, String> {
    tokio::task::spawn_blocking(move || {
        let provider = tuffbox_core::ModrinthProvider::new();
        provider
            .get_project(&project_id)
            .map(|project| project.icon_url)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
async fn get_modrinth_project(project_id: String) -> Result<tuffbox_core::ProjectInfo, String> {
    tokio::task::spawn_blocking(move || {
        let provider = tuffbox_core::ModrinthProvider::new();
        provider.get_project(&project_id).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Live Modrinth category tags (`GET /v2/tag/category`), optionally filtered by project type.
#[tauri::command(rename_all = "camelCase")]
async fn list_modrinth_categories(
    project_type: Option<String>,
) -> Result<Vec<tuffbox_core::ModrinthCategory>, String> {
    tokio::task::spawn_blocking(move || {
        tuffbox_core::ModrinthProvider::new()
            .list_categories(project_type.as_deref())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// CurseForge categories for a project type (resolved to its class id).
#[tauri::command(rename_all = "camelCase")]
async fn list_curseforge_categories(
    project_type: Option<String>,
) -> Result<Vec<serde_json::Value>, String> {
    tokio::task::spawn_blocking(move || {
        let provider = tuffbox_core::CurseForgeProvider::new();
        if !provider.is_configured() {
            return Err("CurseForge API key is not configured".to_string());
        }
        let class_id = tuffbox_core::CurseForgeProvider::class_id_for_project_type(
            project_type.as_deref().unwrap_or("mod"),
        );
        provider
            .list_categories(class_id)
            .map(|cats| {
                cats.into_iter()
                    .map(|(id, name, parent)| {
                        serde_json::json!({
                            "id": id,
                            "name": name,
                            "parentCategoryId": parent,
                        })
                    })
                    .collect()
            })
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}


/// Unified catalog project detail for the in-launcher project page
/// (Modrinth or CurseForge), GDLauncher-style.
#[tauri::command(rename_all = "camelCase")]
async fn get_catalog_project(
    provider: String,
    project_id: String,
) -> Result<serde_json::Value, String> {
    tokio::task::spawn_blocking(move || {
        let provider = provider.trim().to_ascii_lowercase();
        if provider == "curseforge" || provider == "cf" {
            let id: u64 = project_id
                .trim()
                .parse()
                .map_err(|_| format!("Invalid CurseForge project id: {project_id}"))?;
            let cf = tuffbox_core::CurseForgeProvider::new();
            if !cf.is_configured() {
                return Err("CurseForge API key is not configured".into());
            }
            let hit = cf.get_mod(id).map_err(|e| e.to_string())?;
            let description_html = cf.get_mod_description_html(id).unwrap_or_default();
            let mapped_type = match hit.class_id.unwrap_or(6) {
                12 => "resourcepack",
                6552 => "shader",
                6945 => "datapack",
                4471 => "modpack",
                _ => "mod",
            };
            let links = cf.get_mod_links(id).unwrap_or_default();
            return Ok(serde_json::json!({
                "id": hit.id.to_string(),
                "slug": hit.slug,
                "name": hit.name,
                "description": hit.summary,
                "descriptionHtml": description_html,
                "projectType": mapped_type,
                "iconUrl": hit.icon_url,
                "author": hit.authors.first().cloned(),
                "authors": hit.authors,
                "downloads": hit.download_count,
                "follows": hit.thumbs_up_count,
                "dateModified": hit.date_modified.clone().or(hit.date_created.clone()),
                "dateCreated": hit.date_created,
                "categories": hit.categories,
                "issuesUrl": links.issues_url,
                "sourceUrl": links.source_url,
                "wikiUrl": links.wiki_url,
                "discordUrl": links.discord_url,
                "donateUrl": links.donate_url,
                "provider": "curseforge",
            }));
        }

        let mr = tuffbox_core::ModrinthProvider::new();
        let detail = mr
            .get_project_detail(&project_id)
            .map_err(|e| e.to_string())?;
        let project = detail.project;
        let description_html = detail
            .body
            .as_deref()
            .map(tuffbox_core::markdown_to_html)
            .filter(|s| !s.trim().is_empty());
        Ok(serde_json::json!({
            "id": project.id,
            "slug": project.slug,
            "name": project.name,
            "description": project.description,
            "descriptionHtml": description_html,
            "projectType": project.project_type,
            "iconUrl": project.icon_url,
            "author": project.author,
            "authors": project.author.clone().map(|a| vec![a]).unwrap_or_default(),
            "downloads": project.downloads,
            "follows": project.follows,
            "dateModified": project.date_modified,
            "dateCreated": project.date_created,
            "categories": project.categories,
            "license": project.license,
            "clientSide": project.client_side,
            "serverSide": project.server_side,
            "issuesUrl": project.issues_url,
            "sourceUrl": project.source_url,
            "discordUrl": detail.discord_url,
            "wikiUrl": detail.wiki_url,
            "donateUrl": detail.donate_url,
            "loaders": detail.loaders,
            "gameVersions": detail.game_versions,
            "gallery": detail.gallery,
            "creators": detail.creators,
            "provider": "modrinth",
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Versions/files for the in-launcher project page.
#[tauri::command(rename_all = "camelCase")]
async fn get_catalog_versions(
    provider: String,
    project_id: String,
    minecraft_version: Option<String>,
    loader: Option<String>,
) -> Result<Vec<serde_json::Value>, String> {
    let provider_l = provider.trim().to_ascii_lowercase();
    if provider_l == "curseforge" || provider_l == "cf" {
        return tokio::task::spawn_blocking(move || {
            let id: u64 = project_id
                .trim()
                .parse()
                .map_err(|_| format!("Invalid CurseForge project id: {project_id}"))?;
            let cf = tuffbox_core::CurseForgeProvider::new();
            if !cf.is_configured() {
                return Err("CurseForge API key is not configured".into());
            }
            let gv = minecraft_version.as_deref().filter(|s| !s.is_empty());
            let files = cf.get_mod_files(id, gv).map_err(|e| e.to_string())?;
            let loader_slug = loader
                .as_deref()
                .map(|l| l.trim().to_lowercase())
                .filter(|l| !l.is_empty());
            let mut rows: Vec<serde_json::Value> = files
                .into_iter()
                .map(|f| {
                    let mc_ok = gv
                        .map(|v| f.game_versions.iter().any(|g| g == v))
                        .unwrap_or(true);
                    let loader_ok = match &loader_slug {
                        Some(l) => f
                            .game_versions
                            .iter()
                            .any(|g| g.eq_ignore_ascii_case(l) || (*l == "quilt" && g.eq_ignore_ascii_case("fabric"))),
                        None => true,
                    };
                    let channel = match f.release_type {
                        1 => "release",
                        2 => "beta",
                        3 => "alpha",
                        _ => "release",
                    };
                    serde_json::json!({
                        "id": f.id.to_string(),
                        "versionNumber": f.display_name,
                        "name": f.file_name,
                        "gameVersions": f.game_versions,
                        "loaders": [],
                        "datePublished": f.file_date,
                        "versionType": channel,
                        // CurseForge ships the changelog as HTML already.
                        "changelogHtml": f.changelog,
                        "compatible": mc_ok && loader_ok,
                        "compatibleMinecraft": mc_ok,
                        "compatibleLoader": loader_ok,
                    })
                })
                .collect();
            rows.sort_by(|a, b| {
                let ad = a.get("datePublished").and_then(|v| v.as_str()).unwrap_or("");
                let bd = b.get("datePublished").and_then(|v| v.as_str()).unwrap_or("");
                bd.cmp(ad)
            });
            Ok(rows)
        })
        .await
        .map_err(|e| e.to_string())?;
    }

    get_mod_versions(project_id, minecraft_version.unwrap_or_default(), loader).await
}

/// Resolves the download URL of the latest Modrinth modpack file (.mrpack) for
/// a project, so the Library "Discover" tab can import a remote pack directly
/// via `install_modpack`.
#[tauri::command(rename_all = "camelCase")]
async fn get_modrinth_pack_download(project_id: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let provider = tuffbox_core::ModrinthProvider::new();
        let versions = provider
            .get_versions(&project_id, &ProviderSearchQuery::default())
            .map_err(|e| e.to_string())?;
        for version in &versions {
            for file in &version.files {
                if file.filename.to_lowercase().ends_with(".mrpack") {
                    return Ok(file.url.clone());
                }
            }
        }
        // Fallback: any primary file if no .mrpack is published.
        for version in &versions {
            if let Some(primary) = version.files.iter().find(|f| f.primary) {
                return Ok(primary.url.clone());
            }
        }
        Err("No downloadable file found for this modpack.".into())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Per-project user state for mods found in the Add-Mod browser.
/// `favorites` is a single set of mod IDs the user liked.
/// `lists` is a map of list_name -> ordered list of mod IDs, supporting
/// multiple named build lists (e.g. "Performance", "PvP", "QoL").
/// `ratings` stores per-mod star ratings (0–5).
/// Stored as JSON under `.tuffbox/` so it survives restarts without
/// polluting the manifest.

fn mod_user_state_path(project_dir: &Path) -> PathBuf {
    project_dir.join(".tuffbox").join("mods_user_state.json")
}

fn load_mod_user_state(project_dir: &Path) -> ModUserState {
    let p = mod_user_state_path(project_dir);
    std::fs::read_to_string(&p)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_mod_user_state(project_dir: &Path, state: &ModUserState) -> Result<(), String> {
    let p = mod_user_state_path(project_dir);
    if let Some(par) = p.parent() {
        std::fs::create_dir_all(par).map_err(|e| e.to_string())?;
    }
    std::fs::write(
        &p,
        serde_json::to_string_pretty(state).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
fn get_mod_user_state(path: String) -> Result<ModUserState, String> {
    let project_dir = manifest_parent(&path)?;
    Ok(load_mod_user_state(&project_dir))
}

#[tauri::command(rename_all = "camelCase")]
fn set_mod_user_state(
    path: String,
    mod_id: String,
    favorite: Option<bool>,
    saved: Option<bool>,
    rating: Option<u8>,
) -> Result<ModUserState, String> {
    let project_dir = manifest_parent(&path)?;
    let mut state = load_mod_user_state(&project_dir);
    if let Some(f) = favorite {
        if f {
            state.favorites.insert(mod_id.clone(), true);
        } else {
            state.favorites.remove(&mod_id);
        }
    }
    // Legacy `saved` flag is kept for backward compat: adds/removes the mod
    // from a default list named "Saved". New UI should use `add_to_list` /
    // `remove_from_list` / `create_list` / `delete_list` instead.
    if let Some(s) = saved {
        const DEFAULT_LIST: &str = "Saved";
        let entry = state.lists.entry(DEFAULT_LIST.to_string()).or_default();
        if s {
            if !entry.contains(&mod_id) {
                entry.push(mod_id.clone());
            }
        } else {
            entry.retain(|m| m != &mod_id);
            if entry.is_empty() {
                state.lists.remove(DEFAULT_LIST);
            }
        }
    }
    if let Some(r) = rating {
        if r == 0 {
            state.ratings.remove(&mod_id);
        } else {
            state.ratings.insert(mod_id.clone(), r.min(5));
        }
    }
    save_mod_user_state(&project_dir, &state)?;
    Ok(state)
}

/// Creates a new named build list (empty).
#[tauri::command(rename_all = "camelCase")]
fn create_mod_list(path: String, name: String) -> Result<ModUserState, String> {
    let project_dir = manifest_parent(&path)?;
    let mut state = load_mod_user_state(&project_dir);
    let trimmed = name.trim().to_string();
    if trimmed.is_empty() {
        return Err("List name cannot be empty".to_string());
    }
    if !state.lists.contains_key(&trimmed) {
        state.lists.insert(trimmed, Vec::new());
    }
    save_mod_user_state(&project_dir, &state)?;
    Ok(state)
}

/// Deletes a named build list entirely.
#[tauri::command(rename_all = "camelCase")]
fn delete_mod_list(path: String, name: String) -> Result<ModUserState, String> {
    let project_dir = manifest_parent(&path)?;
    let mut state = load_mod_user_state(&project_dir);
    state.lists.remove(&name);
    save_mod_user_state(&project_dir, &state)?;
    Ok(state)
}

/// Renames a build list.
#[tauri::command(rename_all = "camelCase")]
fn rename_mod_list(
    path: String,
    old_name: String,
    new_name: String,
) -> Result<ModUserState, String> {
    let project_dir = manifest_parent(&path)?;
    let mut state = load_mod_user_state(&project_dir);
    let trimmed = new_name.trim().to_string();
    if trimmed.is_empty() {
        return Err("List name cannot be empty".to_string());
    }
    if let Some(mods) = state.lists.remove(&old_name) {
        state.lists.insert(trimmed, mods);
    }
    save_mod_user_state(&project_dir, &state)?;
    Ok(state)
}

/// Adds a mod to a named build list (creates the list if it doesn't exist).
#[tauri::command(rename_all = "camelCase")]
fn add_to_mod_list(path: String, name: String, mod_id: String) -> Result<ModUserState, String> {
    let project_dir = manifest_parent(&path)?;
    let mut state = load_mod_user_state(&project_dir);
    let entry = state.lists.entry(name).or_default();
    if !entry.contains(&mod_id) {
        entry.push(mod_id);
    }
    save_mod_user_state(&project_dir, &state)?;
    Ok(state)
}

/// Removes a mod from a named build list. If the list becomes empty
/// it is kept (user might want to add more mods later).
#[tauri::command(rename_all = "camelCase")]
fn remove_from_mod_list(
    path: String,
    name: String,
    mod_id: String,
) -> Result<ModUserState, String> {
    let project_dir = manifest_parent(&path)?;
    let mut state = load_mod_user_state(&project_dir);
    if let Some(entry) = state.lists.get_mut(&name) {
        entry.retain(|m| m != &mod_id);
    }
    save_mod_user_state(&project_dir, &state)?;
    Ok(state)
}

#[tauri::command(rename_all = "camelCase")]
async fn install_steam_bridge(
    app: tauri::AppHandle,
    path: String,
) -> Result<serde_json::Value, String> {
    tokio::task::spawn_blocking(move || {
        let manifest_path = PathBuf::from(&path);
        auto_snapshot(&manifest_path, "install-steam-bridge").map_err(|e| e.to_string())?;
        let mut manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;

        if tuffbox_core::steam_bridge::project_has_steam_bridge(&manifest.mods) {
            return Err("Steam Bridge is already in this pack.".into());
        }

        let asset = tuffbox_core::steam_bridge::resolve_steam_bridge_asset(
            &manifest.minecraft.version,
            &manifest.loader.kind,
        )?;
        let match_note = match asset.match_kind {
            tuffbox_core::steam_bridge::SteamBridgeMatchKind::Exact => "exact match",
            tuffbox_core::steam_bridge::SteamBridgeMatchKind::SameMinor => {
                "closest same minor (exact jar not published)"
            }
        };
        let file_name = asset.file_name.clone();
        let mc = asset.mc_version.clone();
        let tag = asset.tag.clone();
        let loader_label = asset.loader_label.clone();
        let spec = tuffbox_core::steam_bridge::build_steam_bridge_mod_spec(&asset);
        manifest.mods.push(spec);
        save_manifest(&manifest_path, &manifest).map_err(|e| e.to_string())?;
        download_project_mods_tracked(&app, &manifest_path, &manifest, None, true);

        Ok(serde_json::json!({
            "modId": tuffbox_core::steam_bridge::STEAM_BRIDGE_MOD_ID,
            "fileName": file_name,
            "tag": tag,
            "mcVersion": mc,
            "loader": loader_label,
            "matchKind": match_note,
            "repo": tuffbox_core::steam_bridge::STEAM_BRIDGE_REPO,
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Batch install entry point for the content browser. It delegates to the
/// provider-specific installers so dependency resolution and snapshots remain
/// identical to the existing single-provider actions.
#[tauri::command(rename_all = "camelCase")]
async fn install_content_batch(
    app: tauri::AppHandle,
    path: String,
    provider: String,
    content_ids: Vec<String>,
    side: Option<String>,
) -> Result<Vec<String>, String> {
    let side = side.unwrap_or_else(|| "both".to_string());
    match provider.as_str() {
        "curseforge" => add_curseforge_mods_with_dependencies(app, path, content_ids, side).await,
        _ => add_modrinth_mods_with_dependencies(app, path, content_ids, side, None).await,
    }
}

#[tauri::command(rename_all = "camelCase")]
async fn add_modrinth_mod(
    app: tauri::AppHandle,
    path: String,
    mod_id: String,
    side: String,
) -> Result<(), String> {
    let path_for_stats = path.clone();
    tokio::task::spawn_blocking(move || {
        let mut snapshot = auto_snapshot_before_mod_op(&PathBuf::from(&path), "add-mod")
            .map_err(|e| e.to_string())?;
        let mut manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
        add_mod_from_modrinth(&mut manifest, &mod_id, Some(side)).map_err(|e| e.to_string())?;
        save_manifest(&PathBuf::from(&path), &manifest).map_err(|e| e.to_string())?;
        download_project_mods_tracked(&app, &PathBuf::from(&path), &manifest, None, true);
        let manifest_path = PathBuf::from(&path);
        if let Some(module) = manifest.mods.iter().find(|m| {
            m.id == mod_id || m.source.project_id.as_deref() == Some(mod_id.as_str())
        }) {
            let lines = vec![mod_history_line("Install", module)];
            finalize_mod_history(
                &manifest_path,
                &mut snapshot,
                "add-mod",
                &lines,
                &[module],
                &[],
            );
        }
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| e.to_string())??;
    helpers::invalidate_recent_home_cache(&path_for_stats);
    swarm_api::spawn_pack_cooccurrence(path_for_stats, "mod_install");
    Ok(())
}

#[tauri::command(rename_all = "camelCase")]
async fn add_modrinth_mod_with_dependencies(
    app: tauri::AppHandle,
    path: String,
    mod_id: String,
    side: String,
    dependency_targets: Option<Vec<String>>,
) -> Result<Vec<String>, String> {
    let path_for_stats = path.clone();
    let installed = tokio::task::spawn_blocking(move || {
        let manifest_path = PathBuf::from(&path);
        let mut snapshot =
            auto_snapshot_before_mod_op(&manifest_path, "add-mod-with-dependencies")
                .map_err(|e| e.to_string())?;
        let mut manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
        let installed = install_modrinth_with_dependencies(
            &mut manifest,
            &[mod_id.clone()],
            &side,
            dependency_targets.as_deref(),
        )?;
        save_manifest(&manifest_path, &manifest).map_err(|e| e.to_string())?;
        download_project_mods_tracked(&app, &manifest_path, &manifest, None, true);
        let related: Vec<&ModSpec> = installed
            .iter()
            .filter_map(|id| {
                manifest.mods.iter().find(|m| {
                    m.id == *id || m.source.project_id.as_deref() == Some(id.as_str())
                })
            })
            .collect();
        let lines = mod_install_history_lines(&related, &[mod_id]);
        finalize_mod_history(
            &manifest_path,
            &mut snapshot,
            "add-mod-with-dependencies",
            &lines,
            &related,
            &[],
        );
        Ok::<Vec<String>, String>(installed)
    })
    .await
    .map_err(|e| e.to_string())??;
    swarm_api::spawn_pack_cooccurrence(path_for_stats, "mod_install");
    Ok(installed)
}

#[tauri::command(rename_all = "camelCase")]
async fn add_modrinth_mods_with_dependencies(
    app: tauri::AppHandle,
    path: String,
    mod_ids: Vec<String>,
    side: String,
    dependency_targets: Option<Vec<String>>,
) -> Result<Vec<String>, String> {
    let path_for_stats = path.clone();
    let installed = tokio::task::spawn_blocking(move || {
        let manifest_path = PathBuf::from(&path);
        let mut snapshot =
            auto_snapshot_before_mod_op(&manifest_path, "bulk-add-mods-with-dependencies")
                .map_err(|e| e.to_string())?;
        let mut manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
        let installed = install_modrinth_with_dependencies(
            &mut manifest,
            &mod_ids,
            &side,
            dependency_targets.as_deref(),
        )?;
        save_manifest(&manifest_path, &manifest).map_err(|e| e.to_string())?;
        download_project_mods_tracked(&app, &manifest_path, &manifest, None, true);
        let related: Vec<&ModSpec> = installed
            .iter()
            .filter_map(|id| {
                manifest.mods.iter().find(|m| {
                    m.id == *id || m.source.project_id.as_deref() == Some(id.as_str())
                })
            })
            .collect();
        let lines = mod_install_history_lines(&related, &mod_ids);
        finalize_mod_history(
            &manifest_path,
            &mut snapshot,
            "bulk-add-mods-with-dependencies",
            &lines,
            &related,
            &[],
        );
        Ok::<Vec<String>, String>(installed)
    })
    .await
    .map_err(|e| e.to_string())??;
    swarm_api::spawn_pack_cooccurrence(path_for_stats, "mod_install");
    Ok(installed)
}

#[tauri::command(rename_all = "camelCase")]
async fn remove_project_mod(path: String, mod_id: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let manifest_path = PathBuf::from(&path);
        let mut snapshot = auto_snapshot_before_mod_op(&manifest_path, "remove-mod")
            .map_err(|e| e.to_string())?;
        let mut manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
        let removed_idx = manifest
            .mods
            .iter()
            .position(|m| {
                m.id == mod_id
                    || m.source.project_id.as_deref() == Some(mod_id.as_str())
                    || m.file_name.as_deref() == Some(mod_id.as_str())
            })
            .ok_or_else(|| format!("mod {mod_id} not found in project"))?;
        let removed_mod = manifest.mods.remove(removed_idx);
        save_manifest(&manifest_path, &manifest).map_err(|e| e.to_string())?;

        // Prefer live disk hash so we can also clear leftover renamed jars
        // and the persistent Modrinth hash index.
        let mut sha1 = removed_mod
            .hashes
            .as_ref()
            .and_then(|h| h.sha1.clone())
            .filter(|s| !s.is_empty());
        let mut removed_paths: Vec<String> = Vec::new();
        if let Some(path_on_disk) = existing_mod_file_path(&manifest_path, &removed_mod) {
            if let Ok(hash) = tuffbox_core::sha1_file(&path_on_disk) {
                sha1 = Some(hash);
            }
            if let Some(project_dir) = manifest_path.parent() {
                if let Ok(rel) = path_on_disk.strip_prefix(project_dir) {
                    removed_paths.push(rel.to_string_lossy().replace('\\', "/"));
                }
            }
        } else if let Some(name) = removed_mod.file_name.as_deref() {
            removed_paths.push(format!("mods/{name}"));
        }

        remove_mod_file_from_disk(&manifest_path, &removed_mod);
        if let Some(ref hash) = sha1 {
            // Drop any jar with the same bytes (renames / .disabled leftovers),
            // but skip files still tracked by other manifest entries so we
            // don't accidentally delete a second copy of the same mod.
            if let Some(instance_dir) = tuffbox_core::instance_dir_for_manifest(&manifest_path) {
                let remaining_names: std::collections::HashSet<&str> = manifest
                    .mods
                    .iter()
                    .filter_map(|m| m.file_name.as_deref())
                    .collect();
                let content_dir =
                    tuffbox_core::content_dir_for(&instance_dir, removed_mod.content_type);
                if let Ok(entries) = std::fs::read_dir(&content_dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if !path.is_file() {
                            continue;
                        }
                        // Skip files still referenced by another manifest entry.
                        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                            if remaining_names.contains(name) {
                                continue;
                            }
                        }
                        if let Ok(actual) = tuffbox_core::sha1_file(&path) {
                            if actual.eq_ignore_ascii_case(hash) {
                                tuffbox_core::fs_util::clear_readonly(&path);
                                let _ = std::fs::remove_file(path);
                                release_store_object_detached(actual);
                            }
                        }
                    }
                }
            }
        }

        if let Some(instance_dir) = tuffbox_core::instance_dir_for_manifest(&manifest_path) {
            let mut index = tuffbox_core::ModHashIndex::load(&instance_dir);
            if let Some(hash) = sha1.as_deref() {
                index.remove_sha1(hash);
            }
            index.remove_id(&removed_mod.id);
            if let Some(pid) = removed_mod.source.project_id.as_deref() {
                index.remove_project(pid);
            }
            let _ = index.save(&instance_dir);
        }
        let lines = vec![mod_history_line("Remove", &removed_mod)];
        finalize_mod_history(
            &manifest_path,
            &mut snapshot,
            "remove-mod",
            &lines,
            &[],
            &removed_paths,
        );
        helpers::invalidate_recent_home_cache(&path);
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Soft-disable a tracked mod by renaming its jar to `*.disabled` (Prism/Minecraft
/// convention). Keeps the manifest entry so it can be re-enabled later.
#[tauri::command(rename_all = "camelCase")]
async fn disable_project_mod(path: String, mod_id: String) -> Result<serde_json::Value, String> {
    tokio::task::spawn_blocking(move || {
        let manifest_path = PathBuf::from(&path);
        let mut snapshot = auto_snapshot_before_mod_op(&manifest_path, "disable-mod")
            .map_err(|e| e.to_string())?;
        let mut manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
        let idx = manifest
            .mods
            .iter()
            .position(|m| {
                m.id == mod_id
                    || m.source.project_id.as_deref() == Some(mod_id.as_str())
                    || m.file_name.as_deref() == Some(mod_id.as_str())
            })
            .ok_or_else(|| format!("mod {mod_id} not found in project"))?;
        let module = &mut manifest.mods[idx];
        if module.status.iter().any(|s| s.eq_ignore_ascii_case("disabled")) {
            return Ok(serde_json::json!({
                "id": module.id,
                "disabled": true,
                "alreadyDisabled": true,
                "fileName": module.file_name,
            }));
        }
        let Some(file_name) = module.file_name.clone() else {
            return Err(format!("{} has no file name to disable", module.name));
        };
        let Some(instance_dir) = tuffbox_core::instance_dir_for_manifest(&manifest_path) else {
            return Err("could not resolve instance directory".to_string());
        };
        let content_dir = tuffbox_core::content_dir_for(&instance_dir, module.content_type);
        let active = content_dir.join(&file_name);
        let disabled = content_dir.join(format!("{file_name}.disabled"));
        if disabled.is_file() && !active.is_file() {
            // Already renamed on disk — just mark the status.
        } else if active.is_file() {
            if disabled.exists() {
                tuffbox_core::fs_util::clear_readonly(&disabled);
                let _ = std::fs::remove_file(&disabled);
            }
            std::fs::rename(&active, &disabled).map_err(|e| {
                format!(
                    "failed to rename {} → {}.disabled: {e}",
                    active.display(),
                    file_name
                )
            })?;
        } else {
            return Err(format!(
                "{} not found on disk (looked for {} and {}.disabled)",
                module.name, file_name, file_name
            ));
        }
        if !module
            .status
            .iter()
            .any(|s| s.eq_ignore_ascii_case("disabled"))
        {
            module.status.push("disabled".to_string());
        }
        let id = module.id.clone();
        let name = module.name.clone();
        save_manifest(&manifest_path, &manifest).map_err(|e| e.to_string())?;
        let module = &manifest.mods[idx];
        let lines = vec![mod_history_line("Disable", module)];
        finalize_mod_history(
            &manifest_path,
            &mut snapshot,
            "disable-mod",
            &lines,
            &[module],
            &[],
        );
        // Drop the pre-rename jar from baseline so it isn't reported as removed.
        if let Some(project_dir) = manifest_path.parent() {
            if let Ok(rel) = active.strip_prefix(project_dir) {
                let rel = rel.to_string_lossy().replace('\\', "/");
                let _ = pack_events::sync_baseline_paths(project_dir, &[rel]);
            }
        }
        Ok(serde_json::json!({
            "id": id,
            "name": name,
            "disabled": true,
            "alreadyDisabled": false,
            "fileName": format!("{file_name}.disabled"),
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Re-enable a previously disabled mod by renaming `*.disabled` back.
#[tauri::command(rename_all = "camelCase")]
async fn enable_project_mod(path: String, mod_id: String) -> Result<serde_json::Value, String> {
    tokio::task::spawn_blocking(move || {
        let manifest_path = PathBuf::from(&path);
        let mut snapshot = auto_snapshot_before_mod_op(&manifest_path, "enable-mod")
            .map_err(|e| e.to_string())?;
        let mut manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
        let idx = manifest
            .mods
            .iter()
            .position(|m| {
                m.id == mod_id
                    || m.source.project_id.as_deref() == Some(mod_id.as_str())
                    || m.file_name.as_deref() == Some(mod_id.as_str())
            })
            .ok_or_else(|| format!("mod {mod_id} not found in project"))?;
        let module = &mut manifest.mods[idx];
        let Some(file_name) = module.file_name.clone() else {
            return Err(format!("{} has no file name to enable", module.name));
        };
        let Some(instance_dir) = tuffbox_core::instance_dir_for_manifest(&manifest_path) else {
            return Err("could not resolve instance directory".to_string());
        };
        let content_dir = tuffbox_core::content_dir_for(&instance_dir, module.content_type);
        let active = content_dir.join(&file_name);
        let disabled = content_dir.join(format!("{file_name}.disabled"));
        if active.is_file() {
            // Already active.
        } else if disabled.is_file() {
            std::fs::rename(&disabled, &active).map_err(|e| {
                format!(
                    "failed to rename {}.disabled → {}: {e}",
                    file_name,
                    active.display()
                )
            })?;
        } else {
            return Err(format!(
                "{} is not present as either {} or {}.disabled",
                module.name, file_name, file_name
            ));
        }
        module
            .status
            .retain(|s| !s.eq_ignore_ascii_case("disabled"));
        let id = module.id.clone();
        let name = module.name.clone();
        save_manifest(&manifest_path, &manifest).map_err(|e| e.to_string())?;
        let module = &manifest.mods[idx];
        let lines = vec![mod_history_line("Enable", module)];
        finalize_mod_history(
            &manifest_path,
            &mut snapshot,
            "enable-mod",
            &lines,
            &[module],
            &[],
        );
        if let Some(project_dir) = manifest_path.parent() {
            if let Ok(rel) = disabled.strip_prefix(project_dir) {
                let rel = rel.to_string_lossy().replace('\\', "/");
                let _ = pack_events::sync_baseline_paths(project_dir, &[rel]);
            }
        }
        Ok(serde_json::json!({
            "id": id,
            "name": name,
            "disabled": false,
            "fileName": file_name,
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

fn set_mod_jar_disabled(
    manifest_path: &Path,
    module: &mut ModSpec,
    want_disabled: bool,
) -> Result<bool, String> {
    let already = module
        .status
        .iter()
        .any(|s| s.eq_ignore_ascii_case("disabled"));
    if already == want_disabled {
        return Ok(false);
    }
    let Some(file_name) = module.file_name.clone() else {
        return Err(format!("{} has no file name", module.name));
    };
    let Some(instance_dir) = tuffbox_core::instance_dir_for_manifest(manifest_path) else {
        return Err("could not resolve instance directory".to_string());
    };
    let content_dir = tuffbox_core::content_dir_for(&instance_dir, module.content_type);
    let active = content_dir.join(&file_name);
    let disabled = content_dir.join(format!("{file_name}.disabled"));
    if want_disabled {
        if disabled.is_file() && !active.is_file() {
            // already on disk
        } else if active.is_file() {
            if disabled.exists() {
                tuffbox_core::fs_util::clear_readonly(&disabled);
                let _ = std::fs::remove_file(&disabled);
            }
            std::fs::rename(&active, &disabled).map_err(|e| e.to_string())?;
        } else {
            return Err(format!("{} not found on disk", module.name));
        }
        if !module
            .status
            .iter()
            .any(|s| s.eq_ignore_ascii_case("disabled"))
        {
            module.status.push("disabled".into());
        }
    } else {
        if active.is_file() {
            // already active
        } else if disabled.is_file() {
            std::fs::rename(&disabled, &active).map_err(|e| e.to_string())?;
        } else {
            return Err(format!("{} not found on disk", module.name));
        }
        module
            .status
            .retain(|s| !s.eq_ignore_ascii_case("disabled"));
    }
    Ok(true)
}

fn apply_group_test_layout(
    manifest_path: &Path,
    session: &tuffbox_core::mod_group_test::GroupTestSession,
) -> Result<Vec<String>, String> {
    let mut manifest = ProjectManifest::load_from_path(manifest_path).map_err(|e| e.to_string())?;
    let want_disabled: std::collections::HashSet<String> =
        session.desired_disabled().into_iter().collect();
    let pool: std::collections::HashSet<&str> = session.pool.iter().map(|s| s.as_str()).collect();
    let mut changed = Vec::new();
    let mut enabled_ids: Vec<String> = Vec::new();
    let mut disabled_ids: Vec<String> = Vec::new();
    let mut paths: Vec<String> = Vec::new();
    for module in &mut manifest.mods {
        if !pool.contains(module.id.as_str()) {
            continue;
        }
        let disable = want_disabled.contains(&module.id);
        if set_mod_jar_disabled(manifest_path, module, disable)? {
            if disable {
                disabled_ids.push(module.id.clone());
            } else {
                enabled_ids.push(module.id.clone());
            }
            changed.push(format!(
                "{} {}",
                if disable { "Disable" } else { "Enable" },
                module.name
            ));
            if let Some(name) = module.file_name.as_deref() {
                paths.push(format!("mods/{name}"));
                paths.push(format!("mods/{name}.disabled"));
            }
        }
    }
    save_manifest(manifest_path, &manifest).map_err(|e| e.to_string())?;
    if let Some(project_dir) = manifest_path.parent() {
        let _ = pack_events::record_group_test_layout_event(
            project_dir,
            session.snapshot_id.as_deref(),
            &enabled_ids,
            &disabled_ids,
            &paths,
        );
    }
    Ok(changed)
}

#[tauri::command(rename_all = "camelCase")]
async fn start_mod_group_test(
    path: String,
    suspected: Option<Vec<String>>,
) -> Result<tuffbox_core::mod_group_test::GroupTestSession, String> {
    // Blocking-pool conversion: sync commands run on the main thread; this
    // one snapshots/rolls back files and must not freeze IPC delivery.
    tokio::task::spawn_blocking(move || start_mod_group_test_impl(path, suspected))
        .await
        .map_err(|e| e.to_string())?
}

fn start_mod_group_test_impl(
    path: String,
    suspected: Option<Vec<String>>,
) -> Result<tuffbox_core::mod_group_test::GroupTestSession, String> {
    let manifest_path = PathBuf::from(&path);
    let project_dir = manifest_path
        .parent()
        .ok_or_else(|| "manifest path has no parent".to_string())?;
    let manifest = ProjectManifest::load_from_path(&manifest_path).map_err(|e| e.to_string())?;
    let fp = swarm_api::peek_last_crash_fix_marker(project_dir)
        .ok()
        .flatten()
        .map(|m| m.fingerprint_key)
        .unwrap_or_else(|| "unknown".into());
    let recent = pack_events::recently_changed_mod_ids(project_dir, &fp);
    let suspected = suspected.unwrap_or_default();
    let pool = tuffbox_core::mod_group_test::candidate_pool(&manifest.mods, &recent, &suspected);
    if pool.len() < 2 {
        return Err(format!(
            "Need at least 2 candidate mods to group-test (got {}).",
            pool.len()
        ));
    }
    let snapshot = auto_snapshot_before_mod_op(&manifest_path, "group-test")
        .map_err(|e| e.to_string())?;
    let mut session = tuffbox_core::mod_group_test::GroupTestSession::start(pool);
    session.snapshot_id = Some(snapshot.id.clone());
    apply_group_test_layout(&manifest_path, &session)?;
    swarm_api::save_group_test_session(project_dir, &session)?;
    Ok(session)
}

#[tauri::command(rename_all = "camelCase")]
async fn get_mod_group_test(path: String) -> Result<Option<tuffbox_core::mod_group_test::GroupTestSession>, String> {
    // Blocking-pool conversion: sync commands run on the main thread; this
    // one snapshots/rolls back files and must not freeze IPC delivery.
    tokio::task::spawn_blocking(move || get_mod_group_test_impl(path))
        .await
        .map_err(|e| e.to_string())?
}

fn get_mod_group_test_impl(path: String) -> Result<Option<tuffbox_core::mod_group_test::GroupTestSession>, String> {
    let project_dir = PathBuf::from(&path)
        .parent()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| "manifest path has no parent".to_string())?;
    Ok(swarm_api::load_group_test_session(&project_dir))
}

#[tauri::command(rename_all = "camelCase")]
async fn report_mod_group_test_outcome(
    path: String,
    outcome: String,
) -> Result<tuffbox_core::mod_group_test::GroupTestSession, String> {
    // Blocking-pool conversion: sync commands run on the main thread; this
    // one snapshots/rolls back files and must not freeze IPC delivery.
    tokio::task::spawn_blocking(move || report_mod_group_test_outcome_impl(path, outcome))
        .await
        .map_err(|e| e.to_string())?
}

fn report_mod_group_test_outcome_impl(
    path: String,
    outcome: String,
) -> Result<tuffbox_core::mod_group_test::GroupTestSession, String> {
    let manifest_path = PathBuf::from(&path);
    let project_dir = manifest_path
        .parent()
        .ok_or_else(|| "manifest path has no parent".to_string())?;
    let mut session = swarm_api::load_group_test_session(project_dir)
        .ok_or_else(|| "no group-test session".to_string())?;
    let parsed = match outcome.to_ascii_lowercase().as_str() {
        "healthy" | "launched" => tuffbox_core::mod_group_test::TestOutcome::Healthy,
        "crash" | "crashed" => tuffbox_core::mod_group_test::TestOutcome::Crash,
        other => return Err(format!("unknown outcome: {other}")),
    };
    session.apply_outcome(parsed);
    if !matches!(
        session.phase,
        tuffbox_core::mod_group_test::GroupTestPhase::Done
            | tuffbox_core::mod_group_test::GroupTestPhase::Failed { .. }
    ) {
        apply_group_test_layout(&manifest_path, &session)?;
    } else if session.verified {
        apply_group_test_layout(&manifest_path, &session)?;
        if let Some(plan) = session.share_plan() {
            let _ = swarm_api::record_user_fix_attempt(
                &manifest_path,
                "group_test",
                &plan.human_explanation,
                plan.actions,
                None,
            );
        }
    }
    swarm_api::save_group_test_session(project_dir, &session)?;
    Ok(session)
}

#[tauri::command(rename_all = "camelCase")]
async fn cancel_mod_group_test(path: String) -> Result<(), String> {
    // Blocking-pool conversion: sync commands run on the main thread; this
    // one snapshots/rolls back files and must not freeze IPC delivery.
    tokio::task::spawn_blocking(move || cancel_mod_group_test_impl(path))
        .await
        .map_err(|e| e.to_string())?
}

fn cancel_mod_group_test_impl(path: String) -> Result<(), String> {
    let manifest_path = PathBuf::from(&path);
    let project_dir = manifest_path
        .parent()
        .ok_or_else(|| "manifest path has no parent".to_string())?;
    if let Some(session) = swarm_api::load_group_test_session(project_dir) {
        if let Some(id) = session.snapshot_id {
            let store = SnapshotStore::new(project_dir);
            let _ = store.rollback(id);
        }
    }
    swarm_api::clear_group_test_session(project_dir);
    Ok(())
}

#[tauri::command(rename_all = "camelCase")]
async fn update_project_mod(
    app: tauri::AppHandle,
    path: String,
    mod_id: String,
    version_id: Option<String>,
) -> Result<serde_json::Value, String> {
    tokio::task::spawn_blocking(move || {
        let _guard = MODS_IO_LOCK
            .lock()
            .map_err(|_| "mods I/O lock poisoned".to_string())?;
        emit_mod_update_progress(
            &app,
            "preparing",
            "Creating a safety snapshot…",
            0,
            1,
            5,
            Some(&mod_id),
        );
        let manifest_path = PathBuf::from(&path);
        let mut snapshot = auto_snapshot_before_mod_op(&manifest_path, "update-mod")
            .map_err(|e| e.to_string())?;
        let mut manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
        let old_mod = manifest
            .mods
            .iter()
            .find(|module| {
                module.id == mod_id || module.source.project_id.as_deref() == Some(mod_id.as_str())
            })
            .cloned()
            .ok_or_else(|| format!("mod {mod_id} not found in project"))?;
        let mut old_paths: Vec<String> = Vec::new();
        if let Some(abs) = existing_mod_file_path(&manifest_path, &old_mod) {
            if let Some(project_dir) = manifest_path.parent() {
                if let Ok(rel) = abs.strip_prefix(project_dir) {
                    old_paths.push(rel.to_string_lossy().replace('\\', "/"));
                }
            }
        } else if let Some(name) = old_mod.file_name.as_deref() {
            old_paths.push(format!("mods/{name}"));
        }
        emit_mod_update_progress(
            &app,
            "resolving",
            &format!("Resolving the latest version of {}…", old_mod.name),
            0,
            1,
            20,
            Some(&old_mod.id),
        );
        update_mod_from_modrinth(
            &manifest_path,
            &mut manifest,
            &mod_id,
            version_id.as_deref(),
        )
        .map_err(|e| e.to_string())?;
        emit_mod_update_progress(
            &app,
            "downloading",
            &format!("Downloading {}…", old_mod.name),
            0,
            1,
            40,
            Some(&old_mod.id),
        );
        let report = commit_single_mod_update(&app, &manifest_path, &mut manifest, &old_mod, true)?;
        let new_mod = manifest
            .mods
            .iter()
            .find(|m| m.id == old_mod.id)
            .cloned()
            .unwrap_or_else(|| old_mod.clone());
        let lines = vec![format!(
            "Update {} {} → {}",
            new_mod.name, old_mod.version, new_mod.version
        )];
        finalize_mod_history(
            &manifest_path,
            &mut snapshot,
            "update-mod",
            &lines,
            &[&new_mod],
            &old_paths,
        );
        emit_mod_update_progress(
            &app,
            "done",
            &format!("{} was updated successfully.", old_mod.name),
            1,
            1,
            100,
            Some(&old_mod.id),
        );
        Ok(serde_json::json!({
            "modId": mod_id,
            "download": report,
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Returns available versions for a Modrinth project.
/// Loads *all* project versions (like Modrinth App's content updater), marks
/// compatibility against the given Minecraft version + loader, and sorts
/// compatible releases first. The UI can hide incompatible rows by default.
#[tauri::command(rename_all = "camelCase")]
async fn get_mod_versions(
    mod_id: String,
    minecraft_version: String,
    loader: Option<String>,
) -> Result<Vec<serde_json::Value>, String> {
    tokio::task::spawn_blocking(move || {
        let provider = tuffbox_core::ModrinthProvider::new();
        let loader_slug = loader
            .as_deref()
            .map(|l| l.trim().to_lowercase())
            .filter(|l| !l.is_empty());
        // Fetch unfiltered — filter/mark compatibility client-side style
        // (Modrinth App ContentUpdaterModal pattern).
        let query = ProviderSearchQuery {
            query: None,
            minecraft_version: None,
            loader: None,
            ..Default::default()
        };
        let mut versions = provider
            .get_versions(&mod_id, &query)
            .map_err(|e| e.to_string())?;

        // Newest first (Modrinth usually returns that already; don't rely on it).
        versions.sort_by(|a, b| {
            b.date_published
                .as_deref()
                .unwrap_or("")
                .cmp(a.date_published.as_deref().unwrap_or(""))
        });

        let mc_filter = minecraft_version.trim();
        let mut rows: Vec<serde_json::Value> = versions
            .into_iter()
            .map(|v| {
                // Empty MC filter = "any version" (callers that omit instance
                // context must not mark every row incompatible).
                let mc_ok = mc_filter.is_empty()
                    || v.game_versions.iter().any(|gv| gv == mc_filter);
                let loader_ok = match &loader_slug {
                    Some(loader) => v
                        .loaders
                        .iter()
                        .any(|l| l == loader || (*loader == "quilt" && l == "fabric")),
                    None => true,
                };
                let compatible = mc_ok && loader_ok;
                // Modrinth changelogs are Markdown — render once server-side
                // so the UI receives uniform `changelogHtml` for both providers.
                let changelog_html = v
                    .changelog
                    .as_deref()
                    .map(tuffbox_core::markdown_to_html)
                    .filter(|s| !s.trim().is_empty());
                serde_json::json!({
                    "id": v.id,
                    "versionNumber": v.version_number,
                    "gameVersions": v.game_versions,
                    "loaders": v.loaders,
                    "name": v.name,
                    "changelogHtml": changelog_html,
                    "datePublished": v.date_published,
                    "versionType": v.version_type.unwrap_or_else(|| "release".to_string()),
                    "compatible": compatible,
                    "compatibleMinecraft": mc_ok,
                    "compatibleLoader": loader_ok,
                })
            })
            .collect();

        // Compatible first, then by channel preference (release > beta > alpha).
        rows.sort_by(|a, b| {
            let a_ok = a
                .get("compatible")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let b_ok = b
                .get("compatible")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            match (a_ok, b_ok) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => {
                    let rank = |row: &serde_json::Value| match row
                        .get("versionType")
                        .and_then(|v| v.as_str())
                        .unwrap_or("release")
                    {
                        "release" => 0,
                        "beta" => 1,
                        "alpha" => 2,
                        _ => 3,
                    };
                    rank(a).cmp(&rank(b)).then_with(|| {
                        b.get("datePublished")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .cmp(
                                a.get("datePublished")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or(""),
                            )
                    })
                }
            }
        });

        Ok(rows)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Changes a mod entry to a specific version (identified by Modrinth
/// version id), downloading the new file and updating metadata in the
/// manifest.
#[tauri::command(rename_all = "camelCase")]
async fn change_mod_version(
    app: tauri::AppHandle,
    path: String,
    mod_id: String,
    new_version_id: String,
) -> Result<serde_json::Value, String> {
    tokio::task::spawn_blocking(move || {
        let manifest_path = PathBuf::from(&path);
        auto_snapshot(&manifest_path, "change-mod-version").map_err(|e| e.to_string())?;
        let mut manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;

        let provider = tuffbox_core::ModrinthProvider::new();
        let version_info = provider
            .get_version(&new_version_id)
            .map_err(|e| e.to_string())?;
        let project = provider
            .get_project(&version_info.project_id)
            .map_err(|e| e.to_string())?;
        let file = ProviderFileInfo::select_file_for_loader(
            &version_info,
            &tuffbox_core::graph::loader_kind_slug(&manifest.loader.kind),
        )
        .cloned()
        .ok_or_else(|| format!("no primary file for version {}", version_info.id))?;
        let idx = manifest
            .mods
            .iter()
            .position(|m| m.id == mod_id)
            .ok_or_else(|| format!("mod {mod_id} not found in project"))?;

        let old_mod = manifest.mods[idx].clone();
        let side = infer_project_side(Some(&project));
        let previous_deps = old_mod.dependencies.clone();
        let dependencies = provider
            .resolve_dependencies(&version_info.id)
            .unwrap_or(previous_deps);
        let mut new_spec = build_mod_spec(&project, &version_info, file, dependencies, side);
        // Keep the stable UI / dependency id across version switches.
        new_spec.id = old_mod.id.clone();
        manifest.mods[idx] = new_spec;

        let report = commit_single_mod_update(&app, &manifest_path, &mut manifest, &old_mod, true)?;

        Ok(serde_json::json!({
            "version": version_info.version_number,
            "id": version_info.id,
            "download": report,
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Inner helper: soft-disable a tracked mod (mirrors the `disable_project_mod`
/// command without the async wrapper) so the fix command can reuse it.
fn disable_project_mod_inner(
    manifest_path: &Path,
    mod_id: &str,
) -> Result<serde_json::Value, String> {
    auto_snapshot(manifest_path, "disable-mod").map_err(|e| e.to_string())?;
    let mut manifest = ProjectManifest::load_from_path(manifest_path).map_err(|e| e.to_string())?;
    let idx = manifest
        .mods
        .iter()
        .position(|m| {
            m.id == mod_id
                || m.source.project_id.as_deref() == Some(mod_id)
                || m.file_name.as_deref() == Some(mod_id)
        })
        .ok_or_else(|| format!("mod {mod_id} not found in project"))?;
    let module = &mut manifest.mods[idx];
    if module
        .status
        .iter()
        .any(|s| s.eq_ignore_ascii_case("disabled"))
    {
        return Ok(serde_json::json!({
            "id": module.id,
            "disabled": true,
            "alreadyDisabled": true,
            "fileName": module.file_name,
        }));
    }
    let Some(file_name) = module.file_name.clone() else {
        return Err(format!("{} has no file name to disable", module.name));
    };
    let Some(instance_dir) = tuffbox_core::instance_dir_for_manifest(manifest_path) else {
        return Err("could not resolve instance directory".to_string());
    };
    let content_dir = tuffbox_core::content_dir_for(&instance_dir, module.content_type);
    let active = content_dir.join(&file_name);
    let disabled = content_dir.join(format!("{file_name}.disabled"));
    if disabled.is_file() && !active.is_file() {
        // Already renamed on disk.
    } else if active.is_file() {
        if disabled.exists() {
            tuffbox_core::fs_util::clear_readonly(&disabled);
            let _ = std::fs::remove_file(&disabled);
        }
        std::fs::rename(&active, &disabled).map_err(|e| {
            format!("failed to rename {} → {}.disabled: {e}", active.display(), file_name)
        })?;
    } else {
        return Err(format!(
            "{} not found on disk (looked for {} and {}.disabled)",
            module.name, file_name, file_name
        ));
    }
    if !module
        .status
        .iter()
        .any(|s| s.eq_ignore_ascii_case("disabled"))
    {
        module.status.push("disabled".to_string());
    }
    let id = module.id.clone();
    save_manifest(manifest_path, &manifest).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "id": id,
        "disabled": true,
        "alreadyDisabled": false,
        "fileName": format!("{file_name}.disabled"),
    }))
}

/// Inner helper: remove a tracked mod (mirrors `remove_project_mod`).
fn remove_project_mod_inner(manifest_path: &Path, mod_id: &str) -> Result<(), String> {
    auto_snapshot(manifest_path, "remove-mod").map_err(|e| e.to_string())?;
    let mut manifest = ProjectManifest::load_from_path(manifest_path).map_err(|e| e.to_string())?;
    let removed_idx = manifest
        .mods
        .iter()
        .position(|m| {
            m.id == mod_id
                || m.source.project_id.as_deref() == Some(mod_id)
                || m.file_name.as_deref() == Some(mod_id)
        })
        .ok_or_else(|| format!("mod {mod_id} not found in project"))?;
    let removed_mod = manifest.mods.remove(removed_idx);
    save_manifest(manifest_path, &manifest).map_err(|e| e.to_string())?;
    remove_mod_file_from_disk(manifest_path, &removed_mod);
    Ok(())
}

/// Updates a single mod to the newest version compatible with the project's
/// current Minecraft version + loader. Returns a short summary of what was
/// applied. Used by the crash-diagnosis Fix buttons ("update mod").
fn apply_mod_update_to_latest(
    app: &tauri::AppHandle,
    manifest_path: &Path,
    manifest: &mut ProjectManifest,
    mod_id: &str,
) -> Result<String, String> {
    let idx = manifest
        .mods
        .iter()
        .position(|m| {
            m.id == mod_id
                || m.source.project_id.as_deref() == Some(mod_id)
                || m.file_name.as_deref() == Some(mod_id)
        })
        .ok_or_else(|| format!("mod {mod_id} not found in project"))?;
    let old_mod = manifest.mods[idx].clone();
    let project_id = old_mod
        .source
        .project_id
        .clone()
        .ok_or_else(|| format!("{} is not a Modrinth mod and cannot be auto-updated", old_mod.name))?;
    let (loader_slug, loaders) = update_loaders_for(manifest);
    let provider = tuffbox_core::ModrinthProvider::new();
    let query = ProviderSearchQuery {
        query: None,
        minecraft_version: Some(manifest.minecraft.version.clone()),
        loader: Some(loader_slug.clone()),
        ..Default::default()
    };
    let mut versions = provider
        .get_versions(&project_id, &query)
        .map_err(|e| e.to_string())?;
    versions.sort_by(|a, b| {
        b.date_published
            .as_deref()
            .unwrap_or("")
            .cmp(a.date_published.as_deref().unwrap_or(""))
    });
    let version_info = versions
        .into_iter()
        .find(|v| v.loaders.iter().any(|l| loaders.iter().any(|s| s == l)))
        .ok_or_else(|| format!("no compatible update for {} on this loader", old_mod.name))?;
    if old_mod.version == version_info.version_number {
        return Ok(format!("{} is already on the latest version", old_mod.name));
    }
    let project = provider
        .get_project(&version_info.project_id)
        .map_err(|e| e.to_string())?;
    let file = ProviderFileInfo::select_file_for_loader(&version_info, &loader_slug)
        .cloned()
        .ok_or_else(|| format!("no primary file for version {}", version_info.id))?;
    let dependencies = provider
        .resolve_dependencies(&version_info.id)
        .unwrap_or_else(|_| old_mod.dependencies.clone());
    let mut new_spec = build_mod_spec(
        &project,
        &version_info,
        file,
        dependencies,
        infer_project_side(Some(&project)),
    );
    new_spec.id = old_mod.id.clone();
    manifest.mods[idx] = new_spec;
    let report = commit_single_mod_update(app, manifest_path, manifest, &old_mod, true)?;
    let _ = report;
    Ok(format!(
        "Updated {} → {}",
        old_mod.name, version_info.version_number
    ))
}

/// Reinstalls a mod by removing its tracked entry + jar and re-fetching the
/// current compatible version from Modrinth. Used by the "reinstall mod" fix.
fn apply_mod_reinstall(
    app: &tauri::AppHandle,
    manifest_path: &Path,
    manifest: &mut ProjectManifest,
    mod_id: &str,
) -> Result<String, String> {
    let idx = manifest
        .mods
        .iter()
        .position(|m| {
            m.id == mod_id
                || m.source.project_id.as_deref() == Some(mod_id)
                || m.file_name.as_deref() == Some(mod_id)
        })
        .ok_or_else(|| format!("mod {mod_id} not found in project"))?;
    let old_mod = manifest.mods[idx].clone();
    let project_id = old_mod
        .source
        .project_id
        .clone()
        .ok_or_else(|| format!("{} is not a Modrinth mod and cannot be reinstalled", old_mod.name))?;
    let (loader_slug, loaders) = update_loaders_for(manifest);
    let provider = tuffbox_core::ModrinthProvider::new();
    let query = ProviderSearchQuery {
        query: None,
        minecraft_version: Some(manifest.minecraft.version.clone()),
        loader: Some(loader_slug.clone()),
        ..Default::default()
    };
    let mut versions = provider
        .get_versions(&project_id, &query)
        .map_err(|e| e.to_string())?;
    versions.sort_by(|a, b| {
        b.date_published
            .as_deref()
            .unwrap_or("")
            .cmp(a.date_published.as_deref().unwrap_or(""))
    });
    let version_info = versions
        .into_iter()
        .find(|v| v.loaders.iter().any(|l| loaders.iter().any(|s| s == l)))
        .ok_or_else(|| format!("no compatible version for {} on this loader", old_mod.name))?;
    let project = provider
        .get_project(&version_info.project_id)
        .map_err(|e| e.to_string())?;
    let file = ProviderFileInfo::select_file_for_loader(&version_info, &loader_slug)
        .cloned()
        .ok_or_else(|| format!("no primary file for version {}", version_info.id))?;
    let dependencies = provider
        .resolve_dependencies(&version_info.id)
        .unwrap_or_default();
    manifest.mods.remove(idx);
    remove_mod_file_from_disk(manifest_path, &old_mod);
    let mut new_spec = build_mod_spec(
        &project,
        &version_info,
        file,
        dependencies,
        infer_project_side(Some(&project)),
    );
    new_spec.id = old_mod.id.clone();
    manifest.mods.push(new_spec);
    let report = commit_single_mod_update(app, manifest_path, manifest, &old_mod, true)?;
    let _ = report;
    Ok(format!("Reinstalled {} ({})", old_mod.name, version_info.version_number))
}

/// Applies a machine-actionable fix produced by crash diagnosis.
fn execute_fix_action_inner(
    app: &tauri::AppHandle,
    path: &str,
    action: &FixAction,
    skip_snapshot: bool,
) -> Result<String, String> {
    let manifest_path = PathBuf::from(path);
    let mod_id = action.mod_id.clone().unwrap_or_default();
    match action.kind.as_str() {
        "disableMod" => {
            if mod_id.is_empty() {
                return Err("disableMod requires a mod id".into());
            }
            let res = disable_project_mod_inner(&manifest_path, &mod_id)?;
            Ok(format!(
                "Disabled {} ({})",
                res.get("id").and_then(|v| v.as_str()).unwrap_or(&mod_id),
                res.get("fileName")
                    .and_then(|v| v.as_str())
                    .unwrap_or("jar")
            ))
        }
        "removeMod" => {
            if mod_id.is_empty() {
                return Err("removeMod requires a mod id".into());
            }
            remove_project_mod_inner(&manifest_path, &mod_id)?;
            Ok(format!("Removed {mod_id}"))
        }
        "reinstallMod" => {
            if mod_id.is_empty() {
                return Err("reinstallMod requires a mod id".into());
            }
            if !skip_snapshot {
                auto_snapshot(&manifest_path, "fix-reinstall-mod").map_err(|e| e.to_string())?;
            }
            let mut manifest = ProjectManifest::load_from_path(path).map_err(|e| e.to_string())?;
            let msg = apply_mod_reinstall(app, &manifest_path, &mut manifest, &mod_id)?;
            save_manifest(&manifest_path, &manifest).map_err(|e| e.to_string())?;
            Ok(msg)
        }
        "updateMod" => {
            if mod_id.is_empty() {
                return Err("updateMod requires a mod id".into());
            }
            if !skip_snapshot {
                auto_snapshot(&manifest_path, "fix-update-mod").map_err(|e| e.to_string())?;
            }
            let mut manifest = ProjectManifest::load_from_path(path).map_err(|e| e.to_string())?;
            let msg = apply_mod_update_to_latest(app, &manifest_path, &mut manifest, &mod_id)?;
            save_manifest(&manifest_path, &manifest).map_err(|e| e.to_string())?;
            Ok(msg)
        }
        "installDependency" => {
            if mod_id.is_empty() {
                return Err("installDependency requires a mod id".into());
            }
            if !skip_snapshot {
                auto_snapshot(&manifest_path, "fix-install-dep").map_err(|e| e.to_string())?;
            }
            let mut manifest = ProjectManifest::load_from_path(path).map_err(|e| e.to_string())?;
            install_modrinth_with_dependencies(&mut manifest, &[mod_id.clone()], "both", None)?;
            save_manifest(&manifest_path, &manifest).map_err(|e| e.to_string())?;
            download_project_mods_tracked(app, &manifest_path, &manifest, None, false);
            Ok(format!("Installed dependency {mod_id}"))
        }
        "updateLoader" => {
            if !skip_snapshot {
                auto_snapshot(&manifest_path, "fix-update-loader").map_err(|e| e.to_string())?;
            }
            let mut manifest = ProjectManifest::load_from_path(path).map_err(|e| e.to_string())?;
            let (loader_slug, _loaders) = update_loaders_for(&manifest);
            let latest = tuffbox_core::versions::fetch_loader_versions(
                &loader_slug,
                &manifest.minecraft.version,
            )
            .into_iter()
            .max_by(|a, b| a.id.cmp(&b.id))
            .ok_or_else(|| format!("no {loader_slug} build for {}", manifest.minecraft.version))?;
            manifest.loader.version = latest.id.clone();
            save_manifest(&manifest_path, &manifest).map_err(|e| e.to_string())?;
            Ok(format!("Updated loader to {}", latest.id))
        }
        "raiseMemory" => {
            if !skip_snapshot {
                auto_snapshot(&manifest_path, "fix-raise-memory").map_err(|e| e.to_string())?;
            }
            let mut manifest = ProjectManifest::load_from_path(path).map_err(|e| e.to_string())?;
            let target = 6144u32;
            for profile in manifest.profiles.iter_mut() {
                if profile.memory_mb.map(|m| m < target).unwrap_or(true) {
                    profile.memory_mb = Some(target);
                }
            }
            save_manifest(&manifest_path, &manifest).map_err(|e| e.to_string())?;
            Ok(format!("Set allocated memory to {} MB", target))
        }
        "acceptEula" => {
            let instance_dir = tuffbox_core::instance_dir_for_manifest(&manifest_path)
                .ok_or_else(|| "could not resolve instance directory".to_string())?;
            let eula_path = instance_dir.join("eula.txt");
            std::fs::write(&eula_path, "# Auto-accepted by TuffBox crash fix\neula=true\n")
                .map_err(|e| format!("failed to write {}: {e}", eula_path.display()))?;
            Ok("Set eula=true in eula.txt".into())
        }
        "changePort" => {
            let instance_dir = tuffbox_core::instance_dir_for_manifest(&manifest_path)
                .ok_or_else(|| "could not resolve instance directory".to_string())?;
            let props_path = instance_dir.join("server.properties");
            let content = std::fs::read_to_string(&props_path).unwrap_or_default();
            let mut props = tuffbox_core::properties_parser::PropertiesFile::parse(&content);
            props.set("server-port", "25566");
            std::fs::write(&props_path, props.to_string())
                .map_err(|e| format!("failed to write {}: {e}", props_path.display()))?;
            Ok("Changed server-port to 25566".into())
        }
        "autoJava" => {
            if !skip_snapshot {
                auto_snapshot(&manifest_path, "fix-auto-java").map_err(|e| e.to_string())?;
            }
            let mut manifest = ProjectManifest::load_from_path(path).map_err(|e| e.to_string())?;
            // ensure_java_for_minecraft_with_log picks the best installed
            // runtime for the Minecraft version AND downloads a managed JDK
            // (GraalVM for modern majors, Adoptium Temurin for legacy
            // Java 8/16) when no matching runtime is installed.
            let best = {
                let log = |line: &str| eprintln!("[autoJava] {line}");
                tuffbox_core::jre::ensure_java_for_minecraft_with_log(
                    &manifest.minecraft.version,
                    log,
                )
                .map_err(|e| format!("failed to provision Java: {e}"))?
            };
            let mut java = manifest.java.clone().unwrap_or(tuffbox_core::manifest::JavaSpec {
                major: None,
                distribution: None,
                path: None,
            });
            java.path = Some(best.path.clone());
            java.major = Some(best.major.try_into().unwrap_or(u16::MAX));
            manifest.java = Some(java);
            save_manifest(&manifest_path, &manifest).map_err(|e| e.to_string())?;
            Ok(format!("Selected Java {} ({})", best.major, best.path))
        }
        other => Err(format!("unknown fix action kind: {other}")),
    }
}

fn fix_action_batch_order(kind: &str) -> u8 {
    match kind {
        "installDependency" | "installAllMissing" | "installMissingForMod" => 0,
        "updateMod" | "reinstallMod" | "updateLoader" => 1,
        "raiseMemory" | "autoJava" | "acceptEula" | "changePort" => 2,
        "disableMod" | "removeMod" | "removeWrongJar" => 3,
        _ => 2,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixActionOutcome {
    action: FixAction,
    ok: bool,
    summary: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixActionBatchResult {
    applied: Vec<FixActionOutcome>,
    stopped: bool,
    summary: String,
}

#[tauri::command(rename_all = "camelCase")]
async fn apply_fix_actions(
    app: tauri::AppHandle,
    path: String,
    actions: Vec<FixAction>,
) -> Result<FixActionBatchResult, String> {
    if actions.is_empty() {
        return Ok(FixActionBatchResult {
            applied: Vec::new(),
            stopped: false,
            summary: "No actions to apply".into(),
        });
    }
    let path_for_record = path.clone();
    let mut ordered = actions;
    ordered.sort_by_key(|a| fix_action_batch_order(&a.kind));

    let app_handle = app.clone();
    let result = tokio::task::spawn_blocking(move || {
        let manifest_path = PathBuf::from(&path);
        auto_snapshot(&manifest_path, "fix-batch").map_err(|e| e.to_string())?;

        let mut applied = Vec::new();
        let mut launcher_actions = Vec::new();
        let mut summaries = Vec::new();
        let mut stopped = false;

        for action in ordered {
            match execute_fix_action_inner(&app_handle, &path, &action, true) {
                Ok(summary) => {
                    summaries.push(summary.clone());
                    launcher_actions.push(fix_action_to_launcher_action(&action, &summary));
                    applied.push(FixActionOutcome {
                        action: action.clone(),
                        ok: true,
                        summary: Some(summary),
                        error: None,
                    });
                }
                Err(err) => {
                    applied.push(FixActionOutcome {
                        action: action.clone(),
                        ok: false,
                        summary: None,
                        error: Some(err.clone()),
                    });
                    stopped = true;
                    break;
                }
            }
        }

        let summary = if summaries.is_empty() {
            "No fixes applied".into()
        } else {
            format!("Applied {} fix(es): {}", summaries.len(), summaries.join("; "))
        };

        Ok::<_, String>((applied, stopped, summary, launcher_actions))
    })
    .await
    .map_err(|e| e.to_string())??;

    let (applied, stopped, summary, launcher_actions) = result;
    if !launcher_actions.is_empty() {
        let _ = swarm_api::record_user_fix_attempt(
            Path::new(&path_for_record),
            "diagnose_batch",
            &summary,
            launcher_actions,
            None,
        );
    }

    Ok(FixActionBatchResult {
        applied,
        stopped,
        summary,
    })
}

#[tauri::command(rename_all = "camelCase")]
async fn apply_fix_action(
    app: tauri::AppHandle,
    path: String,
    action: FixAction,
) -> Result<String, String> {
    let path_for_record = path.clone();
    let action_for_record = action.clone();
    let result = tokio::task::spawn_blocking(move || {
        execute_fix_action_inner(&app, &path, &action, false)
    })
    .await
    .map_err(|e| e.to_string())??;

    let launcher = fix_action_to_launcher_action(&action_for_record, &result);
    let _ = swarm_api::record_user_fix_attempt(
        Path::new(&path_for_record),
        "diagnose_hint",
        &result,
        vec![launcher],
        None,
    );

    Ok(result)
}

fn fix_action_to_launcher_action(
    action: &FixAction,
    summary: &str,
) -> tuffbox_core::action_plan::LauncherAction {
    let op = match action.kind.as_str() {
        "disableMod" => "disable_mod",
        "removeMod" => "remove_mod",
        "reinstallMod" => "reinstall_mod",
        "updateMod" => "update_mod",
        "installDependency" => "install_mod",
        "updateLoader" => "update_loader",
        "raiseMemory" => "raise_memory",
        "acceptEula" => "accept_eula",
        "changePort" => "change_port",
        "autoJava" => "auto_java",
        other => other,
    };
    tuffbox_core::action_plan::LauncherAction {
        op: op.into(),
        mod_id: action.mod_id.clone(),
        provider: if op == "install_mod" {
            Some("modrinth".into())
        } else {
            None
        },
        project_id: if op == "install_mod" {
            action.mod_id.clone()
        } else {
            None
        },
        version: None,
        path: None,
        patch_type: None,
        patch: None,
        reason: Some(summary.to_string()),
        risk: "medium".into(),
    }
}

/// One jar sitting in a project's `mods/` folder that was built for a
/// different loader than the project uses (e.g. a Forge jar in a Fabric
/// project). Shared by the Diagnostics command and the launch preflight.
struct WrongLoaderFinding {
    file_name: String,
    /// Loader(s) the jar was built for, e.g. "forge" or "neoforge, fabric".
    jar_loaders: String,
}

/// Core wrong-loader scan shared by the `detect_wrong_loader_mods` command
/// and the launch preflight ('Play does not lie'). Scans `mods_dir` for
/// `.jar`s NOT tracked in the manifest and identifies them via Modrinth hash
/// lookup, flagging jars whose loaders exclude `project_loader`.
fn scan_wrong_loader_jars(
    mods_dir: &Path,
    project_loader: &str,
    tracked: &[String],
) -> Vec<WrongLoaderFinding> {
    let mut findings = Vec::new();

    let entries = match std::fs::read_dir(mods_dir) {
        Ok(e) => e,
        Err(_) => return findings,
    };

    let provider = tuffbox_core::ModrinthProvider::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.extension().map_or(false, |e| e == "jar") {
            continue;
        }
        let file_name = entry.file_name().to_string_lossy().to_string();
        // Skip if already tracked in manifest
        if tracked.iter().any(|t| t == &file_name) {
            continue;
        }

        // Try to identify via Modrinth hash lookup
        let sha1: String = match tuffbox_core::mc_install::sha1_file(&path) {
            Ok(h) => h,
            Err(_) => continue,
        };

        let identified = provider
            .get_version_by_hash(&sha1)
            .ok()
            .flatten();

        if let Some(version) = identified {
            let jar_loaders: Vec<&str> = version.loaders.iter().map(|s| s.as_str()).collect();
            // Empty loader list = loader-agnostic jar; only flag real mismatches.
            if !jar_loaders.is_empty() && !jar_loaders.contains(&project_loader) {
                findings.push(WrongLoaderFinding {
                    file_name,
                    jar_loaders: jar_loaders.join(", "),
                });
            }
        }
    }
    findings
}

/// Scans the `mods/` folder for `.jar` files that appear to be built for a
/// different mod loader than what the project uses (e.g. a Forge mod in a
/// Fabric project), and returns a list of suggestions with the file name
/// and a recommendation.
#[tauri::command(rename_all = "camelCase")]
async fn detect_wrong_loader_mods(path: String) -> Result<Vec<serde_json::Value>, String> {
    tokio::task::spawn_blocking(move || {
        let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
        let project_dir = PathBuf::from(&path)
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default();
        let project_loader =
            tuffbox_core::graph::loader_kind_slug(&manifest.loader.kind).to_string();
        let tracked: Vec<String> = manifest
            .mods
            .iter()
            .filter_map(|m| m.file_name.clone())
            .collect();
        let findings = scan_wrong_loader_jars(&project_dir.join("mods"), &project_loader, &tracked);
        Ok(findings
            .into_iter()
            .map(|f| {
                serde_json::json!({
                    "fileName": f.file_name,
                    "detectedLoader": f.jar_loaders,
                    "projectLoader": project_loader,
                    "recommendation": "disable",
                    "reason": format!(
                        "{} was built for {} but this project uses {}. Disable it (.jar.disabled) or remove it.",
                        f.file_name, f.jar_loaders, project_loader
                    ),
                })
            })
            .collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// A `mods/` file name received over IPC must be a BARE name — never a path.
/// `mods_dir.join("../manifest.json")` would otherwise rename/delete files
/// OUTSIDE the mods directory. Defense in depth: the UI passes scanned
/// names, but one crafted or future caller value must not escape the folder.
fn mods_dir_file(mods_dir: &Path, file_name: &str) -> Result<PathBuf, String> {
    let name = file_name.trim();
    let p = Path::new(name);
    if name.is_empty()
        || p.is_absolute()
        || p.components().count() != 1
        || name == "."
        || name == ".."
    {
        return Err(format!("invalid mods file name: '{file_name}'"));
    }
    Ok(mods_dir.join(name))
}

/// Renames a .jar file in mods/ to .jar.disabled so Minecraft won't load it.
#[tauri::command(rename_all = "camelCase")]
async fn disable_wrong_loader_jar(path: String, file_name: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let project_dir = PathBuf::from(&path)
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default();
        let mods_dir = project_dir.join("mods");
        let src = mods_dir_file(&mods_dir, &file_name)?;
        let dst = mods_dir_file(&mods_dir, &format!("{file_name}.disabled"))?;
        if !src.is_file() {
            return Err(format!("{} not found in mods/", file_name));
        }
        std::fs::rename(&src, &dst).map_err(|e| e.to_string())?;
        Ok(format!("{} → {}.disabled", file_name, file_name))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Removes a specific file from mods/ (used for wrong-loader jar cleanup).
#[tauri::command(rename_all = "camelCase")]
async fn remove_loose_jar(path: String, file_name: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let project_dir = PathBuf::from(&path)
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default();
        let target = mods_dir_file(&project_dir.join("mods"), &file_name)?;
        if !target.is_file() {
            return Err(format!("{} not found in mods/", file_name));
        }
        // Remember the content hash so the store object can be released
        // when no other pack uses these bytes.
        let sha1 = tuffbox_core::sha1_file(&target).ok();
        // The jar may be a dedup-store hardlink carrying the store's
        // read-only attribute — clear it or remove fails on Windows.
        tuffbox_core::fs_util::clear_readonly(&target);
        std::fs::remove_file(&target).map_err(|e| e.to_string())?;
        if let Some(sha1) = sha1 {
            release_store_object_detached(sha1);
        }
        Ok(format!("Removed {}", file_name))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// One jar inside a duplicate-`mod_id` group.
struct DuplicateJarEntry {
    file_name: String,
    mod_id: String,
    mtime_ms: u64,
    size: u64,
    in_manifest: bool,
}

/// A true-duplicate group in `mods/`: several jars sharing one fabric/forge
/// `mod_id`. Shared by the Duplicates command and Pack Health.
struct DuplicateJarGroup {
    mod_id: String,
    /// Newest jar — recommended survivor when deduplicating.
    keep_candidate: String,
    jars: Vec<DuplicateJarEntry>,
}

/// Groups jars in `mods/` that share the same fabric/forge `mod_id`.
/// Jars sort newest-first within a group (the keep candidate leads);
/// groups sort by mod id.
fn scan_duplicate_mod_jars(
    mods_dir: &Path,
    tracked: &std::collections::HashSet<String>,
) -> Vec<DuplicateJarGroup> {
    let entries = match std::fs::read_dir(mods_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    // mod_id → jars
    let mut by_id: std::collections::HashMap<String, Vec<DuplicateJarEntry>> =
        std::collections::HashMap::new();

    for entry in entries.flatten() {
        let jar_path = entry.path();
        if !jar_path
            .extension()
            .map_or(false, |e| e.eq_ignore_ascii_case("jar"))
        {
            continue;
        }
        let file_name = entry.file_name().to_string_lossy().to_string();
        let Ok(meta) = std::fs::metadata(&jar_path) else {
            continue;
        };
        let mtime_ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let size = meta.len();
        let mod_id = match tuffbox_core::mod_scan::scan_mod_jar(&jar_path) {
            Ok(scan) => scan
                .mod_id
                .map(|s| s.trim().to_lowercase())
                .filter(|s| !s.is_empty()),
            Err(_) => None,
        };
        let Some(mod_id) = mod_id else {
            continue;
        };
        by_id
            .entry(mod_id.clone())
            .or_default()
            .push(DuplicateJarEntry {
                in_manifest: tracked.contains(&file_name),
                file_name,
                mod_id,
                mtime_ms,
                size,
            });
    }

    let mut groups: Vec<DuplicateJarGroup> = by_id
        .into_iter()
        .filter(|(_, jars)| jars.len() > 1)
        .map(|(mod_id, mut jars)| {
            jars.sort_by(|a, b| {
                b.mtime_ms
                    .cmp(&a.mtime_ms)
                    .then_with(|| a.file_name.cmp(&b.file_name))
            });
            let keep_candidate = jars
                .first()
                .map(|j| j.file_name.clone())
                .unwrap_or_default();
            DuplicateJarGroup {
                mod_id,
                keep_candidate,
                jars,
            }
        })
        .collect();
    groups.sort_by(|a, b| a.mod_id.cmp(&b.mod_id));
    groups
}

/// Groups jars in `mods/` that share the same fabric/forge `mod_id` (true duplicates).
#[tauri::command(rename_all = "camelCase")]
async fn detect_duplicate_mod_jars(path: String) -> Result<Vec<serde_json::Value>, String> {
    tokio::task::spawn_blocking(move || {
        let project_dir = PathBuf::from(&path)
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default();
        let manifest = ProjectManifest::load_from_path(&path).ok();
        let tracked: std::collections::HashSet<String> = manifest
            .as_ref()
            .map(|m| {
                m.mods
                    .iter()
                    .filter_map(|mod_| mod_.file_name.clone())
                    .collect()
            })
            .unwrap_or_default();

        Ok(scan_duplicate_mod_jars(&project_dir.join("mods"), &tracked)
            .into_iter()
            .map(|g| {
                serde_json::json!({
                    "modId": g.mod_id,
                    "keepCandidate": g.keep_candidate,
                    "jars": g.jars.iter().map(|j| serde_json::json!({
                        "fileName": j.file_name,
                        "modId": j.mod_id,
                        "mtimeMs": j.mtime_ms,
                        "size": j.size,
                        "inManifest": j.in_manifest,
                    })).collect::<Vec<serde_json::Value>>(),
                })
            })
            .collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Keeps one jar for a duplicate `mod_id` group and deletes the others.
/// Updates the manifest `fileName` to the kept jar when needed.
#[tauri::command(rename_all = "camelCase")]
async fn keep_one_duplicate_mod_jar(
    path: String,
    mod_id: String,
    keep_file_name: String,
) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let mod_id_l = mod_id.trim().to_lowercase();
        if mod_id_l.is_empty() {
            return Err("modId is empty".into());
        }
        let manifest_path = PathBuf::from(&path);
        let project_dir = manifest_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default();
        let mods_dir = project_dir.join("mods");
        let keep_path = mods_dir_file(&mods_dir, &keep_file_name)?;
        if !keep_path.is_file() {
            return Err(format!("{keep_file_name} not found in mods/"));
        }

        // Confirm keep jar actually belongs to this mod_id.
        let keep_scan = tuffbox_core::mod_scan::scan_mod_jar(&keep_path).map_err(|e| e.to_string())?;
        let keep_id = keep_scan
            .mod_id
            .as_deref()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty());
        if keep_id.as_deref() != Some(mod_id_l.as_str()) {
            return Err(format!(
                "{keep_file_name} is not mod `{mod_id}` (got {:?})",
                keep_id
            ));
        }

        auto_snapshot(&manifest_path, "dedupe-mod-jars").map_err(|e| e.to_string())?;

        let mut removed = Vec::new();
        let entries = std::fs::read_dir(&mods_dir).map_err(|e| e.to_string())?;
        for entry in entries.flatten() {
            let jar_path = entry.path();
            if !jar_path
                .extension()
                .map_or(false, |e| e.eq_ignore_ascii_case("jar"))
            {
                continue;
            }
            let file_name = entry.file_name().to_string_lossy().to_string();
            if file_name == keep_file_name {
                continue;
            }
            let Ok(scan) = tuffbox_core::mod_scan::scan_mod_jar(&jar_path) else {
                continue;
            };
            let Some(id) = scan
                .mod_id
                .as_deref()
                .map(|s| s.trim().to_lowercase())
                .filter(|s| !s.is_empty())
            else {
                continue;
            };
            if id != mod_id_l {
                continue;
            }
            let sha1 = tuffbox_core::sha1_file(&jar_path).ok();
            tuffbox_core::fs_util::clear_readonly(&jar_path);
            std::fs::remove_file(&jar_path).map_err(|e| e.to_string())?;
            if let Some(sha1) = sha1 {
                release_store_object_detached(sha1);
            }
            removed.push(file_name);
        }

        // Point any manifest entry for this mod at the kept jar; drop extras.
        if let Ok(mut manifest) = ProjectManifest::load_from_path(&path) {
            let mut matched = false;
            let mut drop_idxs = Vec::new();
            for (i, m) in manifest.mods.iter_mut().enumerate() {
                let same_id = m.id.eq_ignore_ascii_case(&mod_id_l)
                    || m.source
                        .project_id
                        .as_deref()
                        .map(|p| p.eq_ignore_ascii_case(&mod_id_l))
                        .unwrap_or(false)
                    || m.file_name
                        .as_deref()
                        .map(|f| removed.iter().any(|r| r == f) || f == keep_file_name)
                        .unwrap_or(false);
                if !same_id {
                    continue;
                }
                if !matched {
                    m.file_name = Some(keep_file_name.clone());
                    matched = true;
                } else {
                    drop_idxs.push(i);
                }
            }
            for i in drop_idxs.into_iter().rev() {
                manifest.mods.remove(i);
            }
            save_manifest(&manifest_path, &manifest).map_err(|e| e.to_string())?;
        }

        if removed.is_empty() {
            Ok(format!("Kept {keep_file_name} (no other jars for `{mod_id}`)"))
        } else {
            Ok(format!(
                "Kept {keep_file_name}; removed {}: {}",
                removed.len(),
                removed.join(", ")
            ))
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
fn list_config_files(path: String) -> Result<Vec<ConfigFileSummary>, String> {
    let project_dir = manifest_parent(&path)?;
    let mut files = Vec::new();
    for root in ["config", "defaultconfigs", "kubejs", "scripts", "overrides"] {
        let dir = project_dir.join(root);
        if dir.is_dir() {
            collect_config_files(&project_dir, &dir, &mut files).map_err(|e| e.to_string())?;
        }
    }
    // Root-level options.txt (History tracks it; Tune should edit it too).
    let options = project_dir.join("options.txt");
    if options.is_file() {
        if let Ok(metadata) = std::fs::metadata(&options) {
            if metadata.len() <= 2 * 1024 * 1024 {
                let modified = metadata
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|duration| duration.as_secs());
                files.push(ConfigFileSummary {
                    name: "options.txt".into(),
                    extension: "txt".into(),
                    path: "options.txt".into(),
                    size: metadata.len(),
                    modified,
                });
            }
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

#[tauri::command(rename_all = "camelCase")]
fn read_config_file(path: String, relative_path: String) -> Result<String, String> {
    let project_dir = manifest_parent(&path)?;
    let target = safe_project_file(&project_dir, &relative_path)?;
    let metadata = std::fs::metadata(&target).map_err(|e| e.to_string())?;
    if metadata.len() > 2 * 1024 * 1024 {
        return Err("file is too large for the MVP config editor".to_string());
    }
    std::fs::read_to_string(target).map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
fn write_config_file(
    path: String,
    relative_path: String,
    content: String,
) -> Result<WriteConfigResult, String> {
    if content.len() > 2 * 1024 * 1024 {
        return Err("file is too large for the MVP config editor".to_string());
    }
    let manifest_path = PathBuf::from(&path);
    let project_dir = manifest_parent(&path)?;
    let target = safe_project_file(&project_dir, &relative_path)?;
    let snap = auto_snapshot_detailed(
        &manifest_path,
        "edit-config",
        &[PathBuf::from(&relative_path)],
        &[format!("Edited config {relative_path}")],
    )
    .map_err(|e| e.to_string())?;
    // Atomic write + .bak-<ts> of the previous content (Reality
    // Launcher-style): a crash mid-write can no longer truncate configs.
    tuffbox_core::fs_util::atomic_write_with_backup(&target, content.as_bytes())
        .map_err(|e| e.to_string())?;
    Ok(WriteConfigResult {
        snapshot_id: snap.id,
    })
}

/// Pretty-print a TOML document; returns Err on parse failure.
#[tauri::command(rename_all = "camelCase")]
fn format_toml(content: String) -> Result<String, String> {
    let value: toml::Value = content
        .parse()
        .map_err(|e| format!("TOML parse error: {e}"))?;
    toml::to_string_pretty(&value).map_err(|e| e.to_string())
}

/// Full-text search across all config and script files in the project.
#[tauri::command(rename_all = "camelCase")]
fn search_in_configs(path: String, query: String) -> Result<Vec<serde_json::Value>, String> {
    let project_dir = manifest_parent(&path)?;
    let roots = ["config", "defaultconfigs", "kubejs", "scripts", "overrides"];
    let whitelist: &[&str] = &[
        "json",
        "json5",
        "toml",
        "properties",
        "cfg",
        "yaml",
        "yml",
        "js",
        "zs",
        "txt",
        "md",
        "html",
        "css",
        "sh",
    ];
    let query_lower = query.to_lowercase();
    let mut results = Vec::new();

    fn walk(dir: &Path, cb: &mut dyn FnMut(&Path)) {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                walk(&p, cb);
            } else {
                cb(&p);
            }
        }
    }

    for root in &roots {
        let root_dir = project_dir.join(root);
        if !root_dir.is_dir() {
            continue;
        }
        walk(&root_dir, &mut |file_path: &Path| {
            if results.len() >= 200 {
                return;
            }
            let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if !whitelist.contains(&ext) {
                return;
            }
            let Ok(content) = std::fs::read_to_string(file_path) else {
                return;
            };
            if content.len() > 1024 * 1024 {
                return;
            }
            for (line_no, line) in content.lines().enumerate() {
                if line.to_lowercase().contains(&query_lower) {
                    if let Ok(rel) = file_path.strip_prefix(&project_dir) {
                        results.push(serde_json::json!({
                            "path": rel.to_string_lossy().replace('\\', "/"),
                            "line": line_no + 1,
                            "text": line.trim().chars().take(200).collect::<String>(),
                        }));
                    }
                    if results.len() >= 200 {
                        return;
                    }
                }
            }
        });
        if results.len() >= 200 {
            break;
        }
    }
    // Also search root options.txt
    let options = project_dir.join("options.txt");
    if options.is_file() && results.len() < 200 {
        if let Ok(content) = std::fs::read_to_string(&options) {
            if content.len() <= 1024 * 1024 {
                for (line_no, line) in content.lines().enumerate() {
                    if line.to_lowercase().contains(&query_lower) {
                        results.push(serde_json::json!({
                            "path": "options.txt",
                            "line": line_no + 1,
                            "text": line.trim().chars().take(200).collect::<String>(),
                        }));
                        if results.len() >= 200 {
                            break;
                        }
                    }
                }
            }
        }
    }
    Ok(results)
}

/// ── Launch statistics (like NitroLaunch stats plugin) ──────────

/// Records a launch event in the project stats.
#[tauri::command(rename_all = "camelCase")]
fn record_launch(path: String) -> Result<(), String> {
    let project_dir = manifest_parent(&path)?;
    let mut stats = load_stats(&project_dir);
    let entry = stats.instances.entry("client".into()).or_default();
    entry.launches += 1;
    entry.last_launch = Some(tuffbox_core::time_util::rfc3339_now());
    save_stats(&project_dir, &stats)?;
    // Size / playtime cache must refresh after a session.
    helpers::invalidate_recent_home_cache(&path);
    Ok(())
}

/// Records a crash event in the project stats.
#[tauri::command(rename_all = "camelCase")]
fn record_crash(path: String) -> Result<(), String> {
    let project_dir = manifest_parent(&path)?;
    let mut stats = load_stats(&project_dir);
    let entry = stats.instances.entry("client".into()).or_default();
    entry.crashes += 1;
    save_stats(&project_dir, &stats)?;
    helpers::invalidate_recent_home_cache(&path);
    Ok(())
}

/// Returns launch/crash statistics for the project.
#[tauri::command(rename_all = "camelCase")]
fn get_launch_stats(path: String) -> Result<serde_json::Value, String> {
    let project_dir = manifest_parent(&path)?;
    let stats = load_stats(&project_dir);
    let mut all_launches = 0u64;
    let mut all_crashes = 0u64;
    let mut all_playtime = 0u64;
    let mut last = None;
    for (_id, inst) in &stats.instances {
        all_launches += inst.launches;
        all_crashes += inst.crashes;
        all_playtime += inst.total_playtime_seconds;
        if inst.last_launch.is_some() {
            last = inst.last_launch.clone();
        }
    }
    Ok(serde_json::json!({
        "totalLaunches": all_launches,
        "totalCrashes": all_crashes,
        "totalPlaytimeSeconds": all_playtime,
        "lastLaunch": last,
        "byProfile": stats.instances.iter().map(|(id, inst)| serde_json::json!({
            "id": id, "launches": inst.launches, "crashes": inst.crashes,
            "playtimeSeconds": inst.total_playtime_seconds,
            "lastLaunch": inst.last_launch,
        })).collect::<Vec<_>>(),
    }))
}

#[tauri::command(rename_all = "camelCase")]
fn get_manifest_schema(path: String) -> Result<serde_json::Value, String> {
    let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "schemaVersion": manifest.schema_version,
        "hasBrief": manifest.brief.is_some(),
        "modCount": manifest.mods.len(),
        "profileCount": manifest.profiles.len(),
    }))
}

/// ── Offline validation / project health report ──────────────────────

/// Runs a set of offline checks on a project without launching Minecraft:
/// JSON syntax errors in config files, missing dependency edges in the graph,
/// circular dependency warnings, and a generated testing checklist.
#[tauri::command(rename_all = "camelCase")]
async fn run_project_validation(path: String) -> Result<serde_json::Value, String> {
    tokio::task::spawn_blocking(move || run_project_validation_impl(path))
        .await
        .map_err(|e| e.to_string())?
}

fn run_project_validation_impl(path: String) -> Result<serde_json::Value, String> {
    let manifest = manifest_for_graph(&path)?;
    let graph = DependencyGraph::from_manifest(&manifest);
    let diagnostics = Resolver::analyze_project(&manifest, &graph);
    let project_dir = manifest_parent(&path)?;

    // Check JSON files for parse errors
    let mut json_errors: Vec<serde_json::Value> = Vec::new();
    let roots = ["config", "defaultconfigs", "kubejs", "scripts"];
    for root in &roots {
        let dir = project_dir.join(root);
        if !dir.is_dir() {
            continue;
        }
        fn walk_json(dir: &Path, acc: &mut Vec<serde_json::Value>) {
            let entries = match std::fs::read_dir(dir) {
                Ok(e) => e,
                Err(_) => return,
            };
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    walk_json(&p, acc);
                    continue;
                }
                if p.extension().map_or(false, |e| e == "json") {
                    if let Ok(content) = std::fs::read_to_string(&p) {
                        if content.len() < 512 * 1024 {
                            if let Err(e) = serde_json::from_str::<serde_json::Value>(&content) {
                                acc.push(serde_json::json!({
                                    "path": p.strip_prefix(dir.parent().unwrap_or(&dir)).unwrap_or(&p).to_string_lossy(),
                                    "error": e.to_string(),
                                }));
                            }
                        }
                    }
                }
                if acc.len() >= 50 {
                    return;
                }
            }
        }
        walk_json(&dir, &mut json_errors);
    }

    // Check for circular dependencies in the manifest mod list
    let mut circular: Vec<Vec<String>> = Vec::new();
    {
        let mut seen = std::collections::HashSet::new();
        for m in &manifest.mods {
            for dep in &m.dependencies {
                if dep.kind == tuffbox_core::manifest::DependencyKind::Requires {
                    let target = &dep.target;
                    if let Some(target_mod) = manifest.mods.iter().find(|t| t.id == *target) {
                        if target_mod.dependencies.iter().any(|d| d.target == m.id) {
                            let pair = vec![m.id.clone(), target.clone()];
                            let key = if m.id < *target {
                                (m.id.clone(), target.clone())
                            } else {
                                (target.clone(), m.id.clone())
                            };
                            let key_str = format!("{}<=>{}", key.0, key.1);
                            if seen.insert(key_str) {
                                circular.push(pair);
                            }
                        }
                    }
                }
            }
        }
    }

    let error_diags: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == tuffbox_core::DiagnosticSeverity::Error)
        .collect();
    let warning_diags: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == tuffbox_core::DiagnosticSeverity::Warning)
        .collect();
    let mods_without_source = manifest
        .mods
        .iter()
        .filter(|m| m.source.url.is_none())
        .count();
    let mods_without_hash = manifest
        .mods
        .iter()
        .filter(|m| m.hashes.as_ref().and_then(|h| h.sha1.as_ref()).is_none())
        .count();

    Ok(serde_json::json!({
        "passed": error_diags.is_empty() && json_errors.is_empty() && circular.is_empty(),
        "jsonErrors": json_errors,
        "graphErrors": error_diags.len(),
        "graphWarnings": warning_diags.len(),
        "graphErrorList": error_diags.iter().map(|d| serde_json::json!({"code": d.code, "message": d.message})).collect::<Vec<_>>(),
        "graphWarningList": warning_diags.iter().take(10).map(|d| serde_json::json!({"code": d.code, "message": d.message})).collect::<Vec<_>>(),
        "circularDeps": circular,
        "modsWithoutSource": mods_without_source,
        "modsWithoutHash": mods_without_hash,
        "totalMods": manifest.mods.len(),
        "totalProfiles": manifest.profiles.len(),
    }))
}

/// ── Batch update manager ────────────────────────────────────────────

/// Resolves the installed sha1 from disk, falling back to manifest metadata
/// only when the file is unavailable. The jar is the source of truth: an
/// interrupted older update may have already changed manifest metadata.
/// SHA1 of a jar, cached by (path, mtime nanos, size). Hashing every jar on
/// every update check re-read gigabytes on big packs; the file identity key
/// makes re-hashes happen only when the jar actually changes.
fn cached_sha1_file(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let key = format!("sha1:{}:{}:{}", path.display(), mtime, meta.len());
    if let Some(cached) = tuffbox_core::api_cache::get::<String>(&key) {
        return Some(cached);
    }
    let hash = tuffbox_core::sha1_file(path).ok()?;
    tuffbox_core::api_cache::put(&key, hash.clone());
    Some(hash)
}

fn resolve_mod_sha1(manifest_path: &Path, module: &ModSpec) -> Option<String> {
    if let Some(path) = existing_mod_file_path(manifest_path, module) {
        if let Some(hash) = cached_sha1_file(&path) {
            return Some(hash);
        }
    }
    module
        .hashes
        .as_ref()
        .and_then(|h| h.sha1.as_ref())
        .filter(|h| !h.is_empty())
        .cloned()
}

fn installed_matches_version(
    module: &ModSpec,
    installed_sha1: Option<&str>,
    latest: &tuffbox_core::VersionInfo,
) -> bool {
    // Modrinth version id is authoritative when the install already points
    // at it — hash metadata can be stale after interrupted updates.
    if module.source.file_id.as_deref() == Some(latest.id.as_str()) {
        return true;
    }
    if let Some(installed_sha1) = installed_sha1 {
        if latest.files.iter().any(|f| {
            f.hashes
                .sha1
                .as_deref()
                .is_some_and(|h| h.eq_ignore_ascii_case(installed_sha1))
        }) {
            return true;
        }
    }
    // Same published version string with no conflicting identity — skip the
    // false "update available" badge users see when hashes disagree in case
    // or the jar filename drifted while the install is already current.
    let installed_ver = module.version.trim();
    !installed_ver.is_empty()
        && installed_ver != "unknown"
        && installed_ver == latest.version_number.trim()
}

fn update_loaders_for(manifest: &ProjectManifest) -> (String, Vec<String>) {
    let loader_slug = tuffbox_core::graph::loader_kind_slug(&manifest.loader.kind).to_string();
    // Quilt can run many Fabric builds; try both like Prism expands loaders.
    let loaders = if loader_slug == "quilt" {
        vec![loader_slug.clone(), "fabric".to_string()]
    } else {
        vec![loader_slug.clone()]
    };
    (loader_slug, loaders)
}

/// Collects pending updates for the project's current Minecraft + loader.
/// Uses Modrinth `version_files/update` for hashed jars, then falls back to
/// `project/{id}/version` for Modrinth mods that still lack a usable hash.
fn resolve_pending_mod_updates(
    manifest_path: &Path,
    manifest: &ProjectManifest,
    provider: &tuffbox_core::ModrinthProvider,
) -> Result<(String, Vec<(usize, tuffbox_core::VersionInfo)>), String> {
    let (loader_slug, loaders) = update_loaders_for(manifest);
    let game_versions = vec![manifest.minecraft.version.clone()];

    let mut hash_to_mod: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let mut hashes: Vec<String> = Vec::new();
    let mut no_hash_idxs: Vec<usize> = Vec::new();

    for (idx, module) in manifest.mods.iter().enumerate() {
        let is_modrinth = module.source.kind == SourceKind::Modrinth
            || (module.source.kind != SourceKind::Curseforge
                && module.source.kind != SourceKind::Github
                && module.source.project_id.is_some());
        if !is_modrinth {
            continue;
        }
        match resolve_mod_sha1(manifest_path, module) {
            Some(hash) => {
                hash_to_mod.insert(hash.clone(), idx);
                hashes.push(hash);
            }
            None if module.source.project_id.is_some() => no_hash_idxs.push(idx),
            None => {}
        }
    }

    let mut pending: Vec<(usize, tuffbox_core::VersionInfo)> = Vec::new();
    let mut resolved_idxs = std::collections::HashSet::new();

    if !hashes.is_empty() {
        let latest_map = provider
            .get_latest_versions(&hashes, &loaders, &game_versions)
            .map_err(|e| e.to_string())?;
        for (hash, latest) in latest_map {
            let Some(&idx) = hash_to_mod.get(&hash) else {
                continue;
            };
            if installed_matches_version(&manifest.mods[idx], Some(&hash), &latest) {
                continue;
            }
            // Prefer versions that actually ship a file for our loader.
            if ProviderFileInfo::select_file_for_loader(&latest, &loader_slug).is_none() {
                continue;
            }
            resolved_idxs.insert(idx);
            pending.push((idx, latest));
        }
    }

    for idx in no_hash_idxs {
        if resolved_idxs.contains(&idx) {
            continue;
        }
        let module = &manifest.mods[idx];
        let Some(project_id) = module.source.project_id.as_ref() else {
            continue;
        };
        let query = ProviderSearchQuery {
            query: None,
            minecraft_version: Some(manifest.minecraft.version.clone()),
            loader: Some(loader_slug.clone()),
            ..Default::default()
        };
        let Ok(versions) = provider.get_versions(project_id, &query) else {
            continue;
        };
        let Some(latest) = versions.into_iter().next() else {
            continue;
        };
        if installed_matches_version(module, None, &latest) {
            continue;
        }
        if ProviderFileInfo::select_file_for_loader(&latest, &loader_slug).is_none() {
            continue;
        }
        pending.push((idx, latest));
    }

    Ok((loader_slug, pending))
}

/// Checks every Modrinth-sourced mod in the project for available updates,
/// comparing the installed version against the latest compatible version.
/// Uses Modrinth's batch update API plus disk-hash / project-id fallbacks.
/// Returns a list with update info for each mod that could be updated.
#[tauri::command(rename_all = "camelCase")]
async fn check_mod_updates(path: String) -> Result<Vec<serde_json::Value>, String> {
    tokio::task::spawn_blocking(move || {
        let manifest_path = PathBuf::from(&path);
        let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
        let provider = tuffbox_core::ModrinthProvider::new();
        let (loader_slug, pending) =
            resolve_pending_mod_updates(&manifest_path, &manifest, &provider)?;

        let mut updates = Vec::new();
        let project_loader = loader_slug.as_str();
        let project_mc = manifest.minecraft.version.as_str();
        for (idx, latest) in pending {
            let m = &manifest.mods[idx];
            let file = ProviderFileInfo::select_file_for_loader(&latest, &loader_slug).cloned();
            let new_file_name = file.as_ref().map(|f| f.filename.as_str()).unwrap_or("");
            let current_file_name = m.file_name.as_deref().unwrap_or("");
            // Quilt can run Fabric builds; treat those as non-breaking.
            let loader_ok = latest
                .loaders
                .iter()
                .any(|l| l == project_loader || (project_loader == "quilt" && l == "fabric"));
            let mc_ok = latest.game_versions.iter().any(|v| v == project_mc);
            let changelog = latest.changelog.as_deref().unwrap_or("").trim();
            let changelog_snippet: String = changelog
                .chars()
                .take(400)
                .collect::<String>()
                .trim()
                .to_string();
            updates.push(serde_json::json!({
                "modId": m.id,
                "name": m.name,
                "currentVersion": m.version,
                "latestVersion": latest.version_number,
                "versionId": latest.id,
                "fileName": new_file_name,
                "currentFileName": if current_file_name.is_empty() { None } else { Some(current_file_name) },
                "fileNameChanged": !current_file_name.is_empty() && current_file_name != new_file_name,
                "breakingLoader": !loader_ok,
                "breakingMinecraft": !mc_ok,
                "gameVersions": latest.game_versions,
                "loaders": latest.loaders,
                "changelog": changelog_snippet,
                "datePublished": latest.date_published,
                "versionType": latest.version_type,
                "iconUrl": m.source.icon_url,
            }));
        }
        Ok(updates)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Applies all available mod updates at once (batch update), creating
/// a single auto-snapshot before the changes. Uses Modrinth's batch
/// update API to resolve all updates in one request.
#[tauri::command(rename_all = "camelCase")]
async fn update_all_mods(
    app: tauri::AppHandle,
    path: String,
    dry_run: Option<bool>,
) -> Result<serde_json::Value, String> {
    tokio::task::spawn_blocking(move || {
        use tauri::Emitter;

        let dry_run = dry_run.unwrap_or(false);
        let _guard = MODS_IO_LOCK
            .lock()
            .map_err(|_| "mods I/O lock poisoned".to_string())?;

        emit_mod_update_progress(
            &app,
            "checking",
            "Checking Modrinth for compatible updates…",
            0,
            0,
            8,
            None,
        );
        let manifest_path = PathBuf::from(&path);
        let mut manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
        let mut updated = Vec::new();
        let mut skipped_errors: Vec<String> = Vec::new();

        let provider = tuffbox_core::ModrinthProvider::new();
        let (loader_slug, pending) =
            resolve_pending_mod_updates(&manifest_path, &manifest, &provider)?;

        // Mandatory dry-run: resolve the preview but do NOT create a snapshot,
        // touch the manifest, or download anything. The UI uses this to render
        // a confirm-with-diff modal before committing (spec: no update-all
        // without a preview). Reuses the enriched preview shape of
        // `check_mod_updates` (breaking flags + changelog snippet + rename).
        if dry_run {
            let project_loader = loader_slug.as_str();
            let project_mc = manifest.minecraft.version.as_str();
            let preview: Vec<serde_json::Value> = pending
                .into_iter()
                .map(|(idx, latest)| {
                    let m = &manifest.mods[idx];
                    let file =
                        ProviderFileInfo::select_file_for_loader(&latest, &loader_slug).cloned();
                    let new_file_name = file.as_ref().map(|f| f.filename.as_str()).unwrap_or("");
                    let current_file_name = m.file_name.as_deref().unwrap_or("");
                    let loader_ok = latest.loaders.iter().any(|l| {
                        l == project_loader || (project_loader == "quilt" && l == "fabric")
                    });
                    let mc_ok = latest.game_versions.iter().any(|v| v == project_mc);
                    let changelog_snippet: String = latest
                        .changelog
                        .as_deref()
                        .unwrap_or("")
                        .trim()
                        .chars()
                        .take(400)
                        .collect::<String>()
                        .trim()
                        .to_string();
                    serde_json::json!({
                        "modId": m.id,
                        "name": m.name,
                        "currentVersion": m.version,
                        "latestVersion": latest.version_number,
                        "versionId": latest.id,
                        "fileName": new_file_name,
                        "currentFileName": if current_file_name.is_empty() { None } else { Some(current_file_name) },
                        "fileNameChanged": !current_file_name.is_empty() && current_file_name != new_file_name,
                        "breakingLoader": !loader_ok,
                        "breakingMinecraft": !mc_ok,
                        "gameVersions": latest.game_versions,
                        "loaders": latest.loaders,
                        "changelog": changelog_snippet,
                        "datePublished": latest.date_published,
                        "versionType": latest.version_type,
                        "iconUrl": m.source.icon_url,
                    })
                })
                .collect();
            return Ok(serde_json::json!({
                "dryRun": true,
                "preview": preview,
                "count": preview.len(),
            }));
        }

        emit_mod_update_progress(
            &app,
            "preparing",
            "Creating a safety snapshot…",
            0,
            pending.len(),
            12,
            None,
        );
        let mut snapshot = auto_snapshot_before_mod_op(&manifest_path, "batch-update-all")
            .map_err(|e| e.to_string())?;

        let scope_mod_ids: Vec<String> = pending
            .iter()
            .map(|(idx, _)| manifest.mods[*idx].id.clone())
            .collect();
        if !pending.is_empty() {
            let queue: Vec<ModDownloadProgressPayload> = pending
                .iter()
                .map(|(idx, _)| {
                    let module = &manifest.mods[*idx];
                    ModDownloadProgressPayload {
                        id: module.id.clone(),
                        name: module.name.clone(),
                        downloaded: 0,
                        total: 0,
                        percent: 0,
                        status: "queued".to_string(),
                    }
                })
                .collect();
            let _ = app.emit(
                "mod-download-batch",
                serde_json::json!({
                    "phase": "start",
                    "items": queue,
                    "scopeModIds": scope_mod_ids,
                }),
            );
        }

        let mut download = tuffbox_core::ModSyncReport::default();
        let pending_count = pending.len();
        for (position, (idx, latest)) in pending.into_iter().enumerate() {
            let current_mod_id = manifest.mods[idx].id.clone();
            let current_mod_name = manifest.mods[idx].name.clone();
            let percent =
                20 + ((position as f64 / pending_count.max(1) as f64) * 70.0).round() as u32;
            emit_mod_update_progress(
                &app,
                "updating",
                &format!(
                    "Updating {} ({}/{})…",
                    current_mod_name,
                    position + 1,
                    pending_count
                ),
                position,
                pending_count,
                percent,
                Some(&current_mod_id),
            );
            let file = ProviderFileInfo::select_file_for_loader(&latest, &loader_slug).cloned();
            let Some(file) = file else {
                skipped_errors.push(format!(
                    "{}: no compatible file for loader {loader_slug}",
                    manifest.mods[idx].name
                ));
                emit_mod_download_status(&app, &current_mod_id, &current_mod_name, "failed", 0);
                continue;
            };

            let old_mod = manifest.mods[idx].clone();
            let project_id = latest.project_id.clone();
            let project = match provider.get_project(&project_id) {
                Ok(p) => p,
                Err(e) => {
                    skipped_errors.push(format!(
                        "{}: project lookup failed ({e}), using cached metadata",
                        old_mod.name
                    ));
                    project_info_from_mod(&old_mod)
                }
            };
            let previous_deps = old_mod.dependencies.clone();
            let dependencies = provider
                .resolve_dependencies(&latest.id)
                .unwrap_or(previous_deps);
            let mut new_spec = build_mod_spec(
                &project,
                &latest,
                file,
                dependencies,
                infer_project_side(Some(&project)),
            );
            // Keep references and frontend progress scopes valid across an
            // update even when the provider has renamed the project slug.
            new_spec.id = old_mod.id.clone();
            let name = new_spec.name.clone();
            manifest.mods[idx] = new_spec;
            match commit_single_mod_update(&app, &manifest_path, &mut manifest, &old_mod, false) {
                Ok(report) => {
                    download.downloaded.extend(report.downloaded);
                    download.already_present.extend(report.already_present);
                    download.skipped.extend(report.skipped);
                    download.failed.extend(report.failed);
                    updated.push(name);
                }
                Err(error) => {
                    manifest.mods[idx] = old_mod;
                    skipped_errors.push(format!("{name}: {error}"));
                    emit_mod_download_status(&app, &current_mod_id, &current_mod_name, "failed", 0);
                }
            }
        }

        emit_mod_update_progress(
            &app,
            "finalizing",
            "Finalizing the mod list…",
            pending_count,
            pending_count,
            95,
            None,
        );
        if !scope_mod_ids.is_empty() {
            let _ = app.emit(
                "mod-download-batch",
                serde_json::json!({
                    "phase": "done",
                    "downloaded": download.downloaded,
                    "failed": download.failed,
                    "alreadyPresent": download.already_present,
                    "skipped": download.skipped,
                    "scopeModIds": scope_mod_ids,
                    "batchComplete": true,
                }),
            );
        }
        emit_mod_update_progress(
            &app,
            "done",
            "Mod updates complete.",
            pending_count,
            pending_count,
            100,
            None,
        );

        if !updated.is_empty() {
            let related: Vec<&ModSpec> = scope_mod_ids
                .iter()
                .filter_map(|id| manifest.mods.iter().find(|m| &m.id == id))
                .collect();
            let lines: Vec<String> = related
                .iter()
                .map(|m| mod_history_line("Update", m))
                .collect();
            finalize_mod_history(
                &manifest_path,
                &mut snapshot,
                "batch-update-all",
                &lines,
                &related,
                &[],
            );
        }

        Ok(serde_json::json!({
            "updated": updated,
            "errors": skipped_errors,
            "skipped": download.skipped,
            "download": download,
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// ── Performance audit ────────────────────────────────────────────────

/// Runs a performance audit on the project configs, checking for common
/// settings that degrade Minecraft performance. Returns a list of
/// recommendations with config file paths and suggested changes.
#[tauri::command(rename_all = "camelCase")]
fn audit_performance(path: String) -> Result<Vec<serde_json::Value>, String> {
    let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
    let project_dir = manifest_parent(&path)?;
    let mut findings = Vec::new();

    // Check Sodium/Embeddium settings for common performance traps
    let config_dir = project_dir.join("config");
    if config_dir.is_dir() {
        // Check sodium-options.json for common issues
        for (filename, check_fn) in SODIUM_CHECKS {
            let fp = config_dir.join(filename);
            if !fp.is_file() {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(&fp) {
                check_fn(&content, &mut findings);
            }
        }
        // Check Forge server config for render distance / spawn limits
        for (pattern, check_fn) in FORGE_PERF_CHECKS {
            if let Ok(entries) = std::fs::read_dir(&config_dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.contains(pattern) {
                        if let Ok(content) = std::fs::read_to_string(entry.path()) {
                            check_fn(&content, &name, &mut findings);
                        }
                    }
                }
            }
        }
    }

    // Check if performance mods are missing (treat forks/ports as covering the base mod).
    let keys = installed_mod_keys(&manifest);
    let perf_mods = [
        "sodium",
        "embeddium",
        "lithium",
        "ferrite-core",
        "immediatelyfast",
        "modernfix",
        "memoryleakfix",
        "smoothboot",
        "entityculling",
        "sodium-extra",
        "c2me",
        "bobby",
        "starlight",
    ];
    let mut missing_perf = Vec::new();
    for pm in perf_mods {
        let aliases = recommendation_aliases(pm);
        let aliases: Vec<&str> = if aliases.is_empty() {
            vec![pm]
        } else {
            aliases
        };
        if !has_installed(&keys, &aliases) {
            missing_perf.push(pm);
        }
    }
    if !missing_perf.is_empty() {
        findings.push(serde_json::json!({
            "severity": if missing_perf.len() >= 3 { "warning" } else { "info" },
            "code": "MISSING_PERFORMANCE_MODS",
            "message": format!("Consider adding performance mods: {}", missing_perf.join(", ")),
            "file": null,
        }));
    }

    // Check JVM args
    let profile = manifest
        .profiles
        .iter()
        .find(|p| p.id == "client")
        .or_else(|| manifest.profiles.first());
    if let Some(profile) = profile {
        // No NO_GC_SETTING finding: empty JVM args are fine — the launcher
        // auto-tunes a GC profile per pack at launch (ZGC on vanilla).
        if profile.memory_mb.unwrap_or(4096) < 3072 {
            findings.push(serde_json::json!({
                "severity": "warning",
                "code": "LOW_MEMORY",
                "message": format!("Memory is set to {} MB — 4-8 GB is recommended for modded Minecraft.", profile.memory_mb.unwrap_or(4096)),
                "file": null,
            }));
        }
    }

    Ok(findings)
}

/// ── Optimize pack (curated + custom) ───────────────────────────────

fn loader_slug_for_manifest(manifest: &ProjectManifest) -> String {
    tuffbox_core::graph::loader_kind_slug(&manifest.loader.kind).to_string()
}

fn is_mod_installed_by_slug(keys: &std::collections::HashSet<String>, slug: &str) -> bool {
    let aliases = tuffbox_core::optimize_pack::recommendation_aliases(slug);
    if aliases.is_empty() {
        has_installed(keys, &[slug])
    } else {
        has_installed(keys, &aliases)
    }
}

fn resolve_opt_mod_modrinth(
    slug: &str,
    name: &str,
    reason: &str,
    mc: &str,
    loader: &str,
) -> Option<OptimizeModOffer> {
    let provider = tuffbox_core::ModrinthProvider::new();
    let project = provider.get_project(slug).ok()?;
    let query = ProviderSearchQuery {
        query: None,
        minecraft_version: Some(mc.to_string()),
        loader: Some(loader.to_string()),
        ..Default::default()
    };
    let versions = provider.get_versions(&project.id, &query).ok()?;
    let version = versions.into_iter().next()?;
    Some(OptimizeModOffer {
        slug: project.slug.clone(),
        name: if project.name.is_empty() {
            name.to_string()
        } else {
            project.name
        },
        provider: "modrinth".into(),
        project_id: project.id,
        version_id: Some(version.id),
        reason: reason.to_string(),
        risk: "low".into(),
        already_installed: false,
    })
}

fn resolve_opt_mod_curseforge(
    slug: &str,
    name: &str,
    reason: &str,
    mc: &str,
    loader: &str,
) -> Option<OptimizeModOffer> {
    let provider = tuffbox_core::CurseForgeProvider::new();
    let loader_type = tuffbox_core::CurseForgeProvider::mod_loader_type(loader);
    let page = provider
        .search_content(
            tuffbox_core::CurseForgeProvider::class_id_for_project_type("mod"),
            name,
            Some(mc),
            loader_type,
            0,
            10,
            Some(2),
            None,
        )
        .ok()?;
    let name_l = name.to_lowercase();
    let slug_compact = slug.replace('-', "");
    let hit = page.hits.into_iter().find(|h| {
        h.slug.eq_ignore_ascii_case(slug)
            || h.name.to_lowercase().contains(&name_l)
            || h.slug.to_lowercase().contains(&slug_compact)
    })?;
    Some(OptimizeModOffer {
        slug: hit.slug.clone(),
        name: hit.name.clone(),
        provider: "curseforge".into(),
        project_id: hit.id.to_string(),
        version_id: None,
        reason: reason.to_string(),
        risk: "medium".into(),
        already_installed: false,
    })
}

#[tauri::command(rename_all = "camelCase")]
fn list_curated_optimize_packs(path: String) -> Result<serde_json::Value, String> {
    let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
    let loader = loader_slug_for_manifest(&manifest);
    let mc = manifest.minecraft.version.clone();
    let current = tuffbox_core::optimize_pack::curated_pack_for(&loader, &mc);
    let entries = tuffbox_core::optimize_pack::list_curated_pack_entries(&loader);
    let mapped_versions: Vec<String> = entries.iter().map(|(ver, _)| ver.clone()).collect();
    let unavailable_message = if current.is_some() {
        None
    } else if mapped_versions.is_empty() {
        Some(format!(
            "No curated optimize pack published for {loader} yet. Use Custom for performance mods."
        ))
    } else {
        Some(format!(
            "No curated pack for {loader} · MC {mc}. Mapped versions: {}. Use Custom for this instance.",
            mapped_versions.join(", ")
        ))
    };
    Ok(serde_json::json!({
        "loader": loader,
        "minecraftVersion": mc,
        "available": current.is_some(),
        "current": current,
        "unavailableMessage": unavailable_message,
        "entries": entries.into_iter().map(|(ver, r)| serde_json::json!({
            "minecraftVersion": ver,
            "projectId": r.project_id,
            "slug": r.slug,
            "name": r.name,
        })).collect::<Vec<_>>(),
    }))
}

#[tauri::command(rename_all = "camelCase")]
async fn preview_curated_optimize_pack(path: String) -> Result<serde_json::Value, String> {
    tokio::task::spawn_blocking(move || {
        let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
        let project_dir = manifest_parent(&path)?;
        let loader = loader_slug_for_manifest(&manifest);
        let mc = manifest.minecraft.version.clone();
        let curated = tuffbox_core::optimize_pack::curated_pack_for(&loader, &mc).ok_or_else(|| {
            format!("No curated optimize pack for {loader} {mc}. Use Custom mode or publish a pack and update optimize-packs.json.")
        })?;
        let id = curated
            .slug
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| curated.project_id.clone());
        let provider = tuffbox_core::ModrinthProvider::new();
        let project = provider.get_project(&id).map_err(|e| {
            format!(
                "Curated pack '{id}' not found on Modrinth yet ({e}). Publish the project or fix optimize-packs.json."
            )
        })?;
        // A mapping that points at a *modpack* (e.g. Fabulously Optimized)
        // cannot be installed as a single mod — its versions carry the set in
        // modrinth.index.json, not in dependencies. Redirect to the FO tab
        // instead of downloading a stray .mrpack into mods/.
        if project.project_type.eq_ignore_ascii_case("modpack") {
            return Err(format!(
                "Curated entry '{}' is a Modrinth modpack, not a single mod — open the Fabulously Optimized tab to install its mod set.",
                project.slug
            ));
        }
        let query = ProviderSearchQuery {
            query: None,
            minecraft_version: Some(mc.clone()),
            loader: Some(loader.clone()),
            ..Default::default()
        };
        let versions = provider
            .get_versions(&project.id, &query)
            .map_err(|e| e.to_string())?;
        let version = versions.first().ok_or_else(|| {
            format!("No Modrinth version of '{}' for {mc}/{loader}", project.slug)
        })?;
        let deps = provider
            .resolve_dependencies(&version.id)
            .unwrap_or_default();
        let keys = installed_mod_keys(&manifest);
        let mut mods = Vec::new();
        mods.push(serde_json::json!({
            "slug": project.slug,
            "name": project.name,
            "projectId": project.id,
            "alreadyInstalled": is_mod_installed_by_slug(&keys, &project.slug),
            "role": "root",
        }));
        for dep in deps {
            let role = match dep.kind {
                tuffbox_core::manifest::DependencyKind::Requires => "requires",
                tuffbox_core::manifest::DependencyKind::Optional => "optional",
                tuffbox_core::manifest::DependencyKind::Conflicts => "conflicts",
                tuffbox_core::manifest::DependencyKind::BreaksWith => "breaks_with",
                tuffbox_core::manifest::DependencyKind::Replaces => "replaces",
            };
            if role == "conflicts" || role == "breaks_with" {
                continue;
            }
            let slug = dep.target.clone();
            mods.push(serde_json::json!({
                "slug": slug,
                "name": dep.target,
                "projectId": dep.target,
                "alreadyInstalled": is_mod_installed_by_slug(&keys, &slug),
                "role": role,
            }));
        }
        let (cfg_actions, warnings) = tuffbox_core::optimize_pack::build_optimize_config_actions(
            &project_dir,
            &manifest,
            true,
        );
        let pack_name = curated
            .name
            .clone()
            .unwrap_or_else(|| project.name.clone());
        Ok(serde_json::json!({
            "pack": {
                "projectId": project.id,
                "slug": project.slug,
                "name": pack_name,
                "versionId": version.id,
                "versionNumber": version.version_number,
            },
            "mods": mods,
            "configActions": cfg_actions,
            "warnings": warnings,
            "minecraftVersion": mc,
            "loader": loader,
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Install curated pack root + required deps (skips already installed via Modrinth resolver).
#[tauri::command(rename_all = "camelCase")]
async fn install_curated_optimize_pack(
    app: tauri::AppHandle,
    path: String,
    apply_configs: bool,
    config_plan: Option<tuffbox_core::action_plan::ActionPlan>,
) -> Result<serde_json::Value, String> {
    let preview = preview_curated_optimize_pack(path.clone()).await?;
    let pack = preview
        .get("pack")
        .cloned()
        .ok_or_else(|| "preview missing pack".to_string())?;
    let root_id = pack
        .get("projectId")
        .or_else(|| pack.get("slug"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "pack missing projectId".to_string())?
        .to_string();

    let install = match add_modrinth_mod_with_dependencies(
        app.clone(),
        path.clone(),
        root_id.clone(),
        "both".into(),
        None,
    )
    .await
    {
        Ok(v) => serde_json::json!({ "ok": true, "installed": v }),
        Err(e) => serde_json::json!({ "ok": false, "error": e }),
    };

    let mut config_result = serde_json::json!(null);
    if apply_configs {
        let plan = if let Some(plan) = config_plan {
            plan
        } else if let Some(actions) = preview.get("configActions").cloned() {
            tuffbox_core::optimize_pack::config_actions_to_plan(
                serde_json::from_value(actions).unwrap_or_default(),
                "Optimize pack (curated) config templates",
            )
        } else {
            tuffbox_core::optimize_pack::config_actions_to_plan(
                Vec::new(),
                "Optimize pack (curated) config templates",
            )
        };
        if !plan.actions.is_empty() {
            config_result = apply_action_plan(
                app,
                path,
                plan,
                Some(format!("optimize-curated-{root_id}")),
            )
            .await?;
        }
    }

    Ok(serde_json::json!({
        "install": install,
        "config": config_result,
        "pack": pack,
    }))
}

/// ── Optimize pack (Fabulously Optimized set) ───────────────────────
///
/// Unlike curated/custom, the FO variant downloads the official FO `.mrpack`
/// for the instance's exact MC version + loader, parses its
/// `modrinth.index.json` and installs the **exact pinned builds** — the set
/// the FO team tested together, not latest-per-mod.

/// Numeric `1.20.1`-style ordering, newest first. Local copy: the one in
/// `optimize_pack` is private and sorts ascending.
fn sort_mc_versions_desc(versions: &mut [String]) {
    fn parts(v: &str) -> Vec<u32> {
        v.split('.').map(|p| p.parse().unwrap_or(0)).collect()
    }
    versions.sort_by(|a, b| parts(b).cmp(&parts(a)));
}

#[tauri::command(rename_all = "camelCase")]
async fn preview_fo_optimize_pack(path: String) -> Result<serde_json::Value, String> {
    tokio::task::spawn_blocking(move || {
        let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
        let project_dir = manifest_parent(&path)?;
        let loader = loader_slug_for_manifest(&manifest);
        let mc = manifest.minecraft.version.clone();
        if loader != "fabric" && loader != "quilt" {
            return Err(format!(
                "The Fabulously Optimized set needs Fabric or Quilt (this instance uses {loader}). Use Custom mode for other loaders."
            ));
        }
        // Exact MC match against the curated mapping (the FO project id lives
        // there, shared with the curated flow).
        let entries = tuffbox_core::optimize_pack::list_curated_pack_entries(&loader);
        let fo_ref = entries
            .iter()
            .find(|(ver, _)| ver == &mc)
            .map(|(_, r)| r.clone());
        let Some(fo_ref) = fo_ref else {
            let mut supported: Vec<String> = entries
                .iter()
                .map(|(ver, _)| ver.clone())
                .filter(|v| v != "default")
                .collect();
            sort_mc_versions_desc(&mut supported);
            let supported = if supported.is_empty() {
                "none mapped yet".to_string()
            } else {
                supported.join(", ")
            };
            return Err(format!(
                "No Fabulously Optimized set for MC {mc}. FO supports: {supported}."
            ));
        };
        let fo_id = fo_ref
            .slug
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| fo_ref.project_id.clone());
        let provider = tuffbox_core::ModrinthProvider::new();
        let fo_project = provider.get_project(&fo_id).map_err(|e| e.to_string())?;
        if !fo_project.project_type.eq_ignore_ascii_case("modpack") {
            return Err(format!(
                "Mapped FO project '{fo_id}' is not a Modrinth modpack (type: {}).",
                fo_project.project_type
            ));
        }
        let mut warnings: Vec<String> = Vec::new();
        let query = ProviderSearchQuery {
            query: None,
            minecraft_version: Some(mc.clone()),
            loader: Some(loader.clone()),
            ..Default::default()
        };
        let mut versions = provider
            .get_versions(&fo_project.id, &query)
            .map_err(|e| e.to_string())?;
        // FO publishes Fabric-tagged builds; Quilt runs Fabric mods, so fall
        // back to the Fabric build when no Quilt-tagged one exists.
        if versions.is_empty() && loader == "quilt" {
            let fabric_query = ProviderSearchQuery {
                query: None,
                minecraft_version: Some(mc.clone()),
                loader: Some("fabric".to_string()),
                ..Default::default()
            };
            versions = provider
                .get_versions(&fo_project.id, &fabric_query)
                .map_err(|e| e.to_string())?;
            if !versions.is_empty() {
                warnings.push(
                    "No Quilt-tagged FO build — using the Fabric FO build (Quilt runs Fabric mods)."
                        .to_string(),
                );
            }
        }
        let fo_version = versions.into_iter().next().ok_or_else(|| {
            let mut supported: Vec<String> = provider
                .get_versions(&fo_project.id, &ProviderSearchQuery::default())
                .map(|vs| {
                    vs.into_iter()
                        .flat_map(|v| v.game_versions)
                        .collect::<std::collections::HashSet<_>>()
                        .into_iter()
                        .collect()
                })
                .unwrap_or_default();
            sort_mc_versions_desc(&mut supported);
            supported.truncate(12);
            let supported = if supported.is_empty() {
                "none published".to_string()
            } else {
                supported.join(", ")
            };
            format!("No Fabulously Optimized build for MC {mc} ({loader}). FO supports: {supported}.")
        })?;

        let mrpack = fo_version
            .files
            .iter()
            .find(|f| f.filename.to_lowercase().ends_with(".mrpack"))
            .or_else(|| fo_version.files.iter().find(|f| f.primary))
            .ok_or_else(|| {
                format!(
                    "FO version {} has no downloadable pack file.",
                    fo_version.version_number
                )
            })?;
        let pack_bytes = tuffbox_core::http::get_bytes(&mrpack.url)
            .map_err(|e| format!("Failed to download the FO pack file: {e}"))?;
        let index_json = tuffbox_core::importer::read_mrpack_index_json(&pack_bytes)
            .map_err(|e| e.to_string())?;
        let (mut fo_mods, stats) =
            tuffbox_core::fo_pack::fo_mods_from_index_json(&index_json).map_err(|e| e.to_string())?;
        if fo_mods.is_empty() {
            return Err("The FO pack index contains no client mods.".to_string());
        }
        // SHA-1 fallback for entries whose download URL didn't parse.
        for fo_mod in fo_mods
            .iter_mut()
            .filter(|m| m.project_id.is_empty() && !m.sha1.is_empty())
        {
            if let Ok(Some(version)) = provider.get_version_by_hash(&fo_mod.sha1) {
                fo_mod.project_id = version.project_id;
                fo_mod.version_id = version.id;
            }
        }
        let unresolved = fo_mods
            .iter()
            .filter(|m| m.project_id.is_empty() || m.version_id.is_empty())
            .count();
        if unresolved > 0 {
            warnings.push(format!(
                "{unresolved} FO entries could not be resolved to a Modrinth version and were skipped."
            ));
        }
        let mut ids: Vec<String> = fo_mods
            .iter()
            .filter(|m| !m.project_id.is_empty() && !m.version_id.is_empty())
            .map(|m| m.project_id.clone())
            .collect();
        ids.sort();
        ids.dedup();
        let infos = provider.get_projects_batch(&ids).map_err(|e| e.to_string())?;
        // Warm the project cache so the install step's per-mod lookups are instant.
        for info in &infos {
            tuffbox_core::api_cache::put(
                tuffbox_core::api_cache::project_key("modrinth", &info.id),
                info.clone(),
            );
            tuffbox_core::api_cache::put(
                tuffbox_core::api_cache::project_key("modrinth", &info.slug),
                info.clone(),
            );
        }
        let by_id: std::collections::HashMap<&str, &tuffbox_core::ProjectInfo> = infos
            .iter()
            .map(|p| (p.id.as_str(), p))
            .collect();
        let keys = installed_mod_keys(&manifest);
        let reason = format!(
            "Fabulously Optimized {} · exact pinned build",
            fo_version.version_number
        );
        let mut mods: Vec<serde_json::Value> = fo_mods
            .iter()
            .filter(|m| !m.project_id.is_empty() && !m.version_id.is_empty())
            .map(|m| {
                let (slug, name) = match by_id.get(m.project_id.as_str()) {
                    Some(p) => (p.slug.clone(), p.name.clone()),
                    None => (
                        m.file_name
                            .trim_end_matches(".jar")
                            .trim_end_matches(".JAR")
                            .to_lowercase()
                            .replace(['_', ' ', '+'], "-"),
                        tuffbox_core::fo_pack::pretty_name_from_file_name(&m.file_name),
                    ),
                };
                let already = keys.contains(&m.project_id.to_lowercase())
                    || is_mod_installed_by_slug(&keys, &slug);
                serde_json::json!({
                    "slug": slug,
                    "name": name,
                    "fileName": m.file_name,
                    "provider": "modrinth",
                    "projectId": m.project_id,
                    "versionId": m.version_id,
                    "reason": reason,
                    "risk": "low",
                    "alreadyInstalled": already,
                })
            })
            .collect();
        // Not-installed first so the set reads as a to-do list, then A–Z.
        mods.sort_by(|a, b| {
            let a_done = a
                .get("alreadyInstalled")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let b_done = b
                .get("alreadyInstalled")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            a_done.cmp(&b_done).then_with(|| {
                a.get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .cmp(b.get("name").and_then(|v| v.as_str()).unwrap_or(""))
            })
        });
        if stats.skipped_non_mod > 0 {
            warnings.push(format!(
                "Skipped {} non-mod FO files (resource packs, configs).",
                stats.skipped_non_mod
            ));
        }
        if stats.skipped_client_unsupported > 0 {
            warnings.push(format!(
                "Skipped {} server-only FO entries.",
                stats.skipped_client_unsupported
            ));
        }
        let (cfg_actions, mut cfg_warnings) =
            tuffbox_core::optimize_pack::build_optimize_config_actions(
                &project_dir,
                &manifest,
                true,
            );
        warnings.append(&mut cfg_warnings);
        let pack_name = fo_ref.name.clone().unwrap_or_else(|| fo_project.name.clone());
        Ok(serde_json::json!({
            "pack": {
                "projectId": fo_project.id,
                "slug": fo_project.slug,
                "name": pack_name,
                "versionId": fo_version.id,
                "versionNumber": fo_version.version_number,
                "minecraftVersion": mc,
                "loader": loader,
                "modCount": mods.len(),
            },
            "mods": mods,
            "configActions": cfg_actions,
            "warnings": warnings,
            "minecraftVersion": mc,
            "loader": loader,
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Installs the selected FO pinned builds (one snapshot + one download batch
/// for the whole set), then fills any missing required dependencies.
#[tauri::command(rename_all = "camelCase")]
async fn install_fo_optimize_pack(
    app: tauri::AppHandle,
    path: String,
    mods: Vec<OptimizeModOffer>,
    apply_configs: bool,
    config_plan: Option<tuffbox_core::action_plan::ActionPlan>,
) -> Result<serde_json::Value, String> {
    let path_for_stats = path.clone();
    // The blocking section moves its own clones: `app` + `path` are still
    // needed afterwards for the config-plan apply.
    let path_for_block = path.clone();
    let app_for_block = app.clone();
    let (installed, mut errors) = tokio::task::spawn_blocking(move || {
        let manifest_path = PathBuf::from(&path_for_block);
        let mut snapshot =
            auto_snapshot_before_mod_op(&manifest_path, "optimize-fo").map_err(|e| e.to_string())?;
        let mut manifest = ProjectManifest::load_from_path(&path_for_block).map_err(|e| e.to_string())?;
        let mut installed: Vec<String> = Vec::new();
        let mut errors: Vec<String> = Vec::new();
        let mut requested: Vec<String> = Vec::new();
        for offer in &mods {
            if offer.already_installed {
                continue;
            }
            if offer.provider != "modrinth" {
                errors.push(format!("{}: only Modrinth entries are supported", offer.slug));
                continue;
            }
            let Some(version_id) = offer
                .version_id
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
            else {
                errors.push(format!("{}: missing pinned version", offer.slug));
                continue;
            };
            if manifest_has_dependency_target(&manifest, &offer.project_id)
                || manifest.mods.iter().any(|m| m.id == offer.slug)
            {
                continue;
            }
            requested.push(offer.project_id.clone());
            match add_mod_from_modrinth_pinned(
                &mut manifest,
                &offer.project_id,
                version_id,
                Some("both".to_string()),
            ) {
                Ok(()) => installed.push(offer.project_id.clone()),
                Err(e) => {
                    let msg = e.to_string();
                    if msg.contains("already in the project") {
                        continue;
                    }
                    errors.push(format!("{}: {msg}", offer.slug));
                }
            }
        }
        // Empty seed: only fills Requires-deps the set itself recorded
        // (FO mods deselected by the user resolve to latest here).
        match install_modrinth_with_dependencies_rounds(&mut manifest, &[], "both", 50, None) {
            Ok(deps) => installed.extend(deps),
            Err(e) => errors.push(format!("dependencies: {e}")),
        }
        save_manifest(&manifest_path, &manifest).map_err(|e| e.to_string())?;
        download_project_mods_tracked(&app_for_block, &manifest_path, &manifest, None, true);
        let related: Vec<&ModSpec> = installed
            .iter()
            .filter_map(|id| {
                manifest.mods.iter().find(|m| {
                    m.id == *id || m.source.project_id.as_deref() == Some(id.as_str())
                })
            })
            .collect();
        let lines = mod_install_history_lines(&related, &requested);
        finalize_mod_history(
            &manifest_path,
            &mut snapshot,
            "optimize-fo",
            &lines,
            &related,
            &[],
        );
        Ok::<(Vec<String>, Vec<String>), String>((installed, errors))
    })
    .await
    .map_err(|e| e.to_string())??;

    let mut config_result = serde_json::json!(null);
    if apply_configs {
        if let Some(plan) = config_plan {
            if !plan.actions.is_empty() {
                match apply_action_plan(app, path, plan, Some("optimize-fo".into())).await {
                    Ok(v) => config_result = v,
                    Err(e) => errors.push(format!("configs: {e}")),
                }
            }
        }
    }
    swarm_api::spawn_pack_cooccurrence(path_for_stats, "mod_install");

    Ok(serde_json::json!({
        "installed": installed,
        "errors": errors,
        "config": config_result,
        "ok": errors.is_empty(),
    }))
}

#[tauri::command(rename_all = "camelCase")]
async fn build_optimize_plan(
    app: tauri::AppHandle,
    path: String,
    use_ai_configs: bool,
) -> Result<serde_json::Value, String> {
    let path_for_ai = path.clone();
    let manifest_preview = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
    let loader_preview = loader_slug_for_manifest(&manifest_preview);
    let mc_preview = manifest_preview.minecraft.version.clone();

    // Prefer Supabase FO catalog (Fabric/Quilt). Forge/Neo empty until custom packs land.
    let mut catalog_source = "bundled";
    let remote_candidates = match (
        integrations::swarm_supabase_url(),
        integrations::swarm_supabase_anon_key(),
    ) {
        (Some(url), Some(key)) => {
            match tuffbox_core::swarm_supabase::optimize_mods_for_supabase(
                &url,
                &key,
                &loader_preview,
                &mc_preview,
            )
            .await
            {
                Ok(rows) if !rows.is_empty() => {
                    catalog_source = "supabase";
                    Some(tuffbox_core::swarm_supabase::optimize_rows_to_candidates(
                        &rows,
                    ))
                }
                Ok(_) => None,
                Err(err) => {
                    eprintln!("optimize_mods_for supabase: {err}");
                    None
                }
            }
        }
        _ => None,
    };

    let mut base = tokio::task::spawn_blocking(move || {
        let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
        let project_dir = manifest_parent(&path)?;
        let loader = loader_slug_for_manifest(&manifest);
        let mc = manifest.minecraft.version.clone();
        let keys = installed_mod_keys(&manifest);
        let candidates = remote_candidates.unwrap_or_else(|| {
            tuffbox_core::optimize_pack::optimization_candidates(&loader, &mc)
        });

        let mut offers = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for candidate in candidates {
            let modrinth_slug = candidate
                .modrinth_slug
                .as_deref()
                .unwrap_or(&candidate.slug);
            let aliases = tuffbox_core::optimize_pack::aliases_for_candidate(modrinth_slug);
            let alias_refs: Vec<&str> = aliases.iter().map(|s| s.as_str()).collect();
            if has_installed(&keys, &alias_refs) {
                continue;
            }
            if !seen.insert(candidate.slug.clone()) {
                continue;
            }
            if let Some(mut offer) =
                resolve_opt_mod_modrinth(modrinth_slug, &candidate.name, &candidate.reason, &mc, &loader)
            {
                offer.risk = candidate.risk.clone();
                offer.already_installed = false;
                seen.insert(offer.slug.clone());
                offers.push(offer);
                continue;
            }
            if let Some(cf_slug) = candidate.curseforge_slug.as_deref() {
                if let Some(mut offer) =
                    resolve_opt_mod_curseforge(cf_slug, &candidate.name, &candidate.reason, &mc, &loader)
                {
                    offer.risk = candidate.risk.clone();
                    offer.already_installed = false;
                    seen.insert(offer.slug.clone());
                    offers.push(offer);
                }
            }
        }

        let tokens = tuffbox_core::optimize_pack::inventory_tokens(&manifest);
        let deny = tuffbox_core::optimize_pack::modernfix_denylist_hit(&tokens);
        let (cfg_actions, warnings) =
            tuffbox_core::optimize_pack::build_optimize_config_actions(
                &project_dir,
                &manifest,
                deny.is_empty(),
            );

        let findings = audit_performance(path)?;
        let plan = tuffbox_core::optimize_pack::config_actions_to_plan(
            cfg_actions,
            "Optimize pack custom: safe client/performance config patches",
        );

        Ok::<_, String>(serde_json::json!({
            "mode": "custom",
            "mods": offers,
            "plan": plan,
            "findings": findings,
            "warnings": warnings,
            "minecraftVersion": mc,
            "loader": loader,
            "catalogSource": catalog_source,
            "curatedAvailable": tuffbox_core::optimize_pack::curated_pack_for(&loader, &mc).is_some(),
        }))
    })
    .await
    .map_err(|e| e.to_string())??;

    if use_ai_configs {
        match tune_config_api::tune_config_advise(
            app,
            path_for_ai,
            Some("fps_client".into()),
            Some("Optimize pack for client FPS using safe config patches.".into()),
            None,
            None,
            Some("optimize-pack".into()),
            None,
        )
        .await
        {
            Ok(advise) => {
                let mut warnings = base
                    .get("warnings")
                    .and_then(|w| w.as_array())
                    .cloned()
                    .unwrap_or_default();
                warnings.push(serde_json::json!(
                    "AI Config Advisor refined performance patches — review before apply."
                ));
                for w in advise.validation_warnings {
                    warnings.push(serde_json::json!(w));
                }
                if !advise.validation_ok {
                    for e in &advise.validation_errors {
                        warnings.push(serde_json::json!(format!("AI plan warning: {e}")));
                    }
                } else {
                    base["plan"] = serde_json::to_value(&advise.plan).unwrap_or(base["plan"].clone());
                }
                base["warnings"] = serde_json::Value::Array(warnings);
                base["aiResearchLog"] = serde_json::to_value(&advise.research_log).unwrap_or_default();
                base["aiDiffs"] = serde_json::to_value(&advise.diffs).unwrap_or_default();
            }
            Err(e) => {
                let mut warnings = base
                    .get("warnings")
                    .and_then(|w| w.as_array())
                    .cloned()
                    .unwrap_or_default();
                warnings.push(serde_json::json!(format!(
                    "AI config refine failed — using deterministic templates only ({e})"
                )));
                base["warnings"] = serde_json::Value::Array(warnings);
            }
        }
    }

    Ok(base)
}

#[tauri::command(rename_all = "camelCase")]
async fn apply_optimize_custom_plan(
    app: tauri::AppHandle,
    path: String,
    mods: Vec<OptimizeModOffer>,
    apply_configs: bool,
    config_plan: Option<tuffbox_core::action_plan::ActionPlan>,
) -> Result<serde_json::Value, String> {
    let mut installed = Vec::new();
    let mut errors = Vec::new();

    for offer in mods {
        if offer.already_installed {
            continue;
        }
        if offer.provider == "modrinth" {
            match add_modrinth_mod_with_dependencies(
                app.clone(),
                path.clone(),
                offer.project_id.clone(),
                "both".into(),
                None,
            )
            .await
            {
                Ok(msgs) => installed.extend(msgs),
                Err(e) => errors.push(format!("{}: {e}", offer.slug)),
            }
        } else if offer.provider == "curseforge" {
            match add_curseforge_mod(
                app.clone(),
                path.clone(),
                offer.project_id.clone(),
                "both".into(),
            )
            .await
            {
                Ok(()) => installed.push(format!("Installed {} (curseforge)", offer.slug)),
                Err(e) => errors.push(format!("{} (CF): {e}", offer.slug)),
            }
        }
    }

    let mut config_result = serde_json::json!(null);
    if apply_configs {
        if let Some(plan) = config_plan {
            if !plan.actions.is_empty() {
                match apply_action_plan(app, path, plan, Some("optimize-custom".into())).await {
                    Ok(v) => config_result = v,
                    Err(e) => errors.push(format!("configs: {e}")),
                }
            }
        }
    }

    Ok(serde_json::json!({
        "installed": installed,
        "errors": errors,
        "config": config_result,
        "ok": errors.is_empty(),
    }))
}

// ── Mod presets (Optimize → user-built mod sets) ────────────────────

#[tauri::command(rename_all = "camelCase")]
fn get_mod_presets() -> Result<mod_presets::ModPresetsStore, String> {
    Ok(mod_presets::load_mod_presets())
}

#[tauri::command(rename_all = "camelCase")]
fn save_mod_presets_cmd(store: mod_presets::ModPresetsStore) -> Result<(), String> {
    mod_presets::save_mod_presets(&store)
}

/// Resolve one preset entry against the current instance's MC version + loader.
/// Returns the OptimizeModOffer shape used by apply_optimize_custom_plan.
#[tauri::command(rename_all = "camelCase")]
async fn resolve_preset_mod(
    path: String,
    entry: mod_presets::PresetModEntry,
) -> Result<serde_json::Value, String> {
    let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
    let loader = loader_slug_for_manifest(&manifest);
    let mc = manifest.minecraft.version.clone();
    let keys = installed_mod_keys(&manifest);
    let offer = match entry.provider.as_str() {
        "curseforge" => {
            resolve_opt_mod_curseforge(
                &entry.project_id,
                &entry.name,
                "Preset mod",
                &mc,
                &loader,
            )
            .ok_or_else(|| format!("'{}' not found on CurseForge", entry.name))?
        }
        _ => resolve_opt_mod_modrinth(
            &entry.project_id,
            &entry.name,
            "Preset mod",
            &mc,
            &loader,
        )
        .ok_or_else(|| format!("'{}' has no version for {mc}/{loader}", entry.name))?,
    };
    let aliases = tuffbox_core::optimize_pack::aliases_for_candidate(&offer.slug);
    let alias_refs: Vec<&str> = aliases.iter().map(|s| s.as_str()).collect();
    let already = has_installed(&keys, &alias_refs);
    Ok(serde_json::json!({
        "slug": offer.slug,
        "name": offer.name,
        "provider": offer.provider,
        "projectId": offer.project_id,
        "versionId": offer.version_id,
        "reason": offer.reason,
        "risk": offer.risk,
        "alreadyInstalled": already,
    }))
}

/// ── Shared options.txt sync (docs/17, Part 2) ─────────────────────────

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct OptionsSyncStatus {
   managed: bool,
   group_id: String,
   mc_version: String,
   has_local: bool,
   has_group_template: bool,
   local_path: String,
   group_template_path: String,
}

#[tauri::command(rename_all = "camelCase")]
fn options_sync_status(
   path: String,
) -> Result<OptionsSyncStatus, String> {
   let manifest_path = resolve_manifest_path(&path)?;
   let manifest = ProjectManifest::load_from_path(&manifest_path)
       .map_err(|e| e.to_string())?;
   let project_dir = manifest_path
       .parent()
       .ok_or("manifest has no parent directory")?;
   let mc = manifest.minecraft.version.clone();
   let group = tuffbox_core::options_sync::version_group(&mc);
   let local = project_dir.join("options.txt");
   Ok(OptionsSyncStatus {
       managed: tuffbox_core::options_sync::is_managed(project_dir),
       group_id: group.clone(),
       mc_version: mc,
       has_local: local.is_file(),
       has_group_template: tuffbox_core::options_sync::read_group_template(&group).is_some(),
       local_path: local.to_string_lossy().into_owned(),
       group_template_path: tuffbox_core::options_sync::group_template_path(&group)
           .to_string_lossy()
           .into_owned(),
   })
}

/// Enable shared-options management for a project. `import_local` = seed the
/// group template from this project's current options.txt when the group is
/// empty (UI asks via dialog when the group already has one).
#[tauri::command(rename_all = "camelCase")]
fn options_sync_enable(path: String) -> Result<bool, String> {
   let manifest_path = resolve_manifest_path(&path)?;
   let manifest = ProjectManifest::load_from_path(&manifest_path)
       .map_err(|e| e.to_string())?;
   let project_dir = manifest_path
       .parent()
       .ok_or("manifest has no parent directory")?;
   tuffbox_core::options_sync::enable_management(project_dir, &manifest.minecraft.version)
       .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
fn options_sync_disable(path: String) -> Result<(), String> {
   let manifest_path = resolve_manifest_path(&path)?;
   let project_dir = manifest_path
       .parent()
       .ok_or("manifest has no parent directory")?;
   tuffbox_core::options_sync::disable_management(project_dir);
   Ok(())
}

/// Push the project's current options.txt into its version group.
#[tauri::command(rename_all = "camelCase")]
fn options_sync_push(path: String) -> Result<(), String> {
   let manifest_path = resolve_manifest_path(&path)?;
   let manifest = ProjectManifest::load_from_path(&manifest_path)
       .map_err(|e| e.to_string())?;
   let project_dir = manifest_path
       .parent()
       .ok_or("manifest has no parent directory")?;
   tuffbox_core::options_sync::push_to_group(project_dir, &manifest.minecraft.version)
}

#[tauri::command(rename_all = "camelCase")]
fn options_sync_groups() -> Result<Vec<tuffbox_core::options_sync::GroupInfo>, String> {
    Ok(tuffbox_core::options_sync::list_groups())
}

/// ── Store maintenance: retro-dedup + stats (docs/17, M3) ──────────────

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StoreStats {
    object_count: u64,
    store_bytes: u64,
}

/// Approximate store size: object count + total bytes on disk.
#[tauri::command(rename_all = "camelCase")]
fn store_stats() -> Result<StoreStats, String> {
    let root = tuffbox_core::mod_store::store_root().join("objects");
    let mut count = 0u64;
    let mut bytes = 0u64;
    if root.is_dir() {
        for prefix in std::fs::read_dir(&root).map_err(|e| e.to_string())?.flatten() {
            let p = prefix.path();
            if !p.is_dir() {
                continue;
            }
            for obj in std::fs::read_dir(&p).map_err(|e| e.to_string())?.flatten() {
                if let Ok(meta) = obj.metadata() {
                    if meta.is_file() {
                        count += 1;
                        bytes += meta.len();
                    }
                }
            }
        }
    }
    Ok(StoreStats { object_count: count, store_bytes: bytes })
}

/// Retroactive deduplication over the user's known projects (recent list +
/// any extra dirs the UI passes). Long-running: call from a background task.
#[tauri::command(rename_all = "camelCase")]
async fn store_retro_dedup(extra_paths: Option<Vec<String>>) -> Result<serde_json::Value, String> {
    // Collect project roots: recent projects list (deduped, existing only).
    let mut roots: Vec<PathBuf> = Vec::new();
    for entry in helpers::load_recent_projects() {
        let path = PathBuf::from(&entry.path);
        if path.is_dir() && !roots.contains(&path) {
            roots.push(path);
        }
    }
    for p in extra_paths.unwrap_or_default() {
        let path = PathBuf::from(&p);
        if path.is_dir() && !roots.contains(&path) {
            roots.push(path);
        }
    }

    // The sweep hashes potentially gigabytes — run it off the main thread.
    let report = tokio::task::spawn_blocking(move || {
        let root_refs: Vec<std::path::PathBuf> = roots;
        let refs: Vec<&std::path::Path> = root_refs.iter().map(|p| p.as_path()).collect();
        tuffbox_core::mod_store::retro_dedup(&refs)
    })
    .await
    .map_err(|e| format!("dedup task panicked: {e}"))?;

    Ok(serde_json::to_value(&report).map_err(|e| e.to_string())?)
}

/// Run store GC (remove objects no longer referenced by any instance).
#[tauri::command(rename_all = "camelCase")]
fn store_gc() -> Result<serde_json::Value, String> {
    let (removed, bytes) = tuffbox_core::mod_store::gc().map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "removed": removed, "bytesReclaimed": bytes }))
}

/// ── Personal YouTube feed (docs: home feed sources) ──────────────────
///
/// The default home feed is a curated table. These commands let the player
/// wire the feed to THEIR YouTube instead: channels they follow are polled
/// through the public RSS hub (no key, no login) and their latest videos
/// replace the curated picks. Fetching happens Rust-side: the RSS endpoint
/// sends no CORS headers, so the webview can't reach it directly.

/// Inner text of the first `<tag …>…</tag>` in `xml` (attributes tolerated
/// on the opening tag). Minimal parser — the RSS hub format is stable.
fn yt_xml_tag(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}");
    let start = xml.find(&open)?;
    let after = xml[start..].find('>')? + start + 1;
    let close = format!("</{tag}>");
    let end = xml[after..].find(&close)? + after;
    Some(xml[after..end].trim().to_string())
}

/// Value of `attr="…"` inside the first `<tag …` occurrence.
fn yt_xml_attr(xml: &str, tag: &str, attr: &str) -> Option<String> {
    let open = format!("<{tag}");
    let start = xml.find(&open)?;
    let seg_end = xml[start..].find('>')? + start;
    let seg = &xml[start..seg_end];
    let key = format!("{attr}=\"");
    let a = seg.find(&key)? + key.len();
    let b = seg[a..].find('"')? + a;
    Some(seg[a..b].to_string())
}

/// Decode the handful of entities YouTube puts in RSS titles.
fn yt_unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

async fn yt_fetch_text(url: &str) -> Result<String, String> {
    let parsed = reqwest::Url::parse(url).map_err(|e| e.to_string())?;
    if parsed.scheme() != "https" || parsed.host_str() != Some("www.youtube.com") {
        return Err("only www.youtube.com is allowed".into());
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(parsed)
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) TuffBox")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let text = resp.text().await.map_err(|e| e.to_string())?;
    Ok(text)
}

/// Resolve a user-typed channel reference (UC id, channel URL, @handle,
/// /c/ or /user/ link) to a channel id by reading the public channel page.
#[tauri::command(rename_all = "camelCase")]
async fn youtube_my_feed_lookup(query: String) -> Result<serde_json::Value, String> {
    let q = query.trim();
    if q.is_empty() {
        return Err("Enter a channel link, @handle, or channel ID.".into());
    }
    // Direct channel id.
    let direct = q
        .strip_prefix("UC")
        .filter(|r| r.len() >= 20 && r.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'));
    if direct.is_some() {
        let id = q.to_string();
        let feed = yt_fetch_text(&format!(
            "https://www.youtube.com/feeds/videos.xml?channel_id={id}"
        ))
        .await?;
        let label = yt_xml_tag(&feed, "title").unwrap_or_else(|| id.clone());
        return Ok(serde_json::json!({ "channelId": id, "label": yt_unescape(&label) }));
    }
    let page_url = if q.starts_with("http") {
        // URL forms: ?channel_id=…, /channel/UC…, or a handle page.
        if let Some(id) = q
            .split("channel_id=")
            .nth(1)
            .and_then(|r| r.split('&').next())
            .filter(|r| r.starts_with("UC") && r.len() > 20)
        {
            let feed = yt_fetch_text(&format!(
                "https://www.youtube.com/feeds/videos.xml?channel_id={id}"
            ))
            .await?;
            let label = yt_xml_tag(&feed, "title").unwrap_or_else(|| id.to_string());
            return Ok(serde_json::json!({ "channelId": id, "label": yt_unescape(&label) }));
        }
        if let Some(id) = q
            .split("/channel/")
            .nth(1)
            .and_then(|r| r.split('/').next())
            .filter(|r| r.starts_with("UC") && r.len() > 20)
        {
            let feed = yt_fetch_text(&format!(
                "https://www.youtube.com/feeds/videos.xml?channel_id={id}"
            ))
            .await?;
            let label = yt_xml_tag(&feed, "title").unwrap_or_else(|| id.to_string());
            return Ok(serde_json::json!({ "channelId": id, "label": yt_unescape(&label) }));
        }
        q.to_string()
    } else {
        // Bare @handle or handle without @.
        let handle = q.trim_start_matches('@');
        format!("https://www.youtube.com/@{handle}")
    };

    // Handle / custom URL: read the channel page and pull the channel id
    // (the RSS auto-discovery link or the embedded channelId field).
    let page = yt_fetch_text(&page_url).await?;
    let id = page
        .split("channel_id=")
        .nth(1)
        .and_then(|r| r.split('&').next())
        .filter(|r| r.starts_with("UC") && r.len() > 20)
        .map(|r| r.to_string())
        .or_else(|| {
            page.find("\"channelId\":\"UC")
                .and_then(|i| {
                    let rest = &page[i + 12..];
                    rest.find('"').map(|e| rest[..e].to_string())
                })
                .filter(|r| r.len() > 20)
        });
    let Some(id) = id else {
        return Err("Couldn't find that channel — paste the channel page link or its UC… ID.".into());
    };
    let feed = yt_fetch_text(&format!(
        "https://www.youtube.com/feeds/videos.xml?channel_id={id}"
    ))
    .await?;
    let label = yt_xml_tag(&feed, "title").unwrap_or_else(|| id.clone());
    Ok(serde_json::json!({ "channelId": id, "label": yt_unescape(&label) }))
}

/// Latest videos for the player's channels (YouTube RSS hub, ~15 each).
#[tauri::command(rename_all = "camelCase")]
async fn youtube_my_feed_fetch(channel_ids: Vec<String>) -> Result<serde_json::Value, String> {
    let mut videos: Vec<serde_json::Value> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    for id in channel_ids.iter().take(24) {
        let url = format!("https://www.youtube.com/feeds/videos.xml?channel_id={id}");
        let feed = match yt_fetch_text(&url).await {
            Ok(f) => f,
            Err(e) => {
                errors.push(format!("{id}: {e}"));
                continue;
            }
        };
        for entry in feed.split("<entry>").skip(1) {
            let end = entry.find("</entry>").unwrap_or(entry.len());
            let seg = &entry[..end];
            let Some(video_id) = yt_xml_tag(seg, "yt:videoId") else {
                continue;
            };
            let title = yt_unescape(&yt_xml_tag(seg, "title").unwrap_or_default());
            let published = yt_xml_tag(seg, "published").unwrap_or_default();
            let channel = yt_unescape(&yt_xml_tag(seg, "name").unwrap_or_default());
            let thumb = yt_xml_attr(seg, "media:thumbnail", "url");
            let views = yt_xml_attr(seg, "media:statistics", "views")
                .and_then(|v| v.parse::<u64>().ok());
            videos.push(serde_json::json!({
                "videoId": video_id,
                "title": title,
                "thumbnailUrl": thumb,
                "channelName": channel,
                "viewCount": views,
                "publishedAt": published,
            }));
            if videos.len() >= 360 {
                break;
            }
        }
    }
    Ok(serde_json::json!({ "videos": videos, "errors": errors }))
}

/// After a pack file was deleted, drop the shared store object too when no
/// other pack links it — the user's «delete» must free disk space, not just
/// swap a link for a parked copy. Best-effort on a detached thread: never
/// blocks a command, and hard-link semantics make a wrong call harmless
/// (other packs' links keep the inode alive regardless).
fn release_store_object_detached(sha1: String) {
    let roots: Vec<PathBuf> = helpers::load_recent_projects()
        .into_iter()
        .map(|entry| PathBuf::from(entry.path))
        .collect();
    std::thread::spawn(move || {
        tuffbox_core::mod_store::release(&sha1, &roots);
    });
}

/// ── Per-pack dedup opt-out (docs/17) ─────────────────────────────────

/// Is the shared dedup store used for this pack? Reads the
/// `.tuffbox-no-dedup` marker in the project root (cheap — no hashing).
#[tauri::command(rename_all = "camelCase")]
fn dedup_project_status(path: String) -> Result<serde_json::Value, String> {
    let manifest_path = resolve_manifest_path(&path)?;
    let project_dir = manifest_path
        .parent()
        .ok_or("manifest has no parent directory")?
        .to_path_buf();
    Ok(serde_json::json!({
        "enabled": !tuffbox_core::mod_store::dedup_disabled(&project_dir),
    }))
}

/// Turn the shared dedup store on/off for one pack. Reversible at any time:
/// - off  → writes the marker, then materializes every store hardlink in
///   the pack back into independent copies (atomic per file; the store and
///   other packs keep their objects).
/// - on   → removes the marker and immediately links matching files into
///   the store (single-root retro-dedup).
/// Both sweeps hash the pack's jars/zips — run off the main thread.
#[tauri::command(rename_all = "camelCase")]
async fn dedup_project_set_enabled(path: String, enabled: bool) -> Result<serde_json::Value, String> {
    let manifest_path = resolve_manifest_path(&path)?;
    let project_dir = manifest_path
        .parent()
        .ok_or("manifest has no parent directory")?
        .to_path_buf();

    tokio::task::spawn_blocking(move || -> Result<serde_json::Value, String> {
        let marker = project_dir.join(tuffbox_core::mod_store::NO_DEDUP_MARKER);
        if enabled {
            let _ = std::fs::remove_file(&marker);
            let report = tuffbox_core::mod_store::retro_dedup(&[&project_dir]);
            Ok(serde_json::json!({
                "enabled": true,
                "action": "linked",
                "scanned": report.scanned,
                "linked": report.linked,
                "recorded": report.recorded,
                "skipped": report.skipped,
                "bytesReclaimed": report.bytes_reclaimed,
                "errors": report.errors,
            }))
        } else {
            if let Some(parent) = marker.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::write(&marker, b"")
                .map_err(|e| format!("write {}: {e}", marker.display()))?;
            let report = tuffbox_core::mod_store::materialize_project(&project_dir);
            Ok(serde_json::json!({
                "enabled": false,
                "action": "materialized",
                "scanned": report.scanned,
                "materialized": report.materialized,
                "skipped": report.skipped,
                "errors": report.errors,
            }))
        }
    })
    .await
    .map_err(|e| format!("dedup toggle task panicked: {e}"))?
}

/// Sodium config checks: (filename, fn(&content, &mut findings))
const SODIUM_CHECKS: &[(&str, fn(&str, &mut Vec<serde_json::Value>))] = &[(
    "sodium-options.json",
    |c: &str, f: &mut Vec<serde_json::Value>| {
        // Check if vsync is enabled (can cap FPS)
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(c) {
            if v.get("quality")
                .and_then(|q| q.get("use_block_face_culling"))
                .and_then(|x| x.as_str())
                == Some("1")
            {
                return;
            }
            if v.get("advanced")
                .and_then(|a| a.get("use_chunk_multidraw"))
                .and_then(|x| x.as_bool())
                == Some(false)
            {
                f.push(serde_json::json!({"severity":"info","code":"CHUNK_MULTIDRAW_OFF","message":"Chunk multidraw is disabled in Sodium; enable it for better FPS.","file":"config/sodium-options.json"}));
            }
            let render_dist = v
                .get("quality")
                .and_then(|q| q.get("render_distance"))
                .and_then(|x| x.as_str())
                .unwrap_or("16");
            if render_dist.parse::<u32>().unwrap_or(16) > 16 {
                f.push(serde_json::json!({"severity":"warning","code":"HIGH_RENDER_DISTANCE","message":format!("Render distance is {render_dist} — consider lowering to 12-16 for modded."),"file":"config/sodium-options.json"}));
            }
        }
    },
)];

/// Forge/NeoForge config checks: (filename_pattern, fn(&content, &filename, &mut findings))
const FORGE_PERF_CHECKS: &[(&str, fn(&str, &str, &mut Vec<serde_json::Value>))] = &[(
    "forge-server",
    |c: &str, name: &str, f: &mut Vec<serde_json::Value>| {
        if c.contains("max-tick-time") {
            for line in c.lines() {
                if line.contains("max-tick-time") {
                    let val = line.split('=').last().unwrap_or("").trim();
                    if val == "-1" {
                        f.push(serde_json::json!({"severity":"warning","code":"MAX_TICK_TIME_DISABLED","message":"max-tick-time is -1 (off) — the server won't crash on overload but may become permanently unresponsive.","file":format!("config/{name}")}));
                    }
                }
            }
        }
        // Check entity spawning limits
        for search in &[
            "max-entity-collisions",
            "spawn-limits",
            "max-breed",
            "despawn-ranges",
        ] {
            if c.contains(search) {
                f.push(serde_json::json!({"severity":"info","code":"SERVER_PERF_CONFIG_PRESENT","message":format!("Server performance config detected: {search}. Review limits for your player count."),"file":format!("config/{name}")}));
            }
        }
    },
)];

/// ── Ore generation scanner ──────────────────────────────────────────

/// Scans the project configs for ore-generation settings using both the
/// builtin knowledge base, per-mod overrides, and heuristics, returning a
/// list of detected ore gen toggle keys with estimated values.
///
/// Priority: overrides (exact keys, high confidence) → heuristics (pattern
/// matching, medium/low confidence) for mods without overrides.
#[tauri::command(rename_all = "camelCase")]
async fn scan_ore_generation(path: String) -> Result<Vec<serde_json::Value>, String> {
    tokio::task::spawn_blocking(move || scan_ore_generation_blocking(path))
        .await
        .map_err(|e| format!("ore scan task failed: {e}"))?
}

fn scan_ore_generation_blocking(path: String) -> Result<Vec<serde_json::Value>, String> {
    let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
    let project_dir = manifest_parent(&path)?;
    let mut config_contents = Vec::new();

    // Gather config files (skip huge / irrelevant trees for heuristics).
    for root in &["config", "defaultconfigs"] {
        let dir = project_dir.join(root);
        if !dir.is_dir() {
            continue;
        }
        fn walk(dir: &std::path::Path, root_parent: &std::path::Path, acc: &mut Vec<(String, String)>) {
            for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let p = entry.path();
                if p.is_dir() {
                    walk(&p, root_parent, acc);
                    continue;
                }
                if let Some(ext) = p.extension() {
                    if ext == "toml" || ext == "json" || ext == "cfg" || ext == "json5" {
                        let Ok(rel) = p.strip_prefix(root_parent) else {
                            continue;
                        };
                        let rel_str = rel.to_string_lossy().to_string();
                        // Still collect override-target files even if heuristics would skip them.
                        if let Ok(content) = std::fs::read_to_string(&p) {
                            if content.len() < 256 * 1024 {
                                acc.push((rel_str, content));
                            }
                        }
                    }
                }
            }
        }
        let root_parent = dir.parent().unwrap_or(&dir).to_path_buf();
        walk(&dir, &root_parent, &mut config_contents);
    }

    // Build a content lookup map for reading override key values
    let content_map: std::collections::HashMap<&str, &str> = config_contents
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();

    // ── Pass 1: per-mod overrides (exact keys, highest confidence) ──
    let registry = tuffbox_core::registry::AdapterRegistry::new();
    let mut override_resources = std::collections::HashSet::new();
    let mut results = Vec::new();

    for mod_spec in &manifest.mods {
        let Some(over) = registry.get_override(&mod_spec.id) else {
            continue;
        };
        for mapping in over.ore_gen_config_keys() {
            let Some(content) = content_map.get(mapping.config_file.as_str()) else {
                continue;
            };
            // Read actual enabled value from the config file
            let mut enabled_value = read_config_key(content, &mapping.enabled_key)
                .unwrap_or_else(|| "true".to_string());
            // Invert if this is a disable-key (e.g. disableZincOre = true → ore is OFF)
            if mapping.enabled_inverted && enabled_value == "true" {
                enabled_value = "false".to_string();
            } else if mapping.enabled_inverted && enabled_value == "false" {
                enabled_value = "true".to_string();
            }
            let vein_size = mapping
                .vein_size_key
                .as_deref()
                .and_then(|k| read_config_key(content, k))
                .map(|v| (mapping.vein_size_key.clone().unwrap_or_default(), v));
            let min_height = mapping
                .min_height_key
                .as_deref()
                .and_then(|k| read_config_key(content, k))
                .map(|v| (mapping.min_height_key.clone().unwrap_or_default(), v));
            let max_height = mapping
                .max_height_key
                .as_deref()
                .and_then(|k| read_config_key(content, k))
                .map(|v| (mapping.max_height_key.clone().unwrap_or_default(), v));

            let kb_hint =
                tuffbox_core::knowledge::builtin::ModKnowledgeEntry::lookup(&mapping.resource_name);
            let confidence = if kb_hint.is_some() { "high" } else { "medium" };

            override_resources.insert(mapping.resource_name.clone());
            results.push(serde_json::json!({
                "resource": mapping.resource_name,
                "configFile": mapping.config_file,
                "enabledKey": mapping.enabled_key,
                "enabledValue": enabled_value,
                "veinSize": vein_size,
                "minHeight": min_height,
                "maxHeight": max_height,
                "spawnsPerChunk": null,
                "confidence": confidence,
                "knownMod": kb_hint.map(|k| k.name.clone()),
            }));
        }
    }

    // ── Pass 2: heuristics for mods without overrides (skip junk paths) ──
    let heuristic_inputs: Vec<(String, String)> = config_contents
        .iter()
        .filter(|(p, _)| tuffbox_core::knowledge::heuristics::is_plausible_ore_config_path(p))
        .cloned()
        .collect();
    let heuristic_hits =
        tuffbox_core::knowledge::heuristics::scan_configs_for_ore_gen(&heuristic_inputs);

    for hit in &heuristic_hits {
        // Skip if override already covers this resource
        if override_resources.contains(&hit.resource_name) {
            continue;
        }
        // Drop low-confidence hits with no height/vein signal — they are almost always noise.
        if hit.confidence == tuffbox_core::knowledge::heuristics::HeuristicConfidence::Low
            && hit.min_height.is_none()
            && hit.max_height.is_none()
            && hit.vein_size.is_none()
            && hit.spawns_per_chunk.is_none()
        {
            continue;
        }
        let kb_hint =
            tuffbox_core::knowledge::builtin::ModKnowledgeEntry::lookup(&hit.resource_name);
        let confidence = match (hit.confidence, kb_hint.is_some()) {
            (tuffbox_core::knowledge::heuristics::HeuristicConfidence::Medium, true) => "high",
            (tuffbox_core::knowledge::heuristics::HeuristicConfidence::Medium, false) => "medium",
            (tuffbox_core::knowledge::heuristics::HeuristicConfidence::High, _) => "high",
            (_, true) => "medium",
            _ => "low",
        };
        results.push(serde_json::json!({
            "resource": hit.resource_name,
            "configFile": hit.config_file,
            "enabledKey": hit.enabled_key,
            "enabledValue": hit.enabled_value,
            "veinSize": hit.vein_size,
            "minHeight": hit.min_height,
            "maxHeight": hit.max_height,
            "spawnsPerChunk": hit.spawns_per_chunk,
            "confidence": confidence,
            "knownMod": kb_hint.map(|k| k.name.clone()),
        }));
    }

    // Stable, readable order: known materials first, then by name.
    results.sort_by(|a, b| {
        let ar = a.get("resource").and_then(|v| v.as_str()).unwrap_or("");
        let br = b.get("resource").and_then(|v| v.as_str()).unwrap_or("");
        let ac = a.get("confidence").and_then(|v| v.as_str()).unwrap_or("low");
        let bc = b.get("confidence").and_then(|v| v.as_str()).unwrap_or("low");
        let rank = |c: &str| match c {
            "high" => 0,
            "medium" => 1,
            _ => 2,
        };
        rank(ac).cmp(&rank(bc)).then_with(|| ar.cmp(br))
    });

    Ok(results)
}

/// Read a single key's value from a config file content string.
fn read_config_key(content: &str, key: &str) -> Option<String> {
    let key_lower = key.to_lowercase();
    for line in content.lines() {
        let trimmed = line
            .trim()
            .trim_start_matches("B:")
            .trim_start_matches("I:")
            .trim_start_matches("S:");
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
            continue;
        }
        // TOML: key = value
        if let Some(eq) = trimmed.find('=') {
            let k = trimmed[..eq].trim();
            if k.eq_ignore_ascii_case(key) || k.to_lowercase() == key_lower {
                let v = trimmed[eq + 1..].trim().trim_matches('"').trim_matches('\'');
                return Some(v.to_string());
            }
        }
        // JSON: "key": value
        if let Some(colon) = trimmed.trim_end_matches(',').find(':') {
            let k = trimmed[..colon].trim().trim_matches('"');
            if k.eq_ignore_ascii_case(key) || k.to_lowercase() == key_lower {
                let v = trimmed[colon + 1..].trim().trim_matches('"').trim_matches('\'');
                return Some(v.to_string());
            }
        }
    }
    None
}

/// ── Duplicate detection ─────────────────────────────────────────────

/// Scans installed mods for duplicate resources (e.g., two mods both
/// adding "tin_ingot") and returns resolution suggestions with
/// generated KubeJS/CraftTweaker scripts.
#[tauri::command(rename_all = "camelCase")]
async fn detect_duplicate_items(path: String) -> Result<Vec<serde_json::Value>, String> {
    // Blocking-pool conversion: sync commands run on the main thread (see
    // get_health_report); heavy I/O must not freeze IPC delivery.
    tokio::task::spawn_blocking(move || detect_duplicate_items_impl(path))
        .await
        .map_err(|e| e.to_string())?
}

fn detect_duplicate_items_impl(path: String) -> Result<Vec<serde_json::Value>, String> {
    let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
    let project_dir = manifest_parent(&path)?;
    let mods_dir = project_dir.join("mods");

    // Build mod→items mapping from what we can extract
    let mut mod_items: Vec<(String, Vec<String>)> = Vec::new();
    // Use knowledge base programmatic items as a starting point
    for entry in tuffbox_core::knowledge::builtin::ModKnowledgeEntry::builtin() {
        if manifest.mods.iter().any(|m| m.id == entry.slug) && !entry.programmatic_items.is_empty()
        {
            mod_items.push((entry.slug.clone(), entry.programmatic_items.clone()));
        }
    }

    // Also try to read known item registry from mod jars if available
    if mods_dir.is_dir() {
        for entry in std::fs::read_dir(&mods_dir).into_iter().flatten().flatten() {
            let p = entry.path();
            if p.extension().map_or(true, |e| e != "jar") {
                continue;
            }
            // Quick pattern-match from jar filename
            let name = entry.file_name().to_string_lossy().to_string();
            let id = name.trim_end_matches(".jar").to_string();
            // Try to classify jar filename fragments as potential items
            let parts: Vec<&str> = id.split('-').collect();
            let mut items = Vec::new();
            for part in &parts {
                if let Some((mat, ty)) = tuffbox_core::knowledge::heuristics::classify_item(part) {
                    items.push(format!("{}_{}", mat, ty));
                }
            }
            if !items.is_empty() {
                mod_items.push((id, items));
            }
        }
    }

    // Run duplicate detection
    let groups = tuffbox_core::knowledge::heuristics::detect_duplicate_groups(&mod_items);
    let resolutions = tuffbox_core::unified::duplicate::resolve_duplicates(&groups);

    let mut results = Vec::new();
    for (idx, res) in resolutions.iter().enumerate() {
        results.push(serde_json::json!({
            "id": format!("dedup-{}", idx),
            "material": res.material,
            "itemType": res.item_type,
            "keepItem": res.keep,
            "removeItems": res.remove,
            "kubejsScript": res.to_kubejs6(),
            "crafttweakerScript": res.to_crafttweaker(),
        }));
    }
    Ok(results)
}

/// ── Almost Unified config generator ────────────────────────────────

/// Generates an Almost Unified config (unify.json) tailored for the
/// project's installed mods, and optionally writes it to disk.
#[tauri::command(rename_all = "camelCase")]
async fn generate_unify_config(path: String, save: Option<bool>) -> Result<serde_json::Value, String> {
    // Blocking-pool conversion: sync commands run on the main thread (see
    // get_health_report); heavy I/O must not freeze IPC delivery.
    tokio::task::spawn_blocking(move || generate_unify_config_impl(path, save))
        .await
        .map_err(|e| e.to_string())?
}

fn generate_unify_config_impl(path: String, save: Option<bool>) -> Result<serde_json::Value, String> {
    let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
    let mod_slugs: Vec<String> = manifest.mods.iter().map(|m| m.id.clone()).collect();
    let config = tuffbox_core::unified::unify_config::UnifyConfig::for_project(&mod_slugs);

    if save.unwrap_or(false) {
        let project_dir = manifest_parent(&path)?;
        let config_path = project_dir
            .join("config")
            .join("almostunified")
            .join("unify.json");
        config.save_to(&config_path).map_err(|e| e.to_string())?;
    }

    let kubejs = tuffbox_core::unified::unify_config::generate_unification_script(&[]);
    Ok(serde_json::json!({
        "config": config,
        "expandedTagsCount": config.expanded_tags().len(),
        "materialCount": config.materials.len(),
        "priorityCount": config.mod_priorities.len(),
        "kubejsScript": kubejs,
    }))
}

/// ── Crash Assistant analysis ───────────────────────────────────────

/// Runs the full Crash Assistant analysis on the project, detecting
/// common crash patterns: wrong Java, mixin failures, missing mods,
/// Intel CPU bugs, integrated GPU, corrupted installs, and more.
#[tauri::command(rename_all = "camelCase")]
#[allow(dead_code)]
fn run_crash_assistant(path: String) -> Result<serde_json::Value, String> {
    let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
    let project_dir = manifest_parent(&path)?;

    let installed_mods: Vec<String> = manifest.mods.iter().map(|m| m.id.clone()).collect();
    let mut crash_content = Vec::new();
    let mut latest_log = String::new();
    let mut launcher_log = String::new();

    // Read crash reports
    let crash_dir = project_dir.join("crash-reports");
    if crash_dir.is_dir() {
        for entry in std::fs::read_dir(&crash_dir)
            .into_iter()
            .flatten()
            .flatten()
        {
            if entry.path().extension().map_or(false, |e| e == "txt") {
                if let Ok(content) = std::fs::read_to_string(entry.path()) {
                    if content.len() < 4 * 1024 * 1024 {
                        crash_content.push(content);
                    }
                }
            }
        }
    }

    // Read latest.log
    let latest_path = project_dir.join("logs").join("latest.log");
    if latest_path.is_file() {
        latest_log = tuffbox_core::process::read_log_tail(&latest_path, 900).unwrap_or_default();
    }

    // Read launcher.log
    let launcher_path = project_dir.join("logs").join("launcher.log");
    if launcher_path.is_file() {
        launcher_log = std::fs::read_to_string(&launcher_path).unwrap_or_default();
    }

    // Gather system info
    let java_path = manifest
        .java
        .as_ref()
        .and_then(|j| j.path.clone())
        .unwrap_or_default();
    let java_version = if !java_path.is_empty() {
        tuffbox_core::jre::check_java_at_path(&std::path::PathBuf::from(&java_path))
            .map(|r| r.version)
            .unwrap_or_default()
    } else {
        tuffbox_core::jre::find_all_runtimes()
            .ok()
            .and_then(|r| r.into_iter().next())
            .map(|r| r.version)
            .unwrap_or_default()
    };

    let ctx = tuffbox_core::crash_assistant::AnalysisCtx {
        crash_content: crash_content,
        latest_log: latest_log,
        launcher_log: launcher_log,
        installed_mods: installed_mods.clone(),
        installed_versions: Some(
            manifest
                .mods
                .iter()
                .map(|m| (m.id.clone(), m.version.clone()))
                .collect(),
        ),
        previous_mods: Vec::new(),
        java_version,
        java_vendor: String::new(),
        os_name: std::env::consts::OS.to_string(),
        mc_version: manifest.minecraft.version.clone(),
        loader: format!("{:?}", manifest.loader.kind).to_lowercase(),
        loader_version: manifest.loader.version.clone(),
        cpu_name: String::new(),
        gpu_names: Vec::new(),
        total_ram_mb: 0,
        is_offline: false,
        win_events: Vec::new(),
        combined_lines: std::cell::OnceCell::new(),
    };

    let report = tuffbox_core::crash_assistant::run_full_analysis(&ctx);

    Ok(serde_json::json!({
        "findings": report.findings.iter().map(|f| serde_json::json!({
            "severity": format!("{:?}", f.severity).to_lowercase(),
            "code": f.code,
            "title": f.title,
            "description": f.description,
            "autoFix": f.auto_fix,
            "references": f.references,
        })).collect::<Vec<_>>(),
        "supportMessageDiscord": report.support_message_discord,
        "supportMessageGithub": report.support_message_github,
        "modsAdded": report.mods_added,
        "modsRemoved": report.mods_removed,
        "suspectedMods": report.suspected_mods,
        "findingsCount": report.findings.len(),
    }))
}

/// ── Package/Class Finder + Jdeps (Crash Assistant tools) ──────────

/// Searches all mod JARs to find which one contains a given Java class.
/// This mirrors Crash Assistant's Package/Class Finder GUI tool.
#[tauri::command(rename_all = "camelCase")]
async fn find_class_in_mods(path: String, class_name: String) -> Result<Vec<serde_json::Value>, String> {
    // Blocking-pool conversion: sync commands run on the main thread (see
    // get_health_report); heavy I/O must not freeze IPC delivery.
    tokio::task::spawn_blocking(move || find_class_in_mods_impl(path, class_name))
        .await
        .map_err(|e| e.to_string())?
}

fn find_class_in_mods_impl(path: String, class_name: String) -> Result<Vec<serde_json::Value>, String> {
    let project_dir = manifest_parent(&path)?;
    let mods_dir = project_dir.join("mods");
    let mods_key = format!(
        "{}:{}",
        mods_dir.display(),
        tuffbox_core::installed_jars_fingerprint(Path::new(&path))
    );
    let results = find_class_in_mods_cached(&class_name, &mods_dir, &mods_key);
    Ok(results
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "className": r.class_name,
                "modId": r.mod_id,
                "modName": r.mod_name,
            })
        })
        .collect())
}

/// Searches all mod JARs to find which mods depend on a given class
/// (Jdeps analysis tool from Crash Assistant).
#[tauri::command(rename_all = "camelCase")]
async fn find_dependents_on_class(
    path: String,
    class_name: String,
) -> Result<Vec<serde_json::Value>, String> {
    // Blocking-pool conversion: sync commands run on the main thread (see
    // get_health_report); heavy I/O must not freeze IPC delivery.
    tokio::task::spawn_blocking(move || find_dependents_on_class_impl(path, class_name))
        .await
        .map_err(|e| e.to_string())?
}

fn find_dependents_on_class_impl(
    path: String,
    class_name: String,
) -> Result<Vec<serde_json::Value>, String> {
    let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
    let project_dir = manifest_parent(&path)?;
    let mods_dir = project_dir.join("mods");
    let installed: Vec<String> = manifest.mods.iter().map(|m| m.id.clone()).collect();
    let results = tuffbox_core::crash_assistant::find_mods_depending_on_class(
        &class_name,
        &mods_dir,
        &installed,
    );
    Ok(results
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "className": r.class_name,
                "modId": r.mod_id,
                "modName": r.mod_name,
            })
        })
        .collect())
}

/// Runs the full Crash Assistant analysis and also includes MCreator
/// mod list, class finder results from crash logs, and Jdeps results.
///
/// When `report_id` is set, only that crash report is analyzed (plus
/// `logs/latest.log` and the current installed mod list). Otherwise the
/// newest crash report is used — never the entire crash-reports folder.
#[tauri::command(rename_all = "camelCase")]
async fn run_crash_assistant_full(
    app: tauri::AppHandle,
    path: String,
    report_id: Option<String>,
) -> Result<serde_json::Value, String> {
    // Task #66: same blocking-pool treatment as get_crash_diagnosis.
    tokio::task::spawn_blocking(move || run_crash_assistant_full_impl(&app, path, report_id))
        .await
        .map_err(|e| e.to_string())?
}

/// Task #66: process-wide cache of class→jar lookups. Scanning every mod jar
/// for a missing class is the dominant cost of the Diagnose tab; the same
/// class names reappear on every run, so memoize across runs (keyed by mods
/// dir + class). Entries are cheap (small Vec) and bounded by distinct classes.
#[derive(Default)]
struct ClassFinderCache(Mutex<std::collections::HashMap<(String, String), Vec<tuffbox_core::crash_assistant::ClassMatch>>>);

static GLOBAL_APP_HANDLE: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();

fn app_handle_global() -> Option<&'static tauri::AppHandle> {
    GLOBAL_APP_HANDLE.get()
}

/// Look up `cls` in `mods_dir`, memoizing into the managed process-wide cache.
/// Cache misses fall back to the full jar scan and store the result.
fn find_class_in_mods_cached(
    cls: &str,
    mods_dir: &std::path::Path,
    mods_dir_key: &str,
) -> Vec<tuffbox_core::crash_assistant::ClassMatch> {
    use tauri::Manager;
    // AppHandle global: fall back to a direct scan if state isn't up yet
    // (early startup) — correctness first, cache is only an accelerator.
    let matches = {
        match app_handle_global() {
            Some(app) => {
                let cache = app.state::<ClassFinderCache>();
                let key = (mods_dir_key.to_string(), cls.to_string());
                if let Ok(guard) = cache.0.lock() {
                    if let Some(hit) = guard.get(&key) {
                        return hit.clone();
                    }
                }
                let found = tuffbox_core::crash_assistant::find_class_in_mods(cls, mods_dir);
                if let Ok(mut guard) = cache.0.lock() {
                    guard.insert(key, found.clone());
                }
                found
            }
            None => tuffbox_core::crash_assistant::find_class_in_mods(cls, mods_dir),
        }
    };
    matches
}

fn run_crash_assistant_full_impl(
    app: &tauri::AppHandle,
    path: String,
    report_id: Option<String>,
) -> Result<serde_json::Value, String> {
    let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
    let project_dir = manifest_parent(&path)?;
    let mods_dir = project_dir.join("mods");
    diagnose_stage(app, "Running crash checks…");
    let report = run_crash_assistant_analysis(&path, &manifest, &project_dir, report_id.as_deref())?;

    let class_finder_started = std::time::Instant::now();
    let mut class_finder = Vec::new();
    let mut combined = String::new();
    // Class attribution is performed by the batch scanner below; its
    // process-wide single-class cache is used by the interactive tool.
    let mut class_finder_seen = std::collections::HashSet::new();
    if let Some(text) = load_scoped_crash_report(&project_dir, report_id.as_deref()) {
        combined.push_str(&text);
        combined.push('\n');
    }
    let latest = project_dir.join("logs").join("latest.log");
    if latest.is_file() {
        combined.push_str(
            &tuffbox_core::process::read_log_tail(&latest, 2000).unwrap_or_default(),
        );
    }
    // Pre-compute the unique class list so the stage label can say how many
    // jar-scan rounds are coming (each round opens every mod jar once).
    let mut unique_classes: Vec<String> = Vec::new();
    for line in combined.lines() {
        if line.contains("NoClassDefFoundError") || line.contains("ClassNotFoundException") {
            if let Some(cls) = line
                .split(": ")
                .nth(1)
                .and_then(|s| s.split_whitespace().next())
            {
                if cls.len() > 5
                    && cls.len() < 200
                    && cls.contains('.')
                    && !unique_classes.iter().any(|c| c == cls)
                {
                    unique_classes.push(cls.to_string());
                }
            }
        }
    }
    if !unique_classes.is_empty() {
        diagnose_stage(
            app,
            &format!(
                "Searching {} missing class (candidates) in mod jars…",
                unique_classes.len()
            ),
        );
    }
    if !unique_classes.is_empty() {
        let matches = tuffbox_core::crash_assistant::find_classes_in_mods(
            &unique_classes,
            &mods_dir,
        );
        for m in matches {
            let key = format!("{}:{}", m.mod_id, m.class_name);
            if class_finder_seen.insert(key) {
                class_finder.push(serde_json::json!({
                    "className": m.class_name,
                    "modId": m.mod_id,
                    "modName": m.mod_name,
                }));
            }
        }
    }
    class_finder.truncate(20);
    diagnose_timing(Some(app), "class_finder", class_finder_started, false);

    Ok(serde_json::json!({
        "findings": report.findings.iter().map(|f| serde_json::json!({
            "severity": f.severity,
            "code": f.code,
            "title": f.title,
            "description": f.description,
            "autoFix": f.auto_fix,
            "references": f.references,
            "evidence": f.evidence,
            "fixes": f.fixes.iter().map(|a| serde_json::json!({
                "kind": a.kind,
                "label": a.label,
                "modId": a.mod_id,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "supportMessageDiscord": report.support_message_discord,
        "supportMessageGithub": report.support_message_github,
        "modsAdded": report.mods_added,"modsRemoved": report.mods_removed,
        "suspectedMods": report.suspected_mods,
        "mcreatorMods": report.mcreator_mods,
        "classFinderResults": class_finder,
        "findingsCount": report.findings.len(),
        "scope": {
            "reportId": report_id,
            "latestLog": true,
            "installedMods": true,
        },
    }))
}

/// Load a single crash report by id (filename stem / path fragment), or the
/// newest `.txt` under `crash-reports/` when `report_id` is None.
fn load_scoped_crash_report(project_dir: &Path, report_id: Option<&str>) -> Option<String> {
    load_scoped_crash_report_with_path(project_dir, report_id).map(|(_, text)| text)
}

/// Frontend source-picker sentinel for the launcher log (must mirror
/// `LAUNCHER_LOG_SOURCE` in Diagnostics.svelte).
const LAUNCHER_LOG_SOURCE_MARKER: &str = "__launcher_log__";

/// True when the caller selected a real crash-report file (not latest.log).
fn is_explicit_crash_report_id(report_id: Option<&str>) -> bool {
    matches!(report_id, Some(id) if !id.is_empty() && id != "__latest_log__")
}

/// Load a crash-report only when `report_id` is an explicit file id/name.
/// Does **not** fall back to the newest crash — that broke "AI explain on
/// latest.log" by always injecting the previous crash text into the prompt.
fn load_scoped_crash_report_with_path(
    project_dir: &Path,
    report_id: Option<&str>,
) -> Option<(PathBuf, String)> {
    let id = report_id.filter(|s| !s.is_empty() && *s != "__latest_log__")?;
    let cd = project_dir.join("crash-reports");
    if !cd.is_dir() {
        return None;
    }
    let files: Vec<std::fs::DirEntry> = std::fs::read_dir(&cd)
        .ok()?
        .flatten()
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "txt"))
        .collect();
    if files.is_empty() {
        return None;
    }
    let entry = files.iter().find(|e| {
        let name = e.file_name().to_string_lossy().to_string();
        name == id
            || name.trim_end_matches(".txt") == id
            || e.path().to_string_lossy().contains(id)
    })?;
    let path = entry.path();
    let text = std::fs::read_to_string(&path)
        .ok()
        .filter(|c| c.len() < 4 * 1024 * 1024)?;
    Some((path, text))
}

/// Newest `crash-reports/*.txt` by mtime (for flows that intentionally want
/// "last crash" rather than a user-selected report or latest.log).
fn load_newest_crash_report(project_dir: &Path) -> Option<(PathBuf, String)> {
    let cd = project_dir.join("crash-reports");
    if !cd.is_dir() {
        return None;
    }
    let mut files: Vec<std::fs::DirEntry> = std::fs::read_dir(&cd)
        .ok()?
        .flatten()
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "txt"))
        .collect();
    if files.is_empty() {
        return None;
    }
    files.sort_by_key(|e| std::cmp::Reverse(e.metadata().ok().and_then(|m| m.modified().ok())));
    let path = files[0].path();
    let text = std::fs::read_to_string(&path)
        .ok()
        .filter(|c| c.len() < 4 * 1024 * 1024)?;
    Some((path, text))
}

/// ── Mod compatibility checker ──────────────────────────────────────

/// Scans installed mods against the knowledge base to find known
/// compatibility issues: conflicts, missing dependencies, wrong-loader
/// mods, and version mismatches.
#[tauri::command(rename_all = "camelCase")]
fn check_mod_compatibility(path: String) -> Result<Vec<serde_json::Value>, String> {
    let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
    let project_dir = manifest_parent(&path)?;
    let mut issues = Vec::new();

    let mods: Vec<String> = manifest.mods.iter().map(|m| m.id.clone()).collect();

    // Check known conflicts from knowledge base
    for a in &mods {
        for b in &mods {
            if a < b {
                if let Some(reason) = tuffbox_core::knowledge::check_known_conflict(a, b) {
                    issues.push(serde_json::json!({
                        "severity": "error", "code": "KNOWN_CONFLICT",
                        "message": reason,
                        "mods": [a, b],
                    }));
                }
            }
        }
    }

    // Check wrong-loader mods via heuristic
    let loader = format!("{:?}", manifest.loader.kind).to_lowercase();
    let mods_dir = project_dir.join("mods");
    let provider = tuffbox_core::ModrinthProvider::new();
    if mods_dir.is_dir() {
        for entry in std::fs::read_dir(&mods_dir).into_iter().flatten().flatten() {
            let p = entry.path();
            if p.extension().map_or(true, |e| e != "jar") {
                continue;
            }
            if let Ok(sha1) = tuffbox_core::mc_install::sha1_file(&p) {
                if let Ok(Some((_proj, ver))) = provider.identify_local_jar(&sha1) {
                    let loaders: Vec<&str> = ver.loaders.iter().map(|s| s.as_str()).collect();
                    if !loaders.is_empty() && !loaders.contains(&loader.as_str()) {
                        issues.push(serde_json::json!({
                            "severity": "warning", "code": "WRONG_LOADER",
                            "message": format!("{} is for {} but project uses {}", entry.file_name().to_string_lossy(), loaders.join(","), loader),
                            "mods": [entry.file_name().to_string_lossy()],
                        }));
                    }
                }
            }
        }
    }

    // Run graph diagnostics
    let graph = DependencyGraph::from_manifest(&manifest);
    let diags = Resolver::analyze_project(&manifest, &graph);
    for d in &diags {
        issues.push(serde_json::json!({
            "severity": format!("{:?}", d.severity).to_lowercase(),
            "code": d.code,
            "message": d.message,
            "mods": d.related_nodes.iter().map(|n| n.0.clone()).collect::<Vec<_>>(),
        }));
    }

    Ok(issues)
}

/// Compares two modpacks and returns a diff of mods, versions, and settings.
#[tauri::command(rename_all = "camelCase")]
fn compare_modpacks(path_a: String, path_b: String) -> Result<serde_json::Value, String> {
    let ma = ProjectManifest::load_from_path(&path_a).map_err(|e| e.to_string())?;
    let mb = ProjectManifest::load_from_path(&path_b).map_err(|e| e.to_string())?;

    let mods_a: std::collections::HashSet<String> = ma.mods.iter().map(|m| m.id.clone()).collect();
    let mods_b: std::collections::HashSet<String> = mb.mods.iter().map(|m| m.id.clone()).collect();

    let only_a: Vec<&String> = mods_a.difference(&mods_b).collect();
    let only_b: Vec<&String> = mods_b.difference(&mods_a).collect();
    let common: Vec<&String> = mods_a.intersection(&mods_b).collect();

    // Version differences for common mods
    let mut version_diffs = Vec::new();
    for id in &common {
        let va = ma
            .mods
            .iter()
            .find(|m| m.id == **id)
            .map(|m| m.version.clone());
        let vb = mb
            .mods
            .iter()
            .find(|m| m.id == **id)
            .map(|m| m.version.clone());
        if va != vb {
            version_diffs.push(serde_json::json!({"id": id, "versionA": va, "versionB": vb}));
        }
    }

    Ok(serde_json::json!({
        "nameA": ma.project.name,
        "nameB": mb.project.name,
        "mcVersionA": ma.minecraft.version,
        "mcVersionB": mb.minecraft.version,
        "loaderA": format!("{:?} {}", ma.loader.kind, ma.loader.version),
        "loaderB": format!("{:?} {}", mb.loader.kind, mb.loader.version),
        "modsOnlyInA": only_a,
        "modsOnlyInB": only_b,
        "commonMods": common.len(),
        "versionDiffs": version_diffs,
        "totalModsA": mods_a.len(),
        "totalModsB": mods_b.len(),
    }))
}

/// ── Backup system (like NitroLaunch backup plugin) ──────────────

/// Creates a full backup of the project (mods, configs, resourcepacks,
/// shaderpacks, manifest + lockfile) as a zip archive.
#[tauri::command(rename_all = "camelCase")]
fn create_project_backup(path: String, name: Option<String>) -> Result<serde_json::Value, String> {
    let project_dir = manifest_parent(&path)?;
    let dir = backup_dir(&project_dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let backup_name = name.unwrap_or_else(|| "manual-backup".into());
    let id = format!(
        "{}-{}",
        backup_name.replace(' ', "-"),
        tuffbox_core::time_util::compact_now()
    );
    let zip_path = dir.join(format!("{}.zip", id));

    let output = std::fs::File::create(&zip_path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipWriter::new(output);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    let mut total_size: u64 = 0;
    for folder in &[
        "mods",
        "config",
        "defaultconfigs",
        "kubejs",
        "scripts",
        "resourcepacks",
        "shaderpacks",
        "datapacks",
    ] {
        let d = project_dir.join(folder);
        if d.is_dir() {
            fn add_dir(
                zip: &mut zip::ZipWriter<std::fs::File>,
                opts: zip::write::SimpleFileOptions,
                base: &Path,
                dir: &Path,
                size: &mut u64,
            ) -> Result<(), String> {
                for e in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
                    let e = e.map_err(|e| e.to_string())?;
                    let p = e.path();
                    if p.is_dir() {
                        add_dir(zip, opts, base, &p, size)?;
                    } else if p.is_file() {
                        if let Ok(meta) = p.metadata() {
                            *size += meta.len();
                        }
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
            add_dir(&mut zip, opts, &project_dir, &d, &mut total_size)?;
        }
    }

    // Also backup manifest and lockfile
    let mainfest = project_dir.join("project.tuffbox.json");
    if mainfest.is_file() {
        zip.start_file("project.tuffbox.json", opts)
            .map_err(|e| e.to_string())?;
        zip.write_all(&std::fs::read(&mainfest).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    }

    zip.finish().map_err(|e| e.to_string())?;

    // Update index
    let mut idx = load_backup_index(&project_dir);
    idx.backups.push(BackupEntry {
        id: id.clone(),
        name: backup_name.clone(),
        created_at: tuffbox_core::time_util::rfc3339_now(),
        size_bytes: total_size,
        manifest_snapshot: true,
    });
    // Trim old backups
    while idx.backups.len() > idx.max_count as usize {
        let old = idx.backups.remove(0);
        let _ = std::fs::remove_file(dir.join(format!("{}.zip", old.id)));
    }
    save_backup_index(&project_dir, &idx)?;

    Ok(serde_json::json!({
        "id": id, "name": backup_name, "path": zip_path.to_string_lossy(),
        "sizeBytes": total_size, "createdAt": tuffbox_core::time_util::rfc3339_now(),
    }))
}

/// Lists all project backups.
#[tauri::command(rename_all = "camelCase")]
fn list_backups(path: String) -> Result<Vec<serde_json::Value>, String> {
    let project_dir = manifest_parent(&path)?;
    let idx = load_backup_index(&project_dir);
    Ok(idx
        .backups
        .into_iter()
        .rev()
        .map(|b| {
            serde_json::json!({
                "id": b.id, "name": b.name, "createdAt": b.created_at,
                "sizeBytes": b.size_bytes, "manifestSnapshot": b.manifest_snapshot,
            })
        })
        .collect())
}

/// Deletes a specific backup.
#[tauri::command(rename_all = "camelCase")]
fn delete_backup(path: String, backup_id: String) -> Result<(), String> {
    let project_dir = manifest_parent(&path)?;
    let dir = backup_dir(&project_dir);
    let zip_path = dir.join(format!("{}.zip", backup_id));
    if zip_path.is_file() {
        std::fs::remove_file(&zip_path).map_err(|e| e.to_string())?;
    }
    let mut idx = load_backup_index(&project_dir);
    idx.backups.retain(|b| b.id != backup_id);
    save_backup_index(&project_dir, &idx)
}

/// ── AI Crash Explanation context builder ─────────────────────────

/// Builds a structured AI context from crash data (but does NOT call any
/// LLM — the frontend can send this to any AI provider).
#[tauri::command(rename_all = "camelCase")]
fn prepare_ai_crash_context(
    path: &str,
    report_id: Option<&str>,
) -> Result<
    (
        tuffbox_core::ai_explanation::CrashAiContext,
        tuffbox_core::crash_kb::CrashFingerprint,
        String,
        usize,
    ),
    String,
> {
    // Cache the heavy prep (jar scan, inventory, graph) keyed by input mtimes —
    // analyze_crash_with_ai + build_ai_crash_context both call this per run; the
    // second call must not rescan everything (Diagnose appeared stuck).
    let cache_key = ai_context_cache_key(path, report_id);
    if let Some(cached) = tuffbox_core::api_cache::get::<
        (
            tuffbox_core::ai_explanation::CrashAiContext,
            tuffbox_core::crash_kb::CrashFingerprint,
            String,
            usize,
        ),
    >(&cache_key)
    {
        return Ok(cached);
    }
    let result = prepare_ai_crash_context_uncached(path, report_id)?;
    // Short TTL: inputs are covered by the key; TTL just bounds memory.
    tuffbox_core::api_cache::put_with_ttl(
        cache_key,
        result.clone(),
        std::time::Duration::from_secs(300),
    );
    Ok(result)
}

/// Cache key for [`prepare_ai_crash_context`] — same inputs as the diagnosis
/// cache (manifest, latest.log, newest crash report, mods dir) + report id.
fn ai_context_cache_key(path: &str, report_id: Option<&str>) -> String {
    let manifest_mtime = resolve_manifest_path(path)
        .ok()
        .and_then(|p| file_mtime_nanos(&p))
        .unwrap_or(0);
    let project_dir = resolve_manifest_path(path)
        .ok()
        .and_then(|p| manifest_parent(&p.to_string_lossy().to_string()).ok())
        .unwrap_or_default();
    let latest_mtime = file_mtime_nanos(&project_dir.join("logs").join("latest.log")).unwrap_or(0);
    let crash_mtime = newest_crash_report_mtime(&project_dir).unwrap_or(0);
    let mods_mtime = dir_mtime_nanos(&project_dir.join("mods")).unwrap_or(0);
    let launcher_mtime =
        file_mtime_nanos(&project_dir.join("logs").join("launcher.log")).unwrap_or(0);
    format!(
        "crashai:{}:{:?}:{}:{}:{}:{}:{}",
        path, report_id, manifest_mtime, latest_mtime, launcher_mtime, crash_mtime, mods_mtime
    )
}

fn prepare_ai_crash_context_uncached(
    path: &str,
    report_id: Option<&str>,
) -> Result<
    (
        tuffbox_core::ai_explanation::CrashAiContext,
        tuffbox_core::crash_kb::CrashFingerprint,
        String,
        usize,
    ),
    String,
> {
    let manifest_path = resolve_manifest_path(path)?;
    let manifest = ProjectManifest::load_from_path(&manifest_path).map_err(|e| e.to_string())?;
    let project_dir = manifest_parent(path)?;

    let crash_content =
        load_scoped_crash_report(&project_dir, report_id).unwrap_or_default();
    let latest = project_dir.join("logs").join("latest.log");
    // When explaining latest.log (no crash report selected), pull a larger tail
    // so the model sees the live session instead of an empty crash excerpt.
    let using_crash_file = is_explicit_crash_report_id(report_id);
    // `__launcher_log__` source: feed the launcher log as the primary excerpt so
    // the cascade (fingerprint / KB / AI prompt) actually sees log content
    // instead of analyzing an empty haystack.
    let using_launcher_log = report_id == Some(crate::LAUNCHER_LOG_SOURCE_MARKER);
    let launcher_tail = if using_launcher_log {
        let la = project_dir.join("logs").join("launcher.log");
        if la.is_file() {
            tuffbox_core::process::read_log_tail(&la, 2500).unwrap_or_default()
        } else {
            String::new()
        }
    } else {
        String::new()
    };
    let latest_line_budget = if using_crash_file { 900 } else { 2500 };
    let latest_log = if latest.is_file() {
        tuffbox_core::process::read_log_tail(&latest, latest_line_budget).unwrap_or_default()
    } else {
        String::new()
    };
    // Primary excerpt source: explicit crash file > launcher log > latest.log.
    let (crash_excerpt_budget, latest_excerpt_budget) = if using_crash_file {
        (6000usize, 4000usize)
    } else if using_launcher_log {
        (0usize, 0usize) // sentinel: launcher log carries the content below
    } else {
        (800usize, 7000usize)
    };

    let jv = manifest
        .java
        .as_ref()
        .and_then(|j| j.path.clone())
        .unwrap_or_default();
    let java_version = if !jv.is_empty() {
        tuffbox_core::jre::check_java_at_path(&PathBuf::from(&jv))
            .map(|r| r.version)
            .unwrap_or_default()
    } else {
        "unknown".into()
    };

    let loader = format!("{:?}", manifest.loader.kind).to_lowercase();
    // Primary analysis text for the fingerprint/haystack: crash file, then
    // launcher log tail, else latest.log only.
    let primary_log = if !crash_content.is_empty() {
        crash_content.clone()
    } else if using_launcher_log && !launcher_tail.is_empty() {
        launcher_tail.clone()
    } else {
        latest_log.clone()
    };
    let ctx = tuffbox_core::crash_assistant::AnalysisCtx {
        crash_content: vec![crash_content.clone()],
        latest_log: latest_log.clone(),
        launcher_log: launcher_tail.clone(),
        installed_mods: manifest.mods.iter().map(|m| m.id.clone()).collect(),
        installed_versions: Some(
            manifest
                .mods
                .iter()
                .map(|m| (m.id.clone(), m.version.clone()))
                .collect(),
        ),
        previous_mods: Vec::new(),
        java_version: java_version.clone(),
        java_vendor: String::new(),
        os_name: std::env::consts::OS.to_string(),
        mc_version: manifest.minecraft.version.clone(),
        loader: loader.clone(),
        loader_version: manifest.loader.version.clone(),
        cpu_name: String::new(),
        gpu_names: Vec::new(),
        total_ram_mb: 0,
        is_offline: false,
        win_events: Vec::new(),
        combined_lines: std::cell::OnceCell::new(),
    };
    // Reuse the base diagnosis already computed by get_crash_diagnosis for
    // the same inputs (same mtime-keyed cache entry). Rebuilding it here
    // doubled the full log/archive/graph pass on every cold AI explain —
    // the second-largest cold-path cost after the full-file log reads.
    let diagnosis_cache_key = crash_diagnosis_cache_key(path, report_id);
    let diagnosis =
        match tuffbox_core::api_cache::get::<tuffbox_core::crash::CrashDiagnosis>(
            &diagnosis_cache_key,
        ) {
            Some(cached) => cached,
            None => tuffbox_core::crash::build_crash_diagnosis(
                &project_dir,
                &manifest,
                report_id,
                Vec::new(),
            )
            .map_err(|e| e.to_string())?,
        };

    let report = tuffbox_core::crash_assistant::run_full_analysis(&ctx);

    let blame_ids: Vec<String> = diagnosis
        .suspected_mods
        .iter()
        .filter(|s| {
            matches!(
                s.blame_role,
                tuffbox_core::crash::BlameRole::Primary | tuffbox_core::crash::BlameRole::Secondary
            ) || s.confidence >= 80
        })
        .take(3)
        .map(|s| s.id.clone())
        .collect();

    let haystack = format!("{crash_content}\n{latest_log}\n{launcher_tail}");
    let fingerprint = tuffbox_core::crash_kb::fingerprint_from_text_with_blame(
        &primary_log,
        &manifest.minecraft.version,
        &loader,
        &blame_ids,
    );
    let kb_cases = tuffbox_core::crash_kb::load_all_cases(&project_dir);
    let similar = tuffbox_core::crash_kb::search_similar(&kb_cases, &fingerprint, &haystack, 5);

    let inventory =
        tuffbox_core::project_ai_inventory::collect_project_ai_inventory(&project_dir, &manifest);
    let culprit_details: Vec<tuffbox_core::ai_explanation::CrashAiCulprit> = diagnosis
        .suspected_mods
        .iter()
        .take(8)
        .map(|s| tuffbox_core::ai_explanation::CrashAiCulprit {
            id: s.id.clone(),
            name: s.name.clone(),
            confidence: s.confidence,
            authors: s.authors.clone(),
            blame_role: match s.blame_role {
                tuffbox_core::crash::BlameRole::Primary => "primary".into(),
                tuffbox_core::crash::BlameRole::Secondary => "secondary".into(),
                tuffbox_core::crash::BlameRole::Related => "related".into(),
            },
            match_sources: s.match_sources.clone(),
            evidence: s
                .evidence
                .iter()
                .take(3)
                .map(|e| e.text.clone())
                .collect(),
        })
        .collect();

    let mut installed_sample: Vec<String> = culprit_details.iter().map(|c| c.id.clone()).collect();
    if installed_sample.is_empty() {
        installed_sample = report.suspected_mods.clone();
    }
    for id in inventory.mods.iter().map(|m| m.id.clone()) {
        if installed_sample.len() >= 24 {
            break;
        }
        if !installed_sample.iter().any(|s| s.eq_ignore_ascii_case(&id)) {
            installed_sample.push(id);
        }
    }

    // Pull recent crash-related history into the prompt so the model sees what
    // the user already tried (and what was marked resolved).
    let recent_changes = recent_crash_history_lines(&project_dir, 12);

    let graph_diagnostics: Vec<String> = diagnosis
        .graph_diagnostics
        .iter()
        .take(12)
        .map(|d| format!("[{:?}] {}: {}", d.severity, d.code, d.message))
        .collect();

    let universe: Vec<String> = inventory.mods.iter().map(|m| m.id.clone()).collect();
    let group_test = swarm_api::load_group_test_session(&project_dir)
        .map(|s| tuffbox_core::ai_explanation::CrashAiGroupTest::from_session(&s));
    // Explain must not inject a synthetic Healthy — that would mark the current
    // (crashing) enabled set as clean. Resolve/publish still uses decode_player_share.
    let trail = pack_events::pack_events_to_trail(
        &project_dir,
        &pack_events::events_between_crash_and_resolve(&project_dir, &fingerprint.key),
    );
    let has_toggles = trail.iter().any(|e| {
        matches!(
            e.kind,
            tuffbox_core::mod_group_test::TrailEventKind::Disable(_)
                | tuffbox_core::mod_group_test::TrailEventKind::Enable(_)
        )
    });
    let decoded = tuffbox_core::mod_group_test::decode_player_trail(&universe, &trail);
    let trail_covering = if has_toggles || !decoded.covering.is_empty() || !decoded.clean.is_empty()
    {
        Some(tuffbox_core::ai_explanation::CrashAiTrailCovering::from_decoded(&decoded))
    } else {
        None
    };

    let ai_ctx = tuffbox_core::ai_explanation::CrashAiContext {
        mc_version: manifest.minecraft.version.clone(),
        loader: loader.clone(),
        loader_version: manifest.loader.version.clone(),
        java_version,
        os: std::env::consts::OS.to_string(),
        installed_mods: installed_sample,
        installed_mod_count: inventory.mods.len() as u32,
        crash_report_excerpt: if using_launcher_log {
            tuffbox_core::crash_kb::smart_excerpt(&launcher_tail, 6000)
        } else {
            tuffbox_core::crash_kb::smart_excerpt(&crash_content, crash_excerpt_budget)
        },
        latest_log_excerpt: tuffbox_core::crash_kb::smart_excerpt(
            &latest_log,
            latest_excerpt_budget,
        ),
        suspected_mods: culprit_details.iter().map(|c| c.id.clone()).collect(),
        culprit_details,
        crash_assistant_findings: tuffbox_core::ai_explanation::findings_to_ai(&report.findings),
        recent_changes,
        graph_diagnostics,
        similar_cases: similar,
        fingerprint_key: fingerprint.key.clone(),
        report_id: report_id.map(|s| s.to_string()),
        inventory: Some(inventory),
        group_test,
        trail_covering,
    };

    Ok((ai_ctx, fingerprint, haystack, report.findings.len()))
}

fn recent_crash_history_lines(project_dir: &Path, limit: usize) -> Vec<String> {
    // Prefer the pack activity journal (launcher + external scan + crash_fix).
    let mut lines = pack_events::recent_non_toggle_pack_change_lines(project_dir, limit);
    if lines.len() >= limit {
        return lines;
    }
    // Fallback / supplement: resolved crash fixes.
    if let Ok(entries) = swarm_api::list_crash_resolutions(project_dir) {
        for r in entries.into_iter().take(limit.saturating_sub(lines.len())) {
            lines.push(format!(
                "RESOLVED [{}] via {}: {}",
                r.fingerprint_key,
                r.verified_by,
                tuffbox_core::crash_kb::truncate_at_char_boundary(&r.human_explanation, 160)
            ));
        }
    }
    lines.truncate(limit);
    lines
}

#[tauri::command(rename_all = "camelCase")]
async fn build_ai_crash_context(
    path: String,
    report_id: Option<String>,
) -> Result<serde_json::Value, String> {
    // Task #66 pattern: heavy log/jar/inventory work must not occupy the
    // async IPC executor — run the preparation on the blocking pool.
    let (ai_ctx, _fingerprint, _haystack, findings_count) =
        tokio::task::spawn_blocking(move || {
            prepare_ai_crash_context(&path, report_id.as_deref())
        })
        .await
        .map_err(|e| e.to_string())??;
    let prompt = tuffbox_core::ai_explanation::build_crash_prompt(&ai_ctx);
    let triage = tuffbox_core::ai_explanation::build_triage_prompt(&ai_ctx);
    let settings = integrations::get_integration_status().settings;

    // Do not ship the full inventory blob to the webview — counts + prompt are enough.
    // Keeps IPC small and avoids UI freezes on big packs.
    let mod_count = ai_ctx.installed_mod_count;
    let config_count = ai_ctx
        .inventory
        .as_ref()
        .map(|i| i.config_files.len())
        .unwrap_or(0);
    let pack_count = ai_ctx
        .inventory
        .as_ref()
        .map(|i| i.resourcepacks.len() + i.datapacks.len() + i.shaderpacks.len())
        .unwrap_or(0);
    let similar_case_count = ai_ctx.similar_cases.len();
    let fingerprint_key = ai_ctx.fingerprint_key.clone();
    let ui_ctx = ai_ctx;

    Ok(serde_json::json!({
        "context": ui_ctx,
        "prompt": prompt,
        "triagePrompt": triage,
        "promptLength": prompt.len(),
        "findingsCount": findings_count,
        "similarCaseCount": similar_case_count,
        "fingerprintKey": fingerprint_key,
        "aiProvider": settings.ai.provider,
        "aiModel": settings.ai.model,
        "aiEndpoint": settings.ai.endpoint,
        "diagnoseMode": settings.ai.diagnose_mode,
        "crashKbEndpoint": settings.ai.crash_kb_endpoint,
        "inventorySummary": {
            "mods": mod_count,
            "configs": config_count,
            "packs": pack_count,
        },
    }))
}

/// Single-flight for the AI cascade: one run per (path, report_id) at a time.
/// A second "AI explain" click (or a watchdog reset followed by retry) used
/// to spawn a full parallel cascade — double the network wait, double the
/// CPU — whose late result the frontend generation guard discarded anyway.
/// Concurrent callers now share the in-flight run's result.
static AI_CASCADE_INFLIGHT: std::sync::LazyLock<
    std::sync::Mutex<
        std::collections::HashMap<
            String,
            std::sync::Arc<tokio::sync::OnceCell<Result<serde_json::Value, String>>>,
        >,
    >,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

#[tauri::command(rename_all = "camelCase")]
async fn analyze_crash_with_ai(
    app: tauri::AppHandle,
    path: String,
    report_id: Option<String>,
) -> Result<serde_json::Value, String> {
    let key = format!("{}:{:?}", path, report_id);
    let cell = {
        let mut map = AI_CASCADE_INFLIGHT
            .lock()
            .map_err(|_| "ai cascade lock poisoned".to_string())?;
        map.entry(key.clone())
            .or_insert_with(|| std::sync::Arc::new(tokio::sync::OnceCell::new()))
            .clone()
    };
    let app_for_run = app.clone();
    let path_for_run = path.clone();
    let report_for_run = report_id.clone();
    let result = cell
        .get_or_init(|| async move {
            analyze_crash_with_ai_inner(app_for_run, path_for_run, report_for_run).await
        })
        .await;
    // Unlink the entry once the run has settled so a later call starts a
    // fresh cascade instead of replaying a stale result. Callers still
    // awaiting hold their own Arc clone and keep receiving this run.
    {
        let mut map = AI_CASCADE_INFLIGHT
            .lock()
            .map_err(|_| "ai cascade lock poisoned".to_string())?;
        if map
            .get(&key)
            .map(|c| std::sync::Arc::ptr_eq(c, &cell))
            .unwrap_or(false)
        {
            map.remove(&key);
        }
    }
    result.clone()
}

async fn analyze_crash_with_ai_inner(
    app: tauri::AppHandle,
    path: String,
    report_id: Option<String>,
) -> Result<serde_json::Value, String> {
    // Build structured context directly — avoid JSON round-trip panics / lossy deserialize.
    // Fingerprint MUST match prepare (with_blame) for the whole cascade.
    let project_dir = manifest_parent(&path)?;
    let (mut ai_ctx, fingerprint, haystack, _findings_count) =
        tokio::task::spawn_blocking(move || {
            prepare_ai_crash_context(&path, report_id.as_deref())
        })
        .await
        .map_err(|e| e.to_string())??;
    let settings = integrations::get_integration_status().settings;
    let mode = tuffbox_core::action_plan::DiagnoseMode::parse(&settings.ai.diagnose_mode);

    let swarm_on = integrations::swarm_enabled();
    // Hard cap on every network step of the cascade: a dead hub/P2P/supabase
    // must cost seconds, not minutes (Diagnose appeared stuck on L1/L2/L3).
    const NET_STEP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(12);
    let transport_bases = if swarm_on {
        tokio::time::timeout(
            std::time::Duration::from_secs(20),
            swarm_node::capsule_transport_bases(),
        )
        .await
        .unwrap_or_default()
    } else {
        Vec::new()
    };
    let online_kb = !transport_bases.is_empty();
    let mut network_used = false;
    let mut compact_prompt_used = false;
    let mut kb_short_circuit = false;
    let mut speculative_used = false;
    let mut speculative_draft_model: Option<String> = None;
    let mut fallback_notes: Vec<String> = Vec::new();
    // Assigned by every arm of the plan cascade below before its first read,
    // so it is declared without an initializer.
    let mut cascade_stage: String;
    let mut cascade_tried: Vec<String> = vec!["l1".into()];

    emit_diagnose_cascade(&app, "l1_searching");
    let cascade_started = std::time::Instant::now();

    // Enrich similar_cases from local capsule library + remote lookup (read-only).
    if swarm_on {
        // Library lookup reads + parses the whole capsule JSONL and verifies
        // signatures per hit — disk/CPU work that used to run directly on the
        // async runtime, stalling every other IPC (and the UI) while it ran.
        let fp_for_lookup = fingerprint.clone();
        let hay_for_lookup = haystack.clone();
        let global_hits = tokio::task::spawn_blocking(move || {
            integrations::global_capsule_library().lookup(&fp_for_lookup, &hay_for_lookup, 5)
        })
        .await
        .unwrap_or_default();
        if !global_hits.is_empty() {
            let mut merged = tuffbox_core::crash_remote::hits_to_similar_cases(&global_hits, &fingerprint);
            merged.extend(ai_ctx.similar_cases.drain(..));
            let mut seen = std::collections::HashSet::new();
            merged.retain(|h| seen.insert(h.id.clone()));
            ai_ctx.similar_cases = merged;
        }
    }
    if online_kb && !matches!(mode, tuffbox_core::action_plan::DiagnoseMode::KbOnly) {
        network_used = true;
        let req = tuffbox_core::crash_remote::CrashLookupRequest {
            fingerprint: fingerprint.clone(),
            excerpt: Some(tuffbox_core::crash_kb::smart_excerpt(&haystack, 2000)),
            mc_version: Some(ai_ctx.mc_version.clone()),
            loader: Some(ai_ctx.loader.clone()),
            limit: 5,
        };
        let l1_lookup_started = std::time::Instant::now();
        if let Ok(Some(resp)) = tokio::time::timeout(
            NET_STEP_TIMEOUT,
            swarm_node::lookup_across_transports(&req, &transport_bases),
        )
        .await
        {
            diagnose_timing(
                Some(&app),
                "ai_l1_remote_lookup_hit",
                l1_lookup_started,
                false,
            );
            let mut remote = tuffbox_core::crash_remote::hits_to_similar_cases(&resp.hits, &fingerprint);
            remote.extend(ai_ctx.similar_cases.drain(..));
            let mut seen = std::collections::HashSet::new();
            remote.retain(|h| seen.insert(h.id.clone()));
            ai_ctx.similar_cases = remote;
        }
    }

    let similar_count = ai_ctx.similar_cases.len() as u64;
    let inventory_ids: Vec<String> = ai_ctx
        .inventory
        .as_ref()
        .map(|inv| inv.mods.iter().map(|m| m.id.clone()).collect())
        .unwrap_or_default();
    let missing_ids =
        tuffbox_core::ai_explanation::missing_dep_hints_from_graph(&ai_ctx.graph_diagnostics);

    // ── L1: strong KB / capsule hit (free) ──────────────────────────
    // Candidates come from the capsule-library disk scan + signature verify
    // (same cost as the lookup above) — keep it off the async runtime too.
    let fp_for_l1 = fingerprint.clone();
    let hay_for_l1 = haystack.clone();
    let ctx_for_l1 = ai_ctx.clone();
    let l1_plan = tokio::task::spawn_blocking(move || {
        try_l1_strong_plan(&fp_for_l1, &hay_for_l1, &ctx_for_l1, swarm_on)
    })
    .await
    .unwrap_or(None);

    let mut plan = if let Some(plan) = l1_plan {
        cascade_stage = "l1_hit".into();
        kb_short_circuit = true;
        // Only count the network when the L1 hit actually came from the remote
        // (global capsule) library — a purely local hit must not flip
        // network_used, or the plan gets persisted as network-derived.
        network_used = online_kb || network_used;
        plan
    } else if matches!(mode, tuffbox_core::action_plan::DiagnoseMode::KbOnly) {
        // KbOnly: L1 enrichment via remote lookup only — no L2/L3 LLM.
        cascade_tried.push("kb_only".into());
        if online_kb {
            network_used = true;
            let req = tuffbox_core::crash_remote::CrashLookupRequest {
                fingerprint: fingerprint.clone(),
                excerpt: Some(tuffbox_core::crash_kb::smart_excerpt(&haystack, 2000)),
                mc_version: Some(ai_ctx.mc_version.clone()),
                loader: Some(ai_ctx.loader.clone()),
                limit: 1,
            };
            match tokio::time::timeout(
                NET_STEP_TIMEOUT,
                swarm_node::lookup_across_transports(&req, &transport_bases),
            )
            .await
            {
                Ok(Some(resp)) => {
                    let hit = resp.hits.first().ok_or_else(|| {
                        "no remote KB hits for this fingerprint".to_string()
                    })?;
                    cascade_stage = "l1_hit".into();
                    kb_short_circuit = true;
                    tuffbox_core::action_plan::plan_from_launcher_actions(
                        &hit.solution,
                        &hit.suspected_mods,
                        hit.actions.clone(),
                        &hit.id,
                        hit.score,
                    )
                }
                Ok(None) | Err(_) => {
                    let cases = tuffbox_core::crash_kb::load_all_cases(&project_dir);
                    let similar = tuffbox_core::crash_kb::search_similar(
                        &cases,
                        &fingerprint,
                        &haystack,
                        1,
                    );
                    let hit = similar.first().ok_or_else(|| {
                        "no local KB hits for this fingerprint".to_string()
                    })?;
                    cascade_stage = "l1_hit".into();
                    kb_short_circuit = true;
                    tuffbox_core::action_plan::plan_from_kb_hit(
                        &hit.solution,
                        &hit.suspected_mods,
                        &hit.actions,
                        &hit.id,
                        hit.score,
                    )
                }
            }
        } else {
            let cases = tuffbox_core::crash_kb::load_all_cases(&project_dir);
            let similar =
                tuffbox_core::crash_kb::search_similar(&cases, &fingerprint, &haystack, 1);
            let hit = similar
                .first()
                .ok_or_else(|| "no local KB hits for this fingerprint".to_string())?;
            cascade_stage = "l1_hit".into();
            kb_short_circuit = true;
            tuffbox_core::action_plan::plan_from_kb_hit(
                &hit.solution,
                &hit.suspected_mods,
                &hit.actions,
                &hit.id,
                hit.score,
            )
        }
    } else {
        // ── L2: Fog volunteer (opt-in P2P) — best-effort; miss → L3 ──
        cascade_tried.push("l2".into());
        emit_diagnose_cascade(&app, "l2_asking");
        let l2_started = std::time::Instant::now();
        let l2 = if swarm_on && settings.swarm.p2p_enabled {
            match tokio::time::timeout(
                NET_STEP_TIMEOUT,
                swarm_node::diagnose_via_volunteer(
                    &fingerprint,
                    &ai_ctx,
                    &tuffbox_core::crash_kb::smart_excerpt(&haystack, 4000),
                ),
            )
            .await
            {
                Ok(r) => r,
                Err(_) => Err("fog volunteer timed out".into()),
            }
        } else {
            Err("fog volunteer unavailable".into())
        };
        diagnose_timing(Some(&app), "ai_l2_volunteer", l2_started, false);

        match l2 {
            Ok(mut volunteer_plan) => {
                cascade_stage = "l2_hit".into();
                network_used = true;
                volunteer_plan.source = Some("swarm_volunteer".into());
                volunteer_plan
            }
            Err(miss) => {
                cascade_tried.push(format!("l2_miss:{}", truncate_cascade_miss(&miss)));
                // ── L3: existing DiagnoseMode LLM / server path ──────────
                cascade_tried.push("l3".into());
                emit_diagnose_cascade(&app, "l3_asking");
                match mode {
                tuffbox_core::action_plan::DiagnoseMode::Server if online_kb => {
                    network_used = true;
                    let req = tuffbox_core::crash_remote::CrashDiagnoseRequest {
                        fingerprint: fingerprint.clone(),
                        context: Some(serde_json::to_value(&ai_ctx).unwrap_or_default()),
                        excerpt: Some(tuffbox_core::crash_kb::smart_excerpt(&haystack, 4000)),
                        prefer_kb_only: false,
                    };
                    let l3_started = std::time::Instant::now();
                    let l3_out = match tokio::time::timeout(
                        NET_STEP_TIMEOUT,
                        swarm_node::diagnose_across_transports(&req, &transport_bases),
                    )
                    .await
                    {
                        Ok(Ok(resp)) => {
                            // Explain may read the network; MUST NOT persist peer capsules here.
                            cascade_stage = "l3_hit".into();
                            resp.plan
                        }
                        Ok(Err(remote_err)) => {
                            let (p, compact, note, spec) =
                                ai_plan_with_fallback(&settings.ai, &ai_ctx).await.map_err(
                                    |e| format!("server diagnose failed ({remote_err}); {e}"),
                                )?;
                            compact_prompt_used = compact;
                            speculative_used |= spec.used;
                            if let Some(m) = spec.draft_model {
                                speculative_draft_model = Some(m);
                            }
                            if let Some(n) = note {
                                fallback_notes.push(n);
                            }
                            cascade_stage = if note_is_heuristic(&fallback_notes) {
                                "heuristic".into()
                            } else {
                                "l3_hit".into()
                            };
                            p
                        }
                        Err(_) => {
                            // Transport timeout — fall through to local AI/heuristics.
                            let (p, compact, note, spec) =
                                ai_plan_with_fallback(&settings.ai, &ai_ctx).await?;
                            compact_prompt_used = compact;
                            speculative_used |= spec.used;
                            if let Some(m) = spec.draft_model {
                                speculative_draft_model = Some(m);
                            }
                            if let Some(n) = note {
                                fallback_notes.push(n);
                            }
                            cascade_stage = if note_is_heuristic(&fallback_notes) {
                                "heuristic".into()
                            } else {
                                "l3_hit".into()
                            };
                            p
                        }
                    };
                    diagnose_timing(Some(&app), "ai_l3_server", l3_started, false);
                    l3_out
                }
                tuffbox_core::action_plan::DiagnoseMode::Local
                | tuffbox_core::action_plan::DiagnoseMode::Server => {
                    let (p, compact, note, spec) =
                        ai_plan_with_fallback(&settings.ai, &ai_ctx).await?;
                    compact_prompt_used = compact;
                    speculative_used |= spec.used;
                    if let Some(m) = spec.draft_model {
                        speculative_draft_model = Some(m);
                    }
                    if let Some(n) = note {
                        fallback_notes.push(n);
                    }
                    cascade_stage = if note_is_heuristic(&fallback_notes) {
                        "heuristic".into()
                    } else {
                        "l3_hit".into()
                    };
                    p
                }
                tuffbox_core::action_plan::DiagnoseMode::KbOnly => {
                    // Defensive: KbOnly is fully handled before the L2/L3 block.
                    // Never panic the IPC thread from a cascade refactor.
                    return Err("internal: KbOnly mode reached L2/L3 cascade".into());
                }
            }
            }
        }
    };

    // Co-occurrence negative evidence (best-effort, non-fatal): pairs of mods
    // known to coexist in working packs let us SUPPRESS speculative conflict
    // claims instead of blindly downgrading them. Global Supabase pairs are
    // merged with the project's local co-occurrence store.
    let compat_pairs: Vec<tuffbox_core::action_plan::CoexistingPair> = {
        // Local co-occurrence store is a disk read (JSONL scan) — blocking pool.
        let dir_for_pairs = project_dir.clone();
        let mut pairs: Vec<tuffbox_core::swarm::ModPairStat> =
            tokio::task::spawn_blocking(move || {
                tuffbox_core::swarm::top_cooccurrence_pairs(&dir_for_pairs, 200)
            })
            .await
            .unwrap_or_default();
        if let (Some(url), Some(key)) = (
            integrations::swarm_supabase_url(),
            integrations::swarm_supabase_anon_key(),
        ) {
            if let Ok(net) = tokio::time::timeout(
                NET_STEP_TIMEOUT,
                tuffbox_core::swarm_supabase::fetch_cooccurrence_supabase(
                    &url,
                    &key,
                    &ai_ctx.mc_version,
                    &ai_ctx.loader,
                    200,
                ),
            )
            .await
            .unwrap_or(Err("cooccurrence timed out".into()))
            {
                pairs = tuffbox_core::swarm::merge_cooccurrence_pairs(&pairs, &net, 200);
            }
        }
        pairs
            .into_iter()
            .map(|p| tuffbox_core::action_plan::CoexistingPair {
                a: p.mod_a,
                b: p.mod_b,
                count: p.count,
            })
            .collect()
    };

    // Inventory grounding + Crash Assistant overlay (all modes).
    let grounded = tuffbox_core::action_plan::ground_action_plan_with_compat(
        plan,
        &inventory_ids,
        &missing_ids,
        &compat_pairs,
    );
    let mut normalize_notes = grounded.notes;
    normalize_notes.extend(fallback_notes);
    plan = tuffbox_core::action_plan::overlay_crash_assistant_findings(
        grounded.plan,
        &ai_ctx.crash_assistant_findings,
    );
    if plan.source.is_none() {
        plan.source = Some(if kb_short_circuit {
            "kb".into()
        } else if cascade_stage == "l2_hit" {
            "swarm_volunteer".into()
        } else {
            "ai".into()
        });
    }

    // Pending-plan persist writes to disk (snapshot dir) — keep the write off
    // the async runtime so late-stage I/O cannot stall the IPC response.
    let dir_for_pending = project_dir.clone();
    let plan_for_pending = plan.clone();
    let pending_path = tokio::task::spawn_blocking(move || {
        swarm_api::maybe_persist_pending_from_plan(&dir_for_pending, &plan_for_pending, network_used)
    })
    .await
    .unwrap_or_else(|_| {
        // JoinError: the write is best-effort telemetry — lose it, not the plan.
        None
    });
    let validation = tuffbox_core::action_plan::validate_action_plan_with_inventory_and_compat(
        &plan,
        &inventory_ids,
        &missing_ids,
        &compat_pairs,
    );
    let legacy = tuffbox_core::action_plan::plan_to_legacy_ai_actions(&plan);
    diagnose_timing(
        Some(&app),
        "ai_cascade_total",
        cascade_started,
        kb_short_circuit,
    );

    Ok(serde_json::json!({
        "schemaVersion": plan.schema_version,
        "humanExplanation": plan.human_explanation,
        "human_explanation": plan.human_explanation,
        "confidence": plan.confidence,
        "suspectedMods": plan.suspected_mods,
        "suspected_mods": plan.suspected_mods,
        "needsUserReview": plan.needs_user_review,
        "needs_user_review": plan.needs_user_review,
        "source": plan.source,
        "matchedCaseIds": plan.matched_case_ids,
        "actions": plan.actions,
        "recommendedActions": legacy,
        "recommended_actions": legacy,
        "additionalContext": plan.additional_context,
        "validation": validation,
        "diagnoseMode": mode.as_str(),
        "provider": settings.ai.provider,
        "model": settings.ai.model,
        "similarCaseCount": similar_count,
        "fingerprintKey": fingerprint.key,
        "swarmEnabled": swarm_on,
        "networkUsed": network_used,
        "compactPromptUsed": compact_prompt_used,
        "kbShortCircuit": kb_short_circuit,
        "cascadeStage": cascade_stage,
        "cascadeTried": cascade_tried,
        "speculativeUsed": speculative_used,
        "speculativeDraftModel": speculative_draft_model,
        "normalizeNotes": normalize_notes,
        "pendingPlanPath": pending_path.map(|p| p.to_string_lossy().to_string()),
    }))
}

fn emit_diagnose_cascade(app: &tauri::AppHandle, stage: &str) {
    use tauri::Emitter;
    // Mirror into the diagnose task bus so the progress panel shows the AI
    // stage (searching KB / asking model / writing plan) instead of a bar.
    tuffbox_core::task_progress::set_running_stage(
        DIAGNOSE_TASK_ID,
        &format!("AI: {}", diagnose_cascade_label(stage)),
    );
    let _ = app.emit(
        "diagnose-cascade",
        serde_json::json!({ "stage": stage }),
    );
}

/// Human label for a cascade stage (mirrors DiagnoseStatusBar.cascadeHint).
fn diagnose_cascade_label(stage: &str) -> &str {
    match stage {
        "l1_searching" => "searching known fixes…",
        "l1_hit" => "known fix from network/KB",
        "l2_asking" => "asking a community volunteer…",
        "l2_hit" => "plan from community volunteer",
        "l3_asking" => "generating with AI…",
        "l3_hit" => "AI-generated plan",
        "heuristic" => "local heuristic plan",
        _ => "analyzing…",
    }
}

fn truncate_cascade_miss(msg: &str) -> String {
    let flat: String = msg
        .chars()
        .map(|c| if c.is_control() || c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    let flat = flat.trim();
    const MAX: usize = 96;
    if flat.chars().count() <= MAX {
        flat.to_string()
    } else {
        format!("{}…", flat.chars().take(MAX).collect::<String>())
    }
}

fn try_l1_strong_plan(
    fingerprint: &tuffbox_core::crash_kb::CrashFingerprint,
    haystack: &str,
    ai_ctx: &tuffbox_core::ai_explanation::CrashAiContext,
    swarm_on: bool,
) -> Option<tuffbox_core::action_plan::ActionPlan> {
    if swarm_on {
        if let Some(plan) = integrations::global_capsule_library()
            .diagnose_best(fingerprint, haystack)
            .filter(|p| p.confidence >= tuffbox_core::swarm::STRONG_MATCH_THRESHOLD)
        {
            return Some(plan);
        }
    }
    strong_plan_from_similar(ai_ctx)
}

fn note_is_heuristic(notes: &[String]) -> bool {
    notes.iter().any(|n| {
        let lower = n.to_lowercase();
        lower.contains("heuristic") || lower.contains("fallback")
    })
}

fn strong_plan_from_similar(
    ctx: &tuffbox_core::ai_explanation::CrashAiContext,
) -> Option<tuffbox_core::action_plan::ActionPlan> {
    let hit = ctx
        .similar_cases
        .iter()
        .max_by(|a, b| {
            a.score
                .partial_cmp(&b.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        })?;
    // Strong KB fallback requires an ANCHOR (fingerprint-key / blamed-mod /
    // mod-file agreement), not just a score over the threshold: the score is
    // additive over generic signals (exception wording, common frames,
    // capped popularity), which used to let an unrelated popular case about
    // a generic NullPointerException become the "strong match" plan.
    if !hit.anchored || hit.score < tuffbox_core::swarm::STRONG_MATCH_THRESHOLD {
        return None;
    }
    let mut plan = tuffbox_core::action_plan::plan_from_kb_hit(
        &hit.solution,
        &hit.suspected_mods,
        &hit.actions,
        &hit.id,
        hit.score,
    );
    plan.source = Some("kb".into());
    Some(plan)
}

/// When Ollama/API is down, still return an actionable plan from local culprits.
fn heuristic_plan_from_context(
    ctx: &tuffbox_core::ai_explanation::CrashAiContext,
) -> Option<tuffbox_core::action_plan::ActionPlan> {
    let mut suspected = ctx.suspected_mods.clone();
    for c in &ctx.culprit_details {
        if c.confidence >= 40 && !suspected.iter().any(|s| s.eq_ignore_ascii_case(&c.id)) {
            suspected.push(c.id.clone());
        }
    }
    suspected.truncate(5);
    if suspected.is_empty() && ctx.crash_assistant_findings.is_empty() {
        return None;
    }

    let explanation = if let Some(f) = ctx.crash_assistant_findings.first() {
        let auto = f.auto_fix.as_deref().unwrap_or("");
        format!(
            "{} — {}{}",
            f.title,
            f.description,
            if auto.is_empty() {
                String::new()
            } else {
                format!(" Suggested: {auto}")
            }
        )
    } else {
        format!(
            "Local crash analysis flags: {}. AI was unavailable — review before applying.",
            suspected.join(", ")
        )
    };

    let actions = suspected
        .iter()
        .take(3)
        .map(|id| tuffbox_core::action_plan::LauncherAction {
            op: "disable_mod".into(),
            mod_id: Some(id.clone()),
            provider: None,
            project_id: None,
            version: None,
            path: None,
            patch_type: None,
            patch: None,
            reason: Some(format!(
                "Heuristic: disable high-confidence culprit `{id}` to isolate the crash"
            )),
            risk: "medium".into(),
        })
        .collect::<Vec<_>>();

    Some(tuffbox_core::action_plan::ActionPlan {
        schema_version: tuffbox_core::action_plan::ACTION_PLAN_SCHEMA_VERSION,
        human_explanation: explanation,
        confidence: if actions.is_empty() { 0.35 } else { 0.48 },
        suspected_mods: suspected,
        needs_user_review: true,
        source: Some("heuristic".into()),
        matched_case_ids: Vec::new(),
        actions,
        additional_context: Some(
            "Generated without AI (Ollama/API unavailable or failed). Prefer configuring AI in Settings for better plans."
                .into(),
        ),
    })
}

/// Prefer AI; on failure fall back to strong KB hit, then local heuristics.
async fn ai_plan_with_fallback(
    settings: &integrations::AiSettings,
    ctx: &tuffbox_core::ai_explanation::CrashAiContext,
) -> Result<
    (
        tuffbox_core::action_plan::ActionPlan,
        bool, /*compact*/
        Option<String>,
        speculative::SpeculativeMeta,
    ),
    String,
> {
    let (prompt, compact) = integrations::crash_explain_prompt_for(settings, ctx);
    // Tell the user what the model is doing right now (it can take minutes
    // on local Ollama): building the request → waiting for the answer.
    let stage_msg = if settings.provider == "ollama" {
        format!(
            "AI: waiting for local model '{}' (may take minutes)…",
            settings.model
        )
    } else {
        format!("AI: model '{}' is writing the fix plan…", settings.model)
    };
    tuffbox_core::task_progress::set_running_stage(DIAGNOSE_TASK_ID, &stage_msg);
    // Hard overall deadline so a stuck local/remote AI (or its HTTP client)
    // can never leave the Diagnose IPC pending forever.
    let ai_call = tokio::time::timeout(
        std::time::Duration::from_secs(150),
        integrations::call_ai_crash_explain_detailed(settings, &prompt),
    )
    .await
    .map_err(|_| {
        "AI call timed out after 150s — the local/remote model did not respond. Check Settings → AI and retry.".to_string()
    })?;
    match ai_call {
        Ok(detailed) => {
            let raw = serde_json::to_string(&detailed.value).unwrap_or_default();
            match tuffbox_core::action_plan::parse_action_plan(&raw) {
                Ok(mut plan) => {
                    tuffbox_core::action_plan::veto_content_vs_optimization(&mut plan);
                    Ok((plan, compact, None, detailed.speculative))
                }
                // The model ANSWERED, but not with a usable plan: JSON
                // truncated by a token limit, prose around the object, wrong
                // shape — very common with small local models. The fallback
                // chain below exists exactly for "no usable AI plan", yet it
                // used to run only on transport errors, so a garbage answer
                // surfaced as a hard error instead of the KB/heuristic plan.
                Err(parse_err) => fallback_plan_without_ai(
                    ctx,
                    compact,
                    format!("AI answer was not a usable plan ({parse_err})"),
                ),
            }
        }
        Err(ai_err) => fallback_plan_without_ai(ctx, compact, format!("AI unavailable ({ai_err})")),
    }
}

/// Strong-KB → local-heuristics fallback used when the AI path yields no
/// usable plan — transport failure OR an answer that failed to parse.
fn fallback_plan_without_ai(
    ctx: &tuffbox_core::ai_explanation::CrashAiContext,
    compact: bool,
    reason: String,
) -> Result<
    (
        tuffbox_core::action_plan::ActionPlan,
        bool, /*compact*/
        Option<String>,
        speculative::SpeculativeMeta,
    ),
    String,
> {
    if let Some(mut plan) = strong_plan_from_similar(ctx) {
        tuffbox_core::action_plan::veto_content_vs_optimization(&mut plan);
        return Ok((
            plan,
            compact,
            Some(format!("{reason}; used strong KB match")),
            speculative::SpeculativeMeta::default(),
        ));
    }
    if let Some(mut plan) = heuristic_plan_from_context(ctx) {
        tuffbox_core::action_plan::veto_content_vs_optimization(&mut plan);
        return Ok((
            plan,
            compact,
            Some(format!("{reason}; used local crash heuristics")),
            speculative::SpeculativeMeta::default(),
        ));
    }
    Err(format!(
        "{reason}. Configure Ollama or an OpenAI-compatible endpoint in Settings → AI, or enable Crash KB / TuffSwarm."
    ))
}

/// Apply a validated ActionPlan (after user confirm). Runs snapshot once, then each op.
#[tauri::command(rename_all = "camelCase")]
async fn apply_action_plan(
    app: tauri::AppHandle,
    path: String,
    plan: tuffbox_core::action_plan::ActionPlan,
    fingerprint_key: Option<String>,
) -> Result<serde_json::Value, String> {
    let manifest_path = resolve_manifest_path(&path)?;
    let path_str = manifest_path.to_string_lossy().to_string();
    let (inventory_ids, missing_ids) = {
        let manifest =
            ProjectManifest::load_from_path(&manifest_path).map_err(|e| e.to_string())?;
        let inventory_ids = tuffbox_core::swarm::pack_mod_ids(&manifest);
        let graph = DependencyGraph::from_manifest(&manifest);
        let diagnostics = Resolver::analyze_project(&manifest, &graph);
        let mut missing_ids = diagnostics
            .iter()
            .filter(|d| d.code == "MISSING_DEPENDENCY")
            .filter_map(|d| d.related_nodes.last())
            .filter_map(|id| id.0.strip_prefix("mod:").map(|s| s.to_string()))
            .collect::<Vec<_>>();
        missing_ids.sort();
        missing_ids.dedup();
        (inventory_ids, missing_ids)
    };
    let grounded =
        tuffbox_core::action_plan::ground_action_plan(plan, &inventory_ids, &missing_ids);
    let plan = grounded.plan;
    let validation = tuffbox_core::action_plan::validate_action_plan_with_inventory(
        &plan,
        &inventory_ids,
        &missing_ids,
    );
    if !validation.ok {
        return Err(format!(
            "ActionPlan validation failed: {}",
            validation.errors.join("; ")
        ));
    }
    let snapshot = swarm_api::auto_snapshot_crash_fix(
        &manifest_path,
        &plan,
        fingerprint_key.as_deref(),
    )?;

    let mut applied = Vec::new();
    let mut errors = Vec::new();

    for action in &plan.actions {
        if action.op == "edit_config" {
            match apply_launcher_edit_config(&manifest_path, action) {
                Ok(msg) => applied.push(msg),
                Err(e) => errors.push(e),
            }
            continue;
        }

        // Version-pinned update: resolve target, download jar, then save manifest.
        // Plain `update_mod` without version still goes through update-to-latest below.
        let version_pin = action
            .version
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty());
        if action.op == "change_mod_version"
            || (action.op == "update_mod" && version_pin.is_some())
        {
            let mod_id = action
                .mod_id
                .clone()
                .or_else(|| action.project_id.clone())
                .unwrap_or_default();
            let version = version_pin.unwrap_or("").to_string();
            if mod_id.is_empty() || version.is_empty() {
                errors.push("change_mod_version requires modId and version".into());
                continue;
            }
            let mut manifest =
                ProjectManifest::load_from_path(&manifest_path).map_err(|e| e.to_string())?;
            let old_mod = match manifest.mods.iter().find(|m| {
                m.id == mod_id || m.source.project_id.as_deref() == Some(mod_id.as_str())
            }) {
                Some(m) => m.clone(),
                None => {
                    errors.push(format!("mod {mod_id} not found in project"));
                    continue;
                }
            };
            match update_mod_from_modrinth(
                &manifest_path,
                &mut manifest,
                &mod_id,
                Some(version.as_str()),
            ) {
                Ok(()) => {
                    match commit_single_mod_update(
                        &app,
                        &manifest_path,
                        &mut manifest,
                        &old_mod,
                        false,
                    ) {
                        Ok(_) => {
                            applied.push(format!("changed {mod_id} to {version}"));
                        }
                        Err(e) => errors.push(e),
                    }
                }
                Err(e) => errors.push(e.to_string()),
            }
            continue;
        }

        if let Some(fix) = tuffbox_core::action_plan::launcher_action_to_fix_action(action) {
            match apply_fix_action(app.clone(), path_str.clone(), fix).await {
                Ok(msg) => applied.push(msg),
                Err(e) => errors.push(e),
            }
        } else {
            errors.push(format!("cannot map op '{}' to a fix action", action.op));
        }
    }

    // Soft-verify / distill need a pending fix marker even when apply used ActionPlan path.
    if !applied.is_empty() {
        let explanation = if plan.human_explanation.trim().is_empty() {
            format!("Applied ActionPlan: {}", applied.join("; "))
        } else {
            plan.human_explanation.clone()
        };
        let _ = swarm_api::record_user_fix_attempt(
            &manifest_path,
            plan.source.as_deref().unwrap_or("ai_action_plan"),
            &explanation,
            plan.actions.clone(),
            fingerprint_key.as_deref(),
        );
    }

    // Record co-occurrence after successful crash-fix apply (local + optional Supabase).
    if errors.is_empty() {
        let _ = swarm_api::record_and_upload_cooccurrence(&path_str, &[], "crash_fix_apply").await;
    }

    Ok(serde_json::json!({
        "applied": applied,
        "errors": errors,
        "ok": errors.is_empty(),
        "snapshotId": snapshot.id,
        "snapshotTags": snapshot.tags,
    }))
}

fn apply_launcher_edit_config(
    manifest_path: &Path,
    action: &tuffbox_core::action_plan::LauncherAction,
) -> Result<String, String> {
    let relative = action
        .path
        .as_deref()
        .ok_or_else(|| "edit_config missing path".to_string())?;
    let project_dir = manifest_path
        .parent()
        .ok_or_else(|| "manifest has no parent".to_string())?;
    let target = safe_project_file(project_dir, relative)?;
    // Defense in depth: validation already rejects non-config paths, but the
    // plan comes from an AI model — a `replace_file` text patch must never
    // overwrite a jar/binary/script inside the project, even if a crafted or
    // older plan slips past validation.
    if !is_editable_config_path(&target) {
        return Err(format!(
            "edit_config refused: '{relative}' is not a config file (allowed: json/json5/toml/properties/cfg/conf/txt/js/zs/yaml/yml/md)"
        ));
    }
    let current = if target.is_file() {
        std::fs::read_to_string(&target).map_err(|e| e.to_string())?
    } else {
        String::new()
    };
    let patch_type = action.patch_type.as_deref().unwrap_or("replace_file");
    let patch = action
        .patch
        .as_ref()
        .ok_or_else(|| "edit_config missing patch".to_string())?;
    let new_content =
        tuffbox_core::action_plan::apply_config_patch(&current, relative, patch_type, patch)?;
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    // Atomic write + .bak-<ts> backup (Reality Launcher-style).
    tuffbox_core::fs_util::atomic_write_with_backup(&target, new_content.as_bytes())
        .map_err(|e| e.to_string())?;
    let rel = relative.replace('\\', "/");
    let _ = pack_events::record_mod_change_event(
        project_dir,
        "edit-config",
        None,
        &[format!("edited config {rel}")],
        &[rel],
        &[],
    );
    Ok(format!("edited config {relative}"))
}

/// Record Helped/Wrong feedback into the project crash knowledge base.
#[tauri::command(rename_all = "camelCase")]
async fn record_crash_ai_feedback(
    path: String,
    feedback: CrashAiFeedbackPayload,
) -> Result<String, String> {
    // Blocking-pool conversion: sync commands run on the main thread (see
    // get_health_report); heavy I/O must not freeze IPC delivery.
    tokio::task::spawn_blocking(move || record_crash_ai_feedback_impl(path, feedback))
        .await
        .map_err(|e| e.to_string())?
}

fn record_crash_ai_feedback_impl(
    path: String,
    feedback: CrashAiFeedbackPayload,
) -> Result<String, String> {
    let project_dir = manifest_parent(&path)?;
    let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
    let loader = format!("{:?}", manifest.loader.kind).to_lowercase();
    // Mirror prepare_ai_crash_context: the analyze path fingerprints
    // crash-report + latest.log WITH blame ids, so feedback recorded under a
    // different key would never match in KB search. When the client did not
    // send the exact analyze-time key, rebuild it the same way.
    let crash = load_scoped_crash_report(&project_dir, feedback.report_id.as_deref())
        .unwrap_or_default();
    let latest = project_dir.join("logs").join("latest.log");
    let latest_log = if latest.is_file() {
        tuffbox_core::process::read_log_tail(&latest, 2500).unwrap_or_default()
    } else {
        String::new()
    };
    let mut fp = tuffbox_core::crash_kb::fingerprint_from_text_with_blame(
        &format!("{crash}\n{latest_log}"),
        &manifest.minecraft.version,
        &loader,
        &feedback.suspected_mods.as_deref().unwrap_or(&[]),
    );
    if let Some(key) = feedback.fingerprint_key.filter(|k| !k.is_empty()) {
        fp.key = key;
    }
    let actions = feedback.recommended_actions.unwrap_or_default();
    let mods = feedback.suspected_mods.clone().unwrap_or_default();
    let path = tuffbox_core::crash_kb::record_feedback(
        &project_dir,
        &fp,
        feedback.helped,
        feedback.human_explanation.as_deref(),
        &actions,
        &mods,
    )?;
    Ok(path.to_string_lossy().to_string())
}

/// Author a private KB case from the current crash + your resolution.
#[tauri::command(rename_all = "camelCase")]
async fn save_authored_crash_case(
    path: String,
    input: tuffbox_core::crash_kb::AuthorCaseInput,
) -> Result<tuffbox_core::crash_kb::AuthorCaseSaveResult, String> {
    // Blocking-pool conversion: sync commands run on the main thread; KB
    // reads/writes must not freeze IPC delivery.
    tokio::task::spawn_blocking(move || save_authored_crash_case_impl(path, input))
        .await
        .map_err(|e| e.to_string())?
}

fn save_authored_crash_case_impl(
    path: String,
    input: tuffbox_core::crash_kb::AuthorCaseInput,
) -> Result<tuffbox_core::crash_kb::AuthorCaseSaveResult, String> {
    let project_dir = manifest_parent(&path)?;
    tuffbox_core::crash_kb::save_authored_case(&project_dir, input)
}

/// Prefill author form: fingerprint + optional draft from AI analysis / report.
#[tauri::command(rename_all = "camelCase")]
async fn draft_authored_crash_case(
    path: String,
    report_id: Option<String>,
) -> Result<serde_json::Value, String> {
    // Blocking-pool conversion: sync commands run on the main thread; KB
    // reads/writes must not freeze IPC delivery.
    tokio::task::spawn_blocking(move || draft_authored_crash_case_impl(path, report_id))
        .await
        .map_err(|e| e.to_string())?
}

fn draft_authored_crash_case_impl(
    path: String,
    report_id: Option<String>,
) -> Result<serde_json::Value, String> {
    let project_dir = manifest_parent(&path)?;
    let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
    let loader = format!("{:?}", manifest.loader.kind).to_lowercase();
    let crash =
        load_scoped_crash_report(&project_dir, report_id.as_deref()).unwrap_or_default();
    let latest = project_dir.join("logs").join("latest.log");
    let latest_log = if latest.is_file() {
        tuffbox_core::process::read_log_tail(&latest, 900).unwrap_or_default()
    } else {
        String::new()
    };
    let haystack = format!("{crash}\n{latest_log}");
    let fingerprint = tuffbox_core::crash_kb::fingerprint_from_text(
        &haystack,
        &manifest.minecraft.version,
        &loader,
    );
    let symptoms: Vec<String> = [
        fingerprint.exception.clone(),
        fingerprint.mixin.clone().unwrap_or_default(),
        fingerprint.mod_file.clone().unwrap_or_default(),
    ]
    .into_iter()
    .filter(|s| !s.trim().is_empty())
    .collect();

    Ok(serde_json::json!({
        "fingerprint": fingerprint,
        "symptoms": symptoms,
        "mcVersion": manifest.minecraft.version,
        "loader": loader,
        "reportId": report_id,
        "authoredCount": tuffbox_core::crash_kb::list_authored_cases(&project_dir).len(),
    }))
}

#[tauri::command(rename_all = "camelCase")]
async fn list_authored_crash_cases(path: String) -> Result<Vec<tuffbox_core::crash_kb::CrashCase>, String> {
    // Blocking-pool conversion: sync commands run on the main thread; KB
    // reads/writes must not freeze IPC delivery.
    tokio::task::spawn_blocking(move || list_authored_crash_cases_impl(path))
        .await
        .map_err(|e| e.to_string())?
}

fn list_authored_crash_cases_impl(path: String) -> Result<Vec<tuffbox_core::crash_kb::CrashCase>, String> {
    let project_dir = manifest_parent(&path)?;
    Ok(tuffbox_core::crash_kb::list_authored_cases(&project_dir))
}

#[tauri::command(rename_all = "camelCase")]
async fn get_authored_case_export(path: String, case_id: String) -> Result<String, String> {
    // Blocking-pool conversion: sync commands run on the main thread; KB
    // reads/writes must not freeze IPC delivery.
    tokio::task::spawn_blocking(move || get_authored_case_export_impl(path, case_id))
        .await
        .map_err(|e| e.to_string())?
}

fn get_authored_case_export_impl(path: String, case_id: String) -> Result<String, String> {
    let project_dir = manifest_parent(&path)?;
    let case = tuffbox_core::crash_kb::list_authored_cases(&project_dir)
        .into_iter()
        .find(|c| c.id == case_id)
        .ok_or_else(|| format!("authored case not found: {case_id}"))?;
    let public = tuffbox_core::crash_kb::public_case_for_export(&case);
    serde_json::to_string_pretty(&public).map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
#[allow(deprecated)]
fn open_authored_kb_folder(_app: tauri::AppHandle, path: String) -> Result<(), String> {
    let project_dir = manifest_parent(&path)?;
    let dir = tuffbox_core::crash_kb::author_export_dir(&project_dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // Same detached-spawn helper as open_project_folder — shell plugin's
    // CoInitialize/ShellExecuteExW path aborted the app on Windows.
    file_manager::open_in_file_manager(&dir, None)
}

/// ── Mod recommendation engine ─────────────────────────────────────

/// Lowercase alphanumeric-only token so `modernfix-mvus` / `ModernFix mVUS`
/// / `modernfix-neoforge-5.20.9.jar` collapse to comparable forms.
fn compact_mod_token(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

fn installed_mod_keys(manifest: &ProjectManifest) -> std::collections::HashSet<String> {
    let mut keys = std::collections::HashSet::new();
    for m in &manifest.mods {
        if m.content_type != tuffbox_core::manifest::ContentType::Mod {
            continue;
        }
        keys.insert(m.id.to_lowercase());
        keys.insert(m.name.to_lowercase());
        if let Some(pid) = &m.source.project_id {
            keys.insert(pid.to_lowercase());
        }
        if let Some(file) = &m.file_name {
            keys.insert(file.to_lowercase());
            if let Some(stem) = std::path::Path::new(file)
                .file_stem()
                .and_then(|s| s.to_str())
            {
                keys.insert(stem.to_lowercase());
            }
        }
    }
    keys
}

/// True if any installed mod covers `aliases` — exact slug/name match, or a
/// fork/port whose id/name/jar starts with (or clearly contains) the alias
/// (e.g. `modernfix-mvus` covers `modernfix`).
fn has_installed(keys: &std::collections::HashSet<String>, aliases: &[&str]) -> bool {
    let compact_keys: Vec<String> = keys
        .iter()
        .map(|k| compact_mod_token(k))
        .filter(|k| k.len() >= 3)
        .collect();

    for alias in aliases {
        let lower = alias.to_lowercase();
        if keys.contains(&lower) {
            return true;
        }
        let ac = compact_mod_token(alias);
        if ac.len() < 3 {
            continue;
        }
        for ck in &compact_keys {
            if ck == &ac {
                return true;
            }
            // Short slugs (emi, jei, iris, …) stay exact-only to avoid
            // false positives inside unrelated names.
            if ac.len() < 6 {
                continue;
            }
            // Installed fork/port of the suggested mod.
            if ck.starts_with(&ac) {
                return true;
            }
            // e.g. jar `…-modernfix-…` or name with prefix noise.
            if ck.contains(&ac) {
                return true;
            }
        }
    }
    false
}

/// Known Modrinth slug families for heuristic filtering (ports/forks/loaders).
fn recommendation_aliases(slug: &str) -> Vec<&'static str> {
    match slug {
        "emi" => vec!["emi", "roughly-enough-items", "jei", "rei"],
        "jei" => vec!["jei", "emi", "roughly-enough-items", "rei"],
        "fabric-api" | "fabric_api" => vec!["fabric-api", "fabric_api"],
        _ => tuffbox_core::optimize_pack::recommendation_aliases(slug),
    }
}

fn aliases_for_candidate(slug: &str) -> Vec<String> {
    tuffbox_core::optimize_pack::aliases_for_candidate(slug)
}

fn alias_refs(aliases: &[String]) -> Vec<&str> {
    aliases.iter().map(|s| s.as_str()).collect()
}

type RecCandidate = (&'static str, &'static str, &'static str, &'static str);

fn qol_candidates(loader: &str) -> Vec<RecCandidate> {
    let recipe_viewer: RecCandidate = match loader {
        "fabric" | "quilt" => (
            "emi",
            "EMI",
            "Modern recipe viewer (REI/JEI alternative for Fabric)",
            "qol",
        ),
        _ => ("jei", "JEI", "Recipe viewer — essential for modded Minecraft", "qol"),
    };
    vec![
        recipe_viewer,
        (
            "jade",
            "Jade",
            "Shows block/entity info when looking at them",
            "qol",
        ),
        (
            "appleskin",
            "AppleSkin",
            "Hunger and saturation overlay for food",
            "qol",
        ),
        (
            "mouse-tweaks",
            "Mouse Tweaks",
            "Better inventory mouse handling",
            "qol",
        ),
        (
            "controlling",
            "Controlling",
            "Searchable keybind menu",
            "qol",
        ),
    ]
}

fn push_rec(
    out: &mut Vec<serde_json::Value>,
    reason: &str,
    slug: &str,
    name: &str,
    description: &str,
    priority: &str,
) {
    if out.iter().any(|r| r.get("slug").and_then(|v| v.as_str()) == Some(slug)) {
        return;
    }
    out.push(serde_json::json!({
        "reason": reason,
        "slug": slug,
        "name": name,
        "description": description,
        "priority": priority,
    }));
}

/// Drop suggestions that have no Modrinth file for this pack's MC + loader.
/// Also rewrites the slug to the first alias that actually resolves.
/// A compat-check error is DEFINITIVE (the rec should be dropped) only when
/// the API actually answered — 404 means the project does not exist on
/// Modrinth (hallucinated slug / renamed project). Everything else — DNS
/// failure, timeout, rate limit, 5xx, open circuit breaker — is inconclusive
/// and must NOT drop the recommendation. `get_json_with_context` embeds the
/// HTTP status in the error text, which is the only signal at this layer.
fn compat_check_definitive(err_msg: &str) -> bool {
    err_msg.contains("status 404")
}

fn filter_compatible_recommendations(
    manifest: &ProjectManifest,
    recs: Vec<serde_json::Value>,
) -> Vec<serde_json::Value> {
    let provider = tuffbox_core::ModrinthProvider::new();
    let loader = tuffbox_core::graph::loader_kind_slug(&manifest.loader.kind);
    let query = ProviderSearchQuery {
        minecraft_version: Some(manifest.minecraft.version.clone()),
        loader: Some(loader.to_string()),
        ..Default::default()
    };

    let mut out = Vec::new();
    // Compat-check bookkeeping: an answer from the API (even "no versions
    // for this loader/MC") is a DEFINITIVE verdict; a transport error is
    // INCONCLUSIVE. The old code treated both as "drop the recommendation",
    // so a Modrinth outage / rate limit silently wiped the whole list —
    // including locally computed heuristics.
    let mut definitive_checks = 0usize;
    let mut inconclusive_checks = 0usize;
    for mut rec in recs {
        let Some(slug) = rec.get("slug").and_then(|v| v.as_str()).map(|s| s.to_string()) else {
            continue;
        };
        let mut try_slugs: Vec<String> = vec![slug.clone()];
        for a in recommendation_aliases(&slug) {
            if a != slug.as_str() {
                try_slugs.push(a.to_string());
            }
        }

        let mut matched: Option<(String, String)> = None;
        let mut check_failed = false;
        for candidate in &try_slugs {
            match provider.get_versions(candidate, &query) {
                Ok(versions) if !versions.is_empty() => {
                    definitive_checks += 1;
                    matched = Some((candidate.clone(), versions[0].version_number.clone()));
                    break;
                }
                Ok(_) => {
                    // The API answered: no build for this loader/MC version.
                    definitive_checks += 1;
                }
                Err(e) => {
                    if compat_check_definitive(&e.to_string()) {
                        definitive_checks += 1;
                    } else {
                        check_failed = true;
                    }
                }
            }
        }

        let Some((resolved_slug, version_number)) = matched else {
            if check_failed {
                // Keep the recommendation, marked unverified: dropping it
                // here is how a Modrinth outage used to empty the panel.
                inconclusive_checks += 1;
                if let Some(obj) = rec.as_object_mut() {
                    obj.insert("compatibility".into(), serde_json::json!("unverified"));
                }
                out.push(rec);
            }
            // else: definitive negative → drop the recommendation.
            continue;
        };
        if let Some(obj) = rec.as_object_mut() {
            obj.insert("compatibility".into(), serde_json::json!("verified"));
            if resolved_slug != slug {
                if let Ok(project) = provider.get_project(&resolved_slug) {
                    obj.insert("name".into(), serde_json::json!(project.name));
                    if !project.description.is_empty() {
                        let short: String = project.description.chars().take(120).collect();
                        obj.insert("description".into(), serde_json::json!(short));
                    }
                }
            }
            obj.insert("slug".into(), serde_json::json!(resolved_slug));
            obj.insert("compatibleVersion".into(), serde_json::json!(version_number));
            obj.insert("loader".into(), serde_json::json!(loader));
            obj.insert(
                "minecraftVersion".into(),
                serde_json::json!(manifest.minecraft.version),
            );
        }
        out.push(rec);
    }
    // Total outage: not a single query got a real answer, so "filtering"
    // proved nothing. Everything was already kept above and marked
    // `unverified`; annotate the list so the UI can say so.
    if definitive_checks == 0 && inconclusive_checks > 0 {
        for rec in &mut out {
            if let Some(obj) = rec.as_object_mut() {
                obj.insert(
                    "compatibilityNote".into(),
                    serde_json::json!("Modrinth could not be reached — compatibility unverified"),
                );
            }
        }
    }
    out
}

fn heuristic_mod_recommendations(manifest: &ProjectManifest) -> Vec<serde_json::Value> {
    let keys = installed_mod_keys(manifest);
    let loader = tuffbox_core::graph::loader_kind_slug(&manifest.loader.kind);
    let mc = manifest.minecraft.version.clone();
    let mut recommendations = Vec::new();

    for candidate in tuffbox_core::optimize_pack::optimization_candidates(loader, &mc) {
        let aliases = aliases_for_candidate(&candidate.slug);
        if !has_installed(&keys, &alias_refs(&aliases)) {
            push_rec(
                &mut recommendations,
                "optimization",
                &candidate.slug,
                &candidate.name,
                &candidate.reason,
                "high",
            );
        }
    }

    for (slug, name, desc, reason) in qol_candidates(loader) {
        let aliases = aliases_for_candidate(slug);
        if !has_installed(&keys, &alias_refs(&aliases)) {
            push_rec(&mut recommendations, reason, slug, name, desc, "medium");
        }
    }

    if matches!(loader, "fabric" | "quilt")
        && !has_installed(&keys, &alias_refs(&aliases_for_candidate("fabric-api")))
    {
        push_rec(
            &mut recommendations,
            "dependency",
            "fabric-api",
            "Fabric API",
            "Required by most Fabric/Quilt mods",
            "critical",
        );
    }

    if has_installed(&keys, &["create"]) {
        for (slug, name, desc) in [
            (
                "createaddition",
                "Create Crafts & Additions",
                "Electricity and extras for Create",
            ),
            (
                "create-steam-n-rails",
                "Create: Steam 'n' Rails",
                "Trains and advanced rails for Create",
            ),
        ] {
            if !has_installed(&keys, &alias_refs(&aliases_for_candidate(slug))) {
                push_rec(&mut recommendations, "synergy", slug, name, desc, "low");
            }
        }
    }

    // Annotate with pack context so the UI can show why these were picked.
    for rec in &mut recommendations {
        if let Some(obj) = rec.as_object_mut() {
            obj.insert("loader".into(), serde_json::json!(loader));
            obj.insert("minecraftVersion".into(), serde_json::json!(mc));
            obj.insert("source".into(), serde_json::json!("heuristic"));
        }
    }

    recommendations
}

fn build_mod_recommendation_prompt(manifest: &ProjectManifest, seed: &[serde_json::Value]) -> String {
    let loader = tuffbox_core::graph::loader_kind_slug(&manifest.loader.kind);
    let mc = &manifest.minecraft.version;
    let installed: Vec<String> = manifest
        .mods
        .iter()
        .filter(|m| m.content_type == tuffbox_core::manifest::ContentType::Mod)
        .take(80)
        .map(|m| format!("{} ({})", m.id, m.name))
        .collect();
    let seed_slugs: Vec<String> = seed
        .iter()
        .filter_map(|r| r.get("slug").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .collect();

    format!(
        r#"You are a Minecraft modpack optimization advisor.
Return ONLY valid JSON with this schema:
{{
  "recommendations": [
    {{
      "slug": "modrinth-slug",
      "name": "Display Name",
      "description": "Why this helps this pack",
      "priority": "critical|high|medium|low",
      "reason": "optimization|qol|dependency|synergy"
    }}
  ]
}}

Rules:
- Suggest at most 8 mods.
- Prefer performance / optimization mods that exist on Modrinth for loader "{loader}" and Minecraft {mc}.
- Do NOT suggest mods already installed.
- Do NOT suggest a mod if a fork, port, or unofficial build of it is already installed (e.g. modernfix-mvus covers modernfix; rubidium/embeddium cover sodium).
- Do NOT suggest Fabric-only mods for Forge/NeoForge (e.g. no Sodium on Forge — use Embeddium).
- Do NOT suggest Forge-only mods for Fabric/Quilt.
- Prefer well-known Modrinth slugs (sodium, lithium, embeddium, modernfix, ferrite-core, iris, oculus, jei, emi).
- Skip anything incompatible with {loader} {mc}.

Installed mods:
{}

Heuristic seed suggestions (refine/replace if wrong):
{}
"#,
        installed.join(", "),
        seed_slugs.join(", ")
    )
}

/// Analyzes the current modpack and suggests optimization / QoL mods for the
/// active loader + Minecraft version. Uses heuristics first, then optionally
/// refines via the configured AI provider.
#[tauri::command(rename_all = "camelCase")]
async fn recommend_mods(path: String) -> Result<Vec<serde_json::Value>, String> {
    let path_clone = path.clone();
    let mut recommendations = tokio::task::spawn_blocking(move || {
        let manifest = ProjectManifest::load_from_path(&path_clone).map_err(|e| e.to_string())?;
        Ok::<_, String>(heuristic_mod_recommendations(&manifest))
    })
    .await
    .map_err(|e| e.to_string())??;

    // Best-effort AI refinement — never fail the whole command if AI is offline.
    let ai_result = tokio::task::spawn_blocking({
        let path = path.clone();
        let seed = recommendations.clone();
        move || -> Result<Vec<serde_json::Value>, String> {
            let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
            let prompt = build_mod_recommendation_prompt(&manifest, &seed);
            Ok(vec![serde_json::json!({
                "_prompt": prompt,
                "_loader": tuffbox_core::graph::loader_kind_slug(&manifest.loader.kind),
                "_mc": manifest.minecraft.version,
            })])
        }
    })
    .await;

    if let Ok(Ok(meta)) = ai_result {
        if let Some(prompt) = meta
            .first()
            .and_then(|v| v.get("_prompt"))
            .and_then(|v| v.as_str())
        {
            let settings = integrations::get_integration_status().settings;
            // Best-effort step: cap it so a stalled endpoint cannot pin the
            // whole recommendations command (per-attempt retries alone can
            // stretch the underlying call to ~10 minutes).
            let ai_call = tokio::time::timeout(
                std::time::Duration::from_secs(90),
                integrations::call_ai(&settings.ai, prompt),
            )
            .await;
            if let Ok(Ok(ai_json)) = ai_call {
                let keys = tokio::task::spawn_blocking({
                    let path = path.clone();
                    move || {
                        ProjectManifest::load_from_path(&path)
                            .map(|m| installed_mod_keys(&m))
                            .unwrap_or_default()
                    }
                })
                .await
                .unwrap_or_default();

                let ai_recs = ai_json
                    .get("recommendations")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();

                let mut merged = Vec::new();
                for rec in ai_recs {
                    let Some(slug) = rec.get("slug").and_then(|v| v.as_str()) else {
                        continue;
                    };
                    let mut aliases = recommendation_aliases(slug);
                    aliases.push(slug);
                    if has_installed(&keys, &aliases) {
                        continue;
                    }
                    let name = rec
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or(slug);
                    let description = rec
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("AI-suggested optimization mod");
                    let priority = rec
                        .get("priority")
                        .and_then(|v| v.as_str())
                        .unwrap_or("high");
                    let reason = rec
                        .get("reason")
                        .and_then(|v| v.as_str())
                        .unwrap_or("optimization");
                    let mut obj = serde_json::json!({
                        "reason": reason,
                        "slug": slug,
                        "name": name,
                        "description": description,
                        "priority": priority,
                        "source": "ai",
                    });
                    if let Some(map) = obj.as_object_mut() {
                        if let Some(m) = meta.first().and_then(|v| v.as_object()) {
                            if let Some(loader) = m.get("_loader") {
                                map.insert("loader".into(), loader.clone());
                            }
                            if let Some(mc) = m.get("_mc") {
                                map.insert("minecraftVersion".into(), mc.clone());
                            }
                        }
                    }
                    merged.push(obj);
                }

                if !merged.is_empty() {
                    // Prefer AI list, then fill gaps from heuristics.
                    for h in recommendations {
                        let slug = h.get("slug").and_then(|v| v.as_str()).unwrap_or("");
                        if !merged
                            .iter()
                            .any(|r| r.get("slug").and_then(|v| v.as_str()) == Some(slug))
                        {
                            merged.push(h);
                        }
                    }
                    recommendations = merged;
                }
            }
        }
    }

    // Cap the list so the panel stays usable — after Modrinth compat filter.
    let path_for_filter = path.clone();
    recommendations = tokio::task::spawn_blocking(move || {
        let manifest = ProjectManifest::load_from_path(&path_for_filter).map_err(|e| e.to_string())?;
        Ok::<_, String>(filter_compatible_recommendations(&manifest, recommendations))
    })
    .await
    .map_err(|e| e.to_string())??;

    recommendations.truncate(12);
    Ok(recommendations)
}

#[cfg(test)]
mod compat_check_tests {
    use super::compat_check_definitive;

    #[test]
    fn not_found_is_definitive() {
        assert!(compat_check_definitive(
            "JSON decode error for https://api.modrinth.com/v2/project/nope/version?loaders=[\"forge\"] (status 404): invalid type: map, expected a sequence"
        ));
    }

    #[test]
    fn network_failures_are_inconclusive() {
        assert!(!compat_check_definitive("HTTP request failed: error sending request for url (https://api.modrinth.com/): operation timed out"));
        assert!(!compat_check_definitive(
            "network request failed: Connection refused (os error 111)"
        ));
        // Rate limits and server errors must never drop a recommendation.
        assert!(!compat_check_definitive(
            "JSON decode error for https://api.modrinth.com/v2/project/jei/version (status 429): invalid type: map, expected a sequence"
        ));
        assert!(!compat_check_definitive(
            "JSON decode error for https://api.modrinth.com/v2/project/jei/version (status 503): invalid type: map, expected a sequence"
        ));
    }

    #[test]
    fn empty_and_unknown_errors_are_inconclusive() {
        assert!(!compat_check_definitive(""));
        assert!(!compat_check_definitive("circuit breaker open for api.modrinth.com"));
    }
}

#[cfg(test)]
mod pack_format_sniff_tests {
    use super::zip_has_entry;
    use std::io::Write;

    /// A Modrinth .mrpack downloaded over HTTP lands in a temp `*.zip` file;
    /// the install path must sniff `modrinth.index.json` from the archive
    /// content instead of trusting the extension (task: "archive error:
    /// specified file not found in archive" for Fabulously Optimized).
    #[test]
    fn zip_has_entry_sniffs_mrpack_index_in_zip_named_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tuffbox-pack-123.zip"); // zip extension on purpose
        {
            let file = std::fs::File::create(&path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options =
                zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            zip.start_file("modrinth.index.json", options).unwrap();
            zip.write_all(br#"{"formatVersion":1,"game":"minecraft","name":"t","files":[],"dependencies":{}}"#)
                .unwrap();
            zip.finish().unwrap();
        }
        assert!(zip_has_entry(&path, "modrinth.index.json"));
        assert!(!zip_has_entry(&path, "instance.cfg"));
        assert!(!zip_has_entry(&path, "missing.json"));
    }

    #[test]
    fn zip_has_entry_false_for_non_zip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("not-a-zip.zip");
        std::fs::write(&path, b"definitely not a zip").unwrap();
        assert!(!zip_has_entry(&path, "modrinth.index.json"));
    }
}

#[cfg(test)]
mod recommend_mod_tests {
    use super::{compact_mod_token, has_installed};
    use std::collections::HashSet;

    #[test]
    fn compact_strips_separators() {
        assert_eq!(compact_mod_token("ModernFix-mVUS"), "modernfixmvus");
        assert_eq!(compact_mod_token("modernfix_neoforge-5.20.9"), "modernfixneoforge5209");
    }

    #[test]
    fn modernfix_mvus_covers_modernfix() {
        let mut keys = HashSet::new();
        keys.insert("modernfix-mvus".into());
        assert!(has_installed(&keys, &["modernfix"]));
    }

    #[test]
    fn modernfix_name_port_covers_modernfix() {
        let mut keys = HashSet::new();
        keys.insert("modernfix mvus".into());
        assert!(has_installed(&keys, &["modernfix"]));
    }

    #[test]
    fn jar_stem_covers_modernfix() {
        let mut keys = HashSet::new();
        keys.insert("modernfix-neoforge-5.20.9".into());
        assert!(has_installed(&keys, &["modernfix"]));
    }

    #[test]
    fn short_slug_stays_exact() {
        let mut keys = HashSet::new();
        keys.insert("something-with-emi-inside".into());
        assert!(!has_installed(&keys, &["emi"]));
        keys.insert("emi".into());
        assert!(has_installed(&keys, &["emi"]));
    }

    #[test]
    fn unrelated_mod_does_not_cover() {
        let mut keys = HashSet::new();
        keys.insert("sodium".into());
        assert!(!has_installed(&keys, &["modernfix"]));
    }
}

/// Returns a compatibility database entry for a mod slug from the builtin
/// knowledge base.
#[tauri::command(rename_all = "camelCase")]
fn get_mod_info(slug: String) -> Result<Option<serde_json::Value>, String> {
    if let Some(entry) = tuffbox_core::knowledge::ModKnowledgeEntry::lookup(&slug) {
        Ok(Some(serde_json::json!({
            "slug": entry.slug, "name": entry.name,
            "configPaths": entry.config_paths,
            "oreKeys": entry.ore_keys,
            "knownConflicts": entry.known_conflicts,
            "loaders": entry.loaders,
            "category": entry.category,
        })))
    } else {
        Ok(None)
    }
}

/// Restores a project backup zip, extracting it over the current project.
/// Creates a snapshot before restoring as a safety net.
#[tauri::command(rename_all = "camelCase")]
fn restore_backup(path: String, backup_id: String) -> Result<(), String> {
    if !backup_id.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
        return Err("invalid backup id".into());
    }
    let project_dir = manifest_parent(&path)?;
    let zip_path = project_dir
        .join(".tuffbox")
        .join("backups")
        .join(format!("{}.zip", backup_id));
    if !zip_path.is_file() {
        return Err("backup not found".into());
    }

    // Safety: snapshot before restore
    let manifest_path = PathBuf::from(&path);
    auto_snapshot(&manifest_path, "before-restore").map_err(|e| e.to_string())?;

    let file = std::fs::File::open(&zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().replace('\\', "/");
        if name.ends_with('/') {
            continue;
        }
        // Zip-slip guard done lexically: backup zips live inside the project,
        // but a hand-crafted or corrupted archive must not escape it. The old
        // canonicalize-based check also false-rejected entries in NEW
        // subdirectories (nothing to canonicalize yet), breaking restores.
        let rel = name.trim_start_matches('/');
        if rel.is_empty() || tuffbox_core::importer::is_unsafe_zip_rel_path(rel) {
            return Err(format!("zip entry escapes project directory: {name}"));
        }
        let target = project_dir.join(rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        // Defense in depth: with parents in place, verify the real path.
        let canonical = std::fs::canonicalize(&target)
            .or_else(|_| std::fs::canonicalize(target.parent().unwrap_or(&project_dir)))
            .map_err(|e| e.to_string())?;
        if !canonical.starts_with(
            std::fs::canonicalize(&project_dir).map_err(|e| e.to_string())?
        ) {
            return Err(format!("zip entry escapes project directory: {name}"));
        }
        let mut dest = std::fs::File::create(&target).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut dest).map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// ── Problematic mods config ─────────────────────────────────────

/// Writes a problematic-mods.json config for mods known to cause crashes.
/// Compatible with Crash Assistant's problematic_mods_config.json format.
#[tauri::command(rename_all = "camelCase")]
fn save_problematic_mods_config(
    path: String,
    entries: Vec<serde_json::Value>,
) -> Result<(), String> {
    let project_dir = manifest_parent(&path)?;
    let p = project_dir
        .join("config")
        .join("problematic_mods_config.json");
    if let Some(par) = p.parent() {
        std::fs::create_dir_all(par).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(&serde_json::json!({ "mods": entries }))
        .map_err(|e| e.to_string())?;
    std::fs::write(&p, json).map_err(|e| e.to_string())
}

/// Returns the current problematic mods config.
#[tauri::command(rename_all = "camelCase")]
fn get_problematic_mods_config(path: String) -> Result<Vec<serde_json::Value>, String> {
    let project_dir = manifest_parent(&path)?;
    let p = project_dir
        .join("config")
        .join("problematic_mods_config.json");
    if !p.is_file() {
        return Ok(vec![]);
    }
    let raw = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    Ok(v.get("mods")
        .and_then(|m| m.as_array())
        .cloned()
        .unwrap_or_default())
}

/// ── Server launch ────────────────────────────────────────────────

/// Prepares a server working directory (both+server mods only), writes
/// `server.properties` / `eula.txt`, then launches the server profile with a
/// visible console window. `server_dir` is where the staged instance lives.
#[tauri::command(rename_all = "camelCase")]
async fn launch_server(
    app: tauri::AppHandle,
    path: String,
    server_dir: String,
    level_seed: Option<String>,
    online_mode: Option<bool>,
) -> Result<tuffbox_core::LaunchResult, LaunchErrorInfo> {
    let profile = "server";
    let server_dir_buf = PathBuf::from(server_dir.trim());
    let stage_log = server_dir_buf.join("logs").join("tuffbox-console.log");
    emit_launch_phase(
        &app,
        &path,
        profile,
        LaunchPhase::Preflight,
        "Preparing server launch…",
        Some(&stage_log),
        None,
        None,
        None,
        false,
        None,
    );
    if server_dir_buf.as_os_str().is_empty() {
        let info = LaunchErrorInfo::new(LaunchErrorKind::Install, "server directory is required")
            .with_log(&stage_log);
        emit_launch_phase(
            &app,
            &path,
            profile,
            LaunchPhase::Failed,
            info.message.clone(),
            Some(&stage_log),
            None,
            None,
            None,
            false,
            Some(info.clone()),
        );
        return Err(info);
    }

    if let Err(error) = record_launch(path.clone()) {
        let info = LaunchErrorInfo::new(LaunchErrorKind::Unknown, error.to_string())
            .with_log(&stage_log);
        emit_launch_phase(
            &app,
            &path,
            profile,
            LaunchPhase::Failed,
            info.message.clone(),
            Some(&stage_log),
            None,
            None,
            None,
            false,
            Some(info.clone()),
        );
        return Err(info);
    }

    emit_launch_phase(
        &app,
        &path,
        profile,
        LaunchPhase::Downloading,
        "Staging the server instance…",
        Some(&stage_log),
        None,
        None,
        None,
        false,
        None,
    );
    let path_for_prep = path.clone();
    let server_dir_for_prep = server_dir_buf.clone();
    let seed = level_seed.clone();
    let online = online_mode;
    let prep = tokio::task::spawn_blocking(move || {
        let manifest_path = resolve_manifest_path(&path_for_prep)
            .map_err(|error| LaunchErrorInfo::new(LaunchErrorKind::Install, error))?;
        let project_dir = manifest_path.parent().ok_or_else(|| {
            LaunchErrorInfo::new(
                LaunchErrorKind::Unknown,
                "manifest has no parent directory",
            )
        })?;
        let manifest = ProjectManifest::load_from_path(&manifest_path).map_err(|error| {
            LaunchErrorInfo::new(LaunchErrorKind::Install, error.to_string())
        })?;

        tuffbox_core::TestLauncher::prepare_server_instance(
            &manifest,
            project_dir,
            &server_dir_for_prep,
            &manifest_path,
        )
        .map_err(|error| LaunchErrorInfo::new(LaunchErrorKind::Install, error.to_string()))?;

        write_server_properties_file(
            &server_dir_for_prep,
            &manifest,
            seed.as_deref(),
            online,
        )
        .map_err(|error| LaunchErrorInfo::new(LaunchErrorKind::Install, error))?;
        Ok::<(), LaunchErrorInfo>(())
    })
    .await
    .map_err(|error| {
        LaunchErrorInfo::new(
            LaunchErrorKind::Unknown,
            format!("server prepare task panicked: {error}"),
        )
    })
    .and_then(|result| result);

    if let Err(info) = prep {
        let info = if info.log_path.is_some() {
            info
        } else {
            info.with_log(&stage_log)
        };
        emit_launch_phase(
            &app,
            &path,
            profile,
            LaunchPhase::Failed,
            info.message.clone(),
            Some(&stage_log),
            None,
            None,
            None,
            false,
            Some(info.clone()),
        );
        return Err(info);
    }

    launch_profile_impl(
        app,
        path,
        profile.into(),
        None,
        None,
        None,
        Some(server_dir_buf),
        true,
        true,
    )
    .await
}

/// Generates a default server.properties file for the project.
#[tauri::command(rename_all = "camelCase")]
fn generate_server_properties(
    path: String,
    level_seed: Option<String>,
    online_mode: Option<bool>,
    target_dir: Option<String>,
) -> Result<String, String> {
    let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
    let project_dir = manifest_parent(&path)?;
    let out_dir = target_dir
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or(project_dir);
    write_server_properties_file(&out_dir, &manifest, level_seed.as_deref(), online_mode)
}

fn write_server_properties_file(
    out_dir: &Path,
    manifest: &ProjectManifest,
    level_seed: Option<&str>,
    online_mode: Option<bool>,
) -> Result<String, String> {
    let profile = manifest
        .profiles
        .iter()
        .find(|p| p.id == "server")
        .or_else(|| manifest.profiles.first());

    let online = online_mode.unwrap_or(true);
    let mut props = String::new();
    props.push_str("# TuffBox generated server.properties\n");
    props.push_str("server-port=25565\n");
    props.push_str("max-players=20\n");
    props.push_str("view-distance=10\n");
    props.push_str("simulation-distance=10\n");
    props.push_str("max-world-size=29999984\n");
    props.push_str("allow-flight=false\n");
    props.push_str(&format!("online-mode={online}\n"));
    props.push_str("difficulty=normal\n");
    props.push_str("gamemode=survival\n");
    props.push_str("enable-command-block=false\n");
    props.push_str("spawn-protection=16\n");
    props.push_str("max-tick-time=60000\n");
    props.push_str("level-name=world\n");
    if let Some(seed) = level_seed.map(str::trim).filter(|s| !s.is_empty()) {
        props.push_str(&format!("level-seed={seed}\n"));
    }
    props.push_str(&format!(
        "motd=A TuffBox {} Server\n",
        manifest.project.name
    ));

    if let Some(profile) = profile {
        if let Some(mem) = profile.memory_mb {
            props.push_str(&format!("# Memory: {mem} MB\n"));
        }
    }

    std::fs::create_dir_all(out_dir).map_err(|e| e.to_string())?;
    let target = out_dir.join("server.properties");
    std::fs::write(&target, &props).map_err(|e| e.to_string())?;
    Ok(props)
}

/// ── Recipe scanner from actual JARs ──────────────────────────────

/// Scans mod JAR / datapack / KubeJS recipes with JEI-style layouts.
#[tauri::command(rename_all = "camelCase")]
async fn scan_mod_recipes(path: String) -> Result<serde_json::Value, String> {
    tokio::task::spawn_blocking(move || {
        let manifest_path = resolve_manifest_path(&path)?;
        let jar_roots = catalog_vanilla_jar_roots();
        let result = tuffbox_core::recipe_scan::scan_project_recipes_with_vanilla_roots(
            &manifest_path,
            &jar_roots,
        )?;
        serde_json::to_value(result).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Launcher + fallback directories searched for the installed vanilla client jar.
fn catalog_vanilla_jar_roots() -> Vec<PathBuf> {
    let mut roots = vec![launcher_settings::resolve_runtime_path()];
    if let Some(data) = dirs::data_dir() {
        roots.push(data.join("TuffBox"));
    }
    if let Some(appdata) = std::env::var_os("APPDATA") {
        roots.push(PathBuf::from(&appdata).join("TuffBox"));
        roots.push(PathBuf::from(appdata).join(".minecraft"));
    }
    if let Some(home) = std::env::var_os("HOME") {
        roots.push(PathBuf::from(&home).join(".local/share/TuffBox"));
        roots.push(PathBuf::from(home).join(".minecraft"));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        roots.push(PathBuf::from(local).join("TuffBox"));
    }
    roots.sort();
    roots.dedup();
    roots
}

/// Whether the vanilla client jar for the project's Minecraft version is installed.
#[tauri::command(rename_all = "camelCase")]
async fn get_vanilla_client_jar_status(
    path: String,
) -> Result<tuffbox_core::item_catalog::VanillaClientJarStatus, String> {
    tokio::task::spawn_blocking(move || {
        let manifest_path = resolve_manifest_path(&path)?;
        let jar_roots = catalog_vanilla_jar_roots();
        tuffbox_core::item_catalog::vanilla_client_jar_status_for_manifest(
            &manifest_path,
            &jar_roots,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Download (or resume) the vanilla client jar for the project's Minecraft version.
#[tauri::command(rename_all = "camelCase")]
async fn download_vanilla_client_jar(path: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let manifest_path = resolve_manifest_path(&path)?;
        let install_root = launcher_settings::resolve_runtime_path();
        let jar_roots = catalog_vanilla_jar_roots();
        let jar = tuffbox_core::item_catalog::download_vanilla_client_jar_for_manifest(
            &manifest_path,
            &install_root,
            &jar_roots,
        )?;
        Ok(jar.to_string_lossy().into_owned())
    })
    .await
    .map_err(|e| e.to_string())?
}

fn recipe_icon_extra_jars(manifest_path: &Path) -> Result<(PathBuf, Vec<PathBuf>), String> {
    let project_dir = manifest_path
        .parent()
        .ok_or_else(|| "manifest path has no parent".to_string())?;
    let manifest = ProjectManifest::load_from_path(manifest_path).map_err(|e| e.to_string())?;
    let version = &manifest.minecraft.version;
    let mut extra_jars = Vec::new();
    for root in catalog_vanilla_jar_roots() {
        let client_jar = root
            .join("versions")
            .join(version)
            .join(format!("{version}.jar"));
        if client_jar.is_file() && !extra_jars.iter().any(|p| p == &client_jar) {
            extra_jars.push(client_jar);
        }
    }
    Ok((project_dir.to_path_buf(), extra_jars))
}

/// Returns a cached PNG path for a Minecraft item id (`namespace:path`), extracted
/// from the project mods and the installed vanilla client jar when available.
#[tauri::command(rename_all = "camelCase")]
async fn get_item_icon(path: String, item_id: String) -> Result<Option<String>, String> {
    tokio::task::spawn_blocking(move || {
        let manifest_path = PathBuf::from(&path);
        let (project_dir, extra_jars) = recipe_icon_extra_jars(&manifest_path)?;
        let icon = tuffbox_core::resolve_item_icon_data_url(&project_dir, &item_id, &extra_jars)?;
        Ok(icon)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Resolves many item icons in one IPC call (opens each mod jar once).
#[tauri::command(rename_all = "camelCase")]
async fn get_item_icons_batch(
    path: String,
    item_ids: Vec<String>,
) -> Result<std::collections::HashMap<String, Option<String>>, String> {
    tokio::task::spawn_blocking(move || {
        let manifest_path = PathBuf::from(&path);
        let (project_dir, extra_jars) = recipe_icon_extra_jars(&manifest_path)?;
        let icons =
            tuffbox_core::resolve_item_icons_data_urls(&project_dir, &item_ids, &extra_jars)?;
        Ok(icons)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
async fn get_recipe_runtime_status(path: String) -> tuffbox_core::RecipeRuntimeStatus {
    tokio::task::spawn_blocking(move || tuffbox_core::recipe_runtime_status(Path::new(&path)))
        .await
        .unwrap_or(tuffbox_core::RecipeRuntimeStatus {
            connected: false,
            supported: false,
            message: "Failed to check JEI runtime".to_string(),
            minecraft_version: None,
            pid: None,
        })
}

#[tauri::command(rename_all = "camelCase")]
async fn get_recipe_runtime_snapshot(path: String) -> Result<serde_json::Value, String> {
    tokio::task::spawn_blocking(move || {
        tuffbox_core::fetch_recipe_runtime_snapshot(Path::new(&path))
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Append recipe remove lines to kubejs/server_scripts/tuffbox_recipe_removes.js
#[tauri::command(rename_all = "camelCase")]
fn write_kubejs_recipe_removes(path: String, recipe_ids: Vec<String>) -> Result<String, String> {
    let project_dir = manifest_parent(&path)?;
    tuffbox_core::recipe_scan::write_kubejs_remove(&project_dir, &recipe_ids)
}

/// Append a shaped/shapeless/cooking/smithing/stonecutting craft to tuffbox_recipe_adds.js.
#[tauri::command(rename_all = "camelCase")]
fn write_kubejs_craft_recipe(
    path: String,
    draft: tuffbox_core::recipe_scan::CraftDraft,
) -> Result<String, String> {
    let project_dir = manifest_parent(&path)?;
    tuffbox_core::recipe_scan::write_kubejs_craft(&project_dir, &draft)
}

/// Append item-tag edits to kubejs/server_scripts/tuffbox_tag_edits.js.
#[tauri::command(rename_all = "camelCase")]
fn write_kubejs_tag_edits(
    path: String,
    draft: tuffbox_core::recipe_scan::TagDraft,
) -> Result<String, String> {
    let project_dir = manifest_parent(&path)?;
    tuffbox_core::recipe_scan::write_kubejs_tags(&project_dir, &draft)
}

/// List known item tags (`#ns:path`) from the offline tag index for the recipe palette.
#[tauri::command(rename_all = "camelCase")]
async fn list_item_tags(path: String) -> Result<Vec<String>, String> {
    tokio::task::spawn_blocking(move || {
        let manifest_path = PathBuf::from(&path);
        let (project_dir, extra_jars) = recipe_icon_extra_jars(&manifest_path)?;
        let loader = tuffbox_core::recipe_scan::loader_kind_from_manifest(&manifest_path)?;
        Ok(tuffbox_core::recipe_scan::list_item_tags(
            &project_dir,
            loader,
            &extra_jars,
        ))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Direct members of an item tag (not fully expanded).
#[tauri::command(rename_all = "camelCase")]
async fn get_item_tag_entries(path: String, tag_id: String) -> Result<Vec<String>, String> {
    tokio::task::spawn_blocking(move || {
        let manifest_path = PathBuf::from(&path);
        let (project_dir, extra_jars) = recipe_icon_extra_jars(&manifest_path)?;
        let loader = tuffbox_core::recipe_scan::loader_kind_from_manifest(&manifest_path)?;
        Ok(tuffbox_core::recipe_scan::get_tag_entries(
            &project_dir,
            loader,
            &extra_jars,
            &tag_id,
        ))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Generate a KubeJS snippet (does not write to disk).
#[tauri::command(rename_all = "camelCase")]
fn generate_kubejs_recipe_script(
    kind: String,
    recipe_ids: Vec<String>,
    new_item: Option<String>,
    count: Option<u32>,
) -> Result<serde_json::Value, String> {
    let script = match kind.as_str() {
        "remove" => tuffbox_core::recipe_scan::kubejs_remove_script(&recipe_ids),
        "replace_output" => {
            let id = recipe_ids
                .first()
                .ok_or_else(|| "recipe id required".to_string())?;
            let item = new_item.unwrap_or_else(|| "minecraft:air".into());
            tuffbox_core::recipe_scan::kubejs_replace_output(id, &item, count.unwrap_or(1))
        }
        other => return Err(format!("unknown script kind: {other}")),
    };
    serde_json::to_value(script).map_err(|e| e.to_string())
}

/// Load FTB Quests chapters from project config via the SNBT parser.
#[tauri::command(rename_all = "camelCase")]
fn load_quest_book(path: String) -> Result<serde_json::Value, String> {
    let project_dir = manifest_parent(&path)?;
    let book = tuffbox_core::unified::QuestBook::load_from_project(&project_dir)?;
    serde_json::to_value(book).map_err(|e| e.to_string())
}

/// Save a single quest chapter back to disk as SNBT.
#[tauri::command(rename_all = "camelCase")]
fn save_quest_chapter(
    path: String,
    chapter: tuffbox_core::unified::Chapter,
    relative_path: Option<String>,
) -> Result<serde_json::Value, String> {
    let _guard = QUEST_IO_LOCK
        .lock()
        .map_err(|_| "quest I/O lock poisoned".to_string())?;
    let manifest_path = PathBuf::from(&path);
    let project_dir = manifest_parent(&path)?;
    let rel = tuffbox_core::unified::QuestBook::save_chapter(
        &project_dir,
        &chapter,
        relative_path.as_deref(),
    )?;
    auto_snapshot_with_changed_files(&manifest_path, "save-quest-chapter", &[PathBuf::from(&rel)])
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "relativePath": rel, "questCount": chapter.quests.len() }))
}

pub(crate) fn collect_catalog_item_ids(
    manifest_path: &std::path::Path,
) -> Result<std::collections::HashSet<String>, String> {
    let mut extra = Vec::new();
    let jar_roots = catalog_vanilla_jar_roots();
    if let Ok(scan) = tuffbox_core::recipe_scan::scan_project_recipes_with_vanilla_roots(
        manifest_path,
        &jar_roots,
    ) {
        for r in scan.recipes {
            if !r.output_id.is_empty() && !r.output_id.starts_with('#') {
                extra.push(r.output_id);
            }
            for id in r.input_ids {
                if !id.is_empty() && !id.starts_with('#') {
                    extra.push(id);
                }
            }
        }
    }
    let items = tuffbox_core::item_catalog::build_item_catalog_for_manifest(
        manifest_path,
        extra,
        &jar_roots,
    )?;
    Ok(items.into_iter().map(|e| e.id).collect())
}

/// Cached recipe + item ids only. Never opens jars.
pub(crate) fn collect_catalog_item_ids_click_path(
    manifest_path: &std::path::Path,
) -> std::collections::HashSet<String> {
    let Some(project_dir) = manifest_path.parent() else {
        return std::collections::HashSet::new();
    };
    let mut ids = std::collections::HashSet::new();
    if let Some(scan) = tuffbox_core::recipe_scan::load_cached_recipe_scan(project_dir) {
        for r in scan.recipes {
            if !r.output_id.is_empty() && !r.output_id.starts_with('#') {
                ids.insert(r.output_id);
            }
            for id in r.input_ids {
                if !id.is_empty() && !id.starts_with('#') {
                    ids.insert(id);
                }
            }
        }
    }
    if let Some(items) = tuffbox_core::item_catalog::load_cached_item_catalog(project_dir) {
        for entry in items {
            ids.insert(entry.id);
        }
    }
    ids
}

/// Full vanilla+mod item catalog for Recipes / quest pickers.
#[tauri::command(rename_all = "camelCase")]
async fn list_item_catalog(path: String) -> Result<Vec<serde_json::Value>, String> {
    tokio::task::spawn_blocking(move || {
        let manifest_path = resolve_manifest_path(&path)?;
        let jar_roots = catalog_vanilla_jar_roots();
        // Recipe-derived ids are merged on the UI side after scan; jar models cover the rest.
        let items = tuffbox_core::item_catalog::build_item_catalog_for_manifest(
            &manifest_path,
            Vec::<String>::new(),
            &jar_roots,
        )?;
        items
            .into_iter()
            .map(|e| serde_json::to_value(e).map_err(|err| err.to_string()))
            .collect()
    })
    .await
    .map_err(|e| e.to_string())?
}

/// List item ids from the recipe catalog (for quest item pickers).
#[tauri::command(rename_all = "camelCase")]
async fn list_quest_item_catalog(path: String) -> Result<Vec<String>, String> {
    tokio::task::spawn_blocking(move || {
        let manifest_path = resolve_manifest_path(&path)?;
        let set = collect_catalog_item_ids_click_path(&manifest_path);
        let mut ids: Vec<String> = set.into_iter().collect();
        ids.sort();
        Ok(ids)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Validate quest book integrity (missing deps, empty tasks, cycles, reachability, items).
#[tauri::command(rename_all = "camelCase")]
async fn validate_quest_book(path: String) -> Result<Vec<serde_json::Value>, String> {
    tokio::task::spawn_blocking(move || {
        let manifest_path = PathBuf::from(&path);
        let project_dir = manifest_parent(&path)?;
        let book = tuffbox_core::unified::QuestBook::load_from_project(&project_dir)?;
        let available = collect_catalog_item_ids_click_path(&manifest_path);
        let errors = if available.is_empty() {
            book.validate()
        } else {
            book.validate_with_items(Some(&available))
        };
        Ok(errors
            .into_iter()
            .map(|e| serde_json::json!({ "questId": e.quest_id, "message": e.message }))
            .collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Parse AI QuestPlan JSON (fences ok) and merge into the project's quest book in memory.
/// Does not write SNBT — caller applies result in the editor, then Save.
#[tauri::command(rename_all = "camelCase")]
fn parse_and_merge_quest_plan(path: String, raw: String) -> Result<serde_json::Value, String> {
    let project_dir = manifest_parent(&path)?;
    let book = tuffbox_core::unified::QuestBook::load_from_project(&project_dir)?;
    let result = tuffbox_core::quest_plan::parse_and_merge_quest_plan(&book, &raw)?;
    serde_json::to_value(result).map_err(|e| e.to_string())
}

/// Validate a QuestPlan object without merging.
#[tauri::command(rename_all = "camelCase")]
fn validate_quest_plan(
    plan: tuffbox_core::quest_plan::QuestPlan,
) -> Result<serde_json::Value, String> {
    let v = tuffbox_core::quest_plan::validate_quest_plan(&plan);
    serde_json::to_value(v).map_err(|e| e.to_string())
}

/// System prompt text for quest-authoring models (UI / local LLM).
#[tauri::command(rename_all = "camelCase")]
fn quest_plan_system_prompt() -> String {
    tuffbox_core::quest_plan::QUEST_PLAN_SYSTEM_PROMPT.to_string()
}

/// Natural-language → QuestPlan → merge preview.
/// Uses offline heuristic for simple numbered prompts; otherwise configured AI (Ollama / OpenAI-compatible).
#[tauri::command(rename_all = "camelCase")]
async fn generate_quest_plan_from_prompt(
    path: String,
    prompt: String,
    force_ai: Option<bool>,
) -> Result<serde_json::Value, String> {
    let prompt = prompt.trim().to_string();
    if prompt.is_empty() {
        return Err("Empty quest prompt".into());
    }
    let project_dir = manifest_parent(&path)?;
    let book = tuffbox_core::unified::QuestBook::load_from_project(&project_dir)?;

    let force_ai = force_ai.unwrap_or(false);
    let plan = if !force_ai {
        if let Some(p) = tuffbox_core::quest_plan::try_heuristic_quest_plan(&prompt) {
            p
        } else {
            generate_quest_plan_via_ai(&path, &prompt, &book).await?
        }
    } else {
        generate_quest_plan_via_ai(&path, &prompt, &book).await?
    };

    let result = tuffbox_core::quest_plan::merge_quest_plan(&book, &plan)?;
    serde_json::to_value(result).map_err(|e| e.to_string())
}

async fn generate_quest_plan_via_ai(
    path: &str,
    prompt: &str,
    book: &tuffbox_core::unified::QuestBook,
) -> Result<tuffbox_core::quest_plan::QuestPlan, String> {
    let settings = integrations::get_integration_status().settings;
    let sample_items = {
        let manifest_path = PathBuf::from(path);
        collect_catalog_item_ids_click_path(&manifest_path)
            .into_iter()
            .take(80)
            .collect::<Vec<_>>()
    };
    let ctx = tuffbox_core::quest_plan::QuestAuthorContext {
        existing_chapters: book
            .chapters
            .iter()
            .map(|c| tuffbox_core::quest_plan::ExistingChapter {
                id: c.id.clone(),
                title: c.title.clone(),
                group: c.group.clone(),
            })
            .collect(),
        existing_groups: book
            .chapter_groups
            .iter()
            .map(|g| tuffbox_core::quest_plan::ExistingGroup {
                id: g.id.clone(),
                title: g.title.clone(),
            })
            .collect(),
        sample_items,
        pack_hint: book.title.clone().or_else(|| {
            let mp = resolve_manifest_path(path).ok()?;
            let m = tuffbox_core::ProjectManifest::load_from_path(&mp).ok()?;
            let b = m.brief?;
            let mut parts: Vec<String> = Vec::new();
            if !b.goal.is_empty() {
                parts.push(format!("Goal: {}", b.goal));
            }
            if !b.target_audience.is_empty() {
                parts.push(format!("Audience: {}", b.target_audience));
            }
            if !b.gameplay_pillars.is_empty() {
                parts.push(format!("Pillars: {}", b.gameplay_pillars.join(", ")));
            }
            if !b.constraints.is_empty() {
                parts.push(format!("Constraints: {}", b.constraints.join(", ")));
            }
            if parts.is_empty() {
                None
            } else {
                Some(parts.join(" | "))
            }
        }),
        existing_quests: Vec::new(),
        existing_quest_lore: Vec::new(),
        anchor_quest: None,
        target_chapter: None,
    };
    let user_msg = tuffbox_core::quest_plan::build_quest_author_user_message(prompt, &ctx);
    let messages = vec![serde_json::json!({"role": "user", "content": user_msg})];
    let value = integrations::call_ai_messages(
        &settings.ai,
        tuffbox_core::quest_plan::QUEST_PLAN_SYSTEM_PROMPT,
        &messages,
        true,
    )
    .await?;
    // call_ai_messages(json_mode) returns a JSON Value — stringify for parse_quest_plan
    let raw = if value.is_string() {
        value.as_str().unwrap_or("").to_string()
    } else {
        value.to_string()
    };
    tuffbox_core::quest_plan::parse_quest_plan(&raw)
}

/// Save an FTB Quests reward table SNBT file.
#[tauri::command(rename_all = "camelCase")]
fn save_quest_reward_table(
    path: String,
    table: tuffbox_core::unified::RewardTable,
    relative_path: Option<String>,
) -> Result<serde_json::Value, String> {
    let _guard = QUEST_IO_LOCK
        .lock()
        .map_err(|_| "quest I/O lock poisoned".to_string())?;
    let manifest_path = PathBuf::from(&path);
    let project_dir = manifest_parent(&path)?;
    let rel = tuffbox_core::unified::RewardTable::save_to_project(
        &project_dir,
        &table,
        relative_path.as_deref(),
    )?;
    auto_snapshot_with_changed_files(&manifest_path, "save-quest-reward-table", &[PathBuf::from(&rel)])
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "relativePath": rel, "entryCount": table.rewards.len() }))
}

/// Save quest book `data.snbt` (title + defaults).
#[tauri::command(rename_all = "camelCase")]
fn save_quest_book_data(
    path: String,
    book: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let _guard = QUEST_IO_LOCK
        .lock()
        .map_err(|_| "quest I/O lock poisoned".to_string())?;
    let manifest_path = PathBuf::from(&path);
    let project_dir = manifest_parent(&path)?;
    let mut loaded = tuffbox_core::unified::QuestBook::load_from_project(&project_dir)?;
    if let Some(t) = book.get("title").and_then(|v| v.as_str()) {
        loaded.title = Some(t.to_string());
    } else if book.get("title").map(|v| v.is_null()).unwrap_or(false) {
        loaded.title = None;
    }
    if let Some(t) = book.get("subtitle").and_then(|v| v.as_str()) {
        loaded.subtitle = Some(t.to_string());
    }
    if let Some(settings) = book.get("bookSettings").and_then(|v| v.as_object()) {
        loaded.book_settings = settings
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
    }
    let rel = tuffbox_core::unified::QuestBook::save_book_data(&project_dir, &loaded)?;
    auto_snapshot_with_changed_files(&manifest_path, "save-quest-book-data", &[PathBuf::from(&rel)])
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "relativePath": rel }))
}

/// Save `chapter_groups.snbt`.
#[tauri::command(rename_all = "camelCase")]
fn save_quest_chapter_groups(
    path: String,
    groups: Vec<tuffbox_core::unified::ChapterGroup>,
) -> Result<serde_json::Value, String> {
    let _guard = QUEST_IO_LOCK
        .lock()
        .map_err(|_| "quest I/O lock poisoned".to_string())?;
    let manifest_path = PathBuf::from(&path);
    let project_dir = manifest_parent(&path)?;
    let rel = tuffbox_core::unified::QuestBook::save_chapter_groups(&project_dir, &groups)?;
    auto_snapshot_with_changed_files(
        &manifest_path,
        "save-quest-chapter-groups",
        &[PathBuf::from(&rel)],
    )
    .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "relativePath": rel }))
}

/// Save `lang/<code>.snbt` locale overlay map.
#[tauri::command(rename_all = "camelCase")]
fn save_quest_locale(
    path: String,
    code: String,
    map: std::collections::HashMap<String, serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let _guard = QUEST_IO_LOCK
        .lock()
        .map_err(|_| "quest I/O lock poisoned".to_string())?;
    let manifest_path = PathBuf::from(&path);
    let project_dir = manifest_parent(&path)?;
    let rel = tuffbox_core::unified::QuestBook::save_locale(&project_dir, &code, &map)?;
    auto_snapshot_with_changed_files(&manifest_path, "save-quest-locale", &[PathBuf::from(&rel)])
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "relativePath": rel }))
}

/// List FTB Quests team progress files under saves/*/ftbquests/.
#[tauri::command(rename_all = "camelCase")]
fn list_quest_progress_teams(path: String) -> Result<Vec<serde_json::Value>, String> {
    let project_dir = manifest_parent(&path)?;
    let teams = tuffbox_core::unified::list_progress_teams(&project_dir);
    teams
        .into_iter()
        .map(|t| serde_json::to_value(t).map_err(|e| e.to_string()))
        .collect()
}

/// Load read-only quest progress overlay for a team file.
#[tauri::command(rename_all = "camelCase")]
fn load_quest_progress(
    path: String,
    relative_path: String,
) -> Result<serde_json::Value, String> {
    let project_dir = manifest_parent(&path)?;
    let book = tuffbox_core::unified::QuestBook::load_from_project(&project_dir)?;
    let snap =
        tuffbox_core::unified::load_progress_for_book(&project_dir, &relative_path, &book)?;
    serde_json::to_value(snap).map_err(|e| e.to_string())
}

/// In-memory playthrough simulate — never reads/writes saves/.
#[tauri::command(rename_all = "camelCase")]
fn simulate_quest_progress(
    book: tuffbox_core::unified::QuestBook,
    completed_ids: Vec<String>,
    task_progress_ids: Option<Vec<String>>,
) -> Result<serde_json::Value, String> {
    use std::collections::HashSet;
    let completed: HashSet<String> = completed_ids.into_iter().collect();
    let tasks: HashSet<String> = task_progress_ids.unwrap_or_default().into_iter().collect();
    let snap = tuffbox_core::unified::build_progress_snapshot(&book, &completed, &tasks);
    serde_json::to_value(snap).map_err(|e| e.to_string())
}

/// ── Quests · KubeJS bridge ──────────────────────────────────────

#[tauri::command(rename_all = "camelCase")]
fn quest_kubejs_list_scripts(path: String) -> Result<serde_json::Value, String> {
    let project_dir = manifest_parent(&path)?;
    let scripts = tuffbox_core::quest_kubejs::list_quest_scripts(&project_dir);
    serde_json::to_value(scripts).map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
fn quest_kubejs_audit(path: String, book: serde_json::Value) -> Result<serde_json::Value, String> {
    let project_dir = manifest_parent(&path)?;
    let audit = tuffbox_core::quest_kubejs::audit_bindings(&project_dir, &book);
    serde_json::to_value(audit).map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
fn quest_kubejs_read_script(path: String, relative_path: String) -> Result<String, String> {
    let project_dir = manifest_parent(&path)?;
    tuffbox_core::quest_kubejs::read_script(&project_dir, &relative_path)
}

#[tauri::command(rename_all = "camelCase")]
fn quest_kubejs_ensure_managed(path: String) -> Result<String, String> {
    let project_dir = manifest_parent(&path)?;
    tuffbox_core::quest_kubejs::ensure_managed_script(&project_dir)
}

#[tauri::command(rename_all = "camelCase")]
fn quest_kubejs_render_template(
    params: tuffbox_core::quest_kubejs::QuestKubeJsTemplateParams,
) -> Result<String, String> {
    tuffbox_core::quest_kubejs::render_template(&params)
}

#[tauri::command(rename_all = "camelCase")]
fn quest_kubejs_append_handler(
    path: String,
    snippet: String,
) -> Result<serde_json::Value, String> {
    let manifest_path = PathBuf::from(&path);
    let project_dir = manifest_parent(&path)?;
    let relative = tuffbox_core::quest_kubejs::MANAGED_RELATIVE;
    let snap = auto_snapshot_detailed(
        &manifest_path,
        "quest-kubejs",
        &[PathBuf::from(relative)],
        &["Append FTB Quests KubeJS handler".into()],
    )
    .map_err(|e| e.to_string())?;
    let written = tuffbox_core::quest_kubejs::append_handler(&project_dir, &snippet)?;
    Ok(serde_json::json!({
        "relativePath": written,
        "snapshotId": snap.id,
    }))
}

/// ── World management ────────────────────────────────────────────

/// Lists resourcepacks or shaderpacks on disk (zip/folders + `.disabled`).
#[tauri::command(rename_all = "camelCase")]
fn list_content_packs(path: String, folder: String) -> Result<Vec<tuffbox_core::content_packs::ContentPackEntry>, String> {
    if folder != "resourcepacks" && folder != "shaderpacks" {
        return Err("folder must be resourcepacks or shaderpacks".into());
    }
    let project_dir = manifest_parent(&path)?;
    tuffbox_core::content_packs::list_content_packs(&project_dir, &folder)
}

#[tauri::command(rename_all = "camelCase")]
fn set_content_pack_enabled(
    path: String,
    folder: String,
    file_name: String,
    enabled: bool,
) -> Result<tuffbox_core::content_packs::ContentPackEntry, String> {
    if folder != "resourcepacks" && folder != "shaderpacks" {
        return Err("folder must be resourcepacks or shaderpacks".into());
    }
    let project_dir = manifest_parent(&path)?;
    tuffbox_core::content_packs::set_content_pack_enabled(&project_dir, &folder, &file_name, enabled)
}

#[tauri::command(rename_all = "camelCase")]
fn list_mc_servers(path: String) -> Result<Vec<tuffbox_core::servers_dat::ServerEntry>, String> {
    let project_dir = manifest_parent(&path)?;
    tuffbox_core::servers_dat::list_servers(&project_dir)
}

#[tauri::command(rename_all = "camelCase")]
fn add_mc_server(
    path: String,
    name: String,
    address: String,
) -> Result<Vec<tuffbox_core::servers_dat::ServerEntry>, String> {
    let project_dir = manifest_parent(&path)?;
    tuffbox_core::servers_dat::add_server(&project_dir, &name, &address)
}

#[tauri::command(rename_all = "camelCase")]
fn remove_mc_server(
    path: String,
    address: String,
) -> Result<Vec<tuffbox_core::servers_dat::ServerEntry>, String> {
    let project_dir = manifest_parent(&path)?;
    tuffbox_core::servers_dat::remove_server(&project_dir, &address)
}

#[tauri::command(rename_all = "camelCase")]
fn ping_mc_server(address: String) -> Result<tuffbox_core::servers_dat::ServerPingResult, String> {
    Ok(tuffbox_core::servers_dat::ping_server_address(&address))
}

/// ── Modpack templates ───────────────────────────────────────────

/// Saves the current project as a reusable template (copies manifest + modlist metadata).
#[tauri::command(rename_all = "camelCase")]
fn save_as_template(path: String, template_name: String) -> Result<(), String> {
    let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
    let project_dir = manifest_parent(&path)?;
    let template_dir = project_dir.join(".tuffbox").join("templates");
    std::fs::create_dir_all(&template_dir).map_err(|e| e.to_string())?;
    let template = serde_json::json!({
        "name": template_name,
        "createdAt": tuffbox_core::time_util::rfc3339_now(),
        "manifest": manifest,
        "modCount": manifest.mods.len(),
    });
    let fname = template_name.to_lowercase().replace(' ', "-");
    let p = template_dir.join(format!("{}.json", fname));
    std::fs::write(
        &p,
        serde_json::to_string_pretty(&template).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

/// Lists saved modpack templates.
#[tauri::command(rename_all = "camelCase")]
fn list_templates(path: String) -> Result<Vec<serde_json::Value>, String> {
    let project_dir = manifest_parent(&path)?;
    let template_dir = project_dir.join(".tuffbox").join("templates");
    if !template_dir.is_dir() {
        return Ok(vec![]);
    }
    let mut templates = Vec::new();
    for entry in std::fs::read_dir(&template_dir)
        .into_iter()
        .flatten()
        .flatten()
    {
        if entry.path().extension().map_or(false, |e| e == "json") {
            if let Ok(raw) = std::fs::read_to_string(entry.path()) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
                    templates.push(v);
                }
            }
        }
    }
    Ok(templates)
}

/// ── Download progress tracking ──────────────────────────────────

static DOWNLOAD_PROGRESS: once_cell::sync::Lazy<
    std::sync::Mutex<std::collections::HashMap<String, (u64, u64)>>,
> = once_cell::sync::Lazy::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

/// Returns the current download progress for active mod downloads.
#[tauri::command(rename_all = "camelCase")]
fn get_download_progress() -> Result<Vec<serde_json::Value>, String> {
    let map = DOWNLOAD_PROGRESS.lock().map_err(|e| e.to_string())?;
    Ok(map.iter().map(|(k, (done, total))| serde_json::json!({
        "id": k, "downloaded": done, "total": total,
        "percent": if *total > 0 { ((*done as f64 / *total as f64) * 100.0).round() as u32 } else { 0 }
    })).collect())
}

/// ── Keyboard shortcut reference ─────────────────────────────────

/// Returns a keyboard shortcut reference sheet.
#[tauri::command(rename_all = "camelCase")]
fn get_keyboard_shortcuts() -> Result<Vec<serde_json::Value>, String> {
    Ok(vec![
        serde_json::json!({"key": "Ctrl+1", "action": "Dashboard", "context": "global"}),
        serde_json::json!({"key": "Ctrl+2", "action": "IDE Workflow", "context": "global"}),
        serde_json::json!({"key": "Ctrl+3", "action": "Mods", "context": "global"}),
        serde_json::json!({"key": "Ctrl+4", "action": "Dependency Graph", "context": "global"}),
        serde_json::json!({"key": "Ctrl+5", "action": "Config Editor", "context": "global"}),
        serde_json::json!({"key": "Ctrl+6", "action": "Health Check", "context": "global"}),
        serde_json::json!({"key": "Ctrl+7", "action": "Snapshots", "context": "global"}),
        serde_json::json!({"key": "Ctrl+S", "action": "Save file", "context": "Config Editor"}),
        serde_json::json!({"key": "Ctrl+N", "action": "New project", "context": "Dashboard"}),
        serde_json::json!({"key": "Ctrl+O", "action": "Open project", "context": "Dashboard"}),
        serde_json::json!({"key": "Escape", "action": "Close modal / deselect", "context": "global"}),
    ])
}

/// ── Config linter ────────────────────────────────────────────────

/// Lints a config file for common Minecraft issues: syntax errors,
/// duplicate keys, missing defaults, and performance-sapping settings.
#[tauri::command(rename_all = "camelCase")]
fn lint_config(path: String, relative_path: String) -> Result<Vec<serde_json::Value>, String> {
    let project_dir = manifest_parent(&path)?;
    let target = project_dir.join(&relative_path);
    let content = std::fs::read_to_string(&target).map_err(|e| e.to_string())?;
    let mut issues = Vec::new();
    let ext = std::path::Path::new(&relative_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    match ext {
        "json" => {
            if let Err(e) = serde_json::from_str::<serde_json::Value>(&content) {
                issues.push(serde_json::json!({"severity":"error","code":"JSON_SYNTAX","message":format!("JSON syntax error: {}", e),"line":null}));
            }
        }
        "properties" | "txt" => {
            let mut seen_keys = std::collections::HashSet::new();
            for (line_no, line) in content.lines().enumerate() {
                let t = line.trim();
                if t.is_empty() || t.starts_with('#') {
                    continue;
                }
                // Task #64: .properties files legitimately contain non-key lines
                // (section headers, license blocks, continuation backslashes,
                // MO/other-launcher metadata). Only flag a missing '=' when the
                // line looks like a key: starts with an identifier-ish token and
                // is short. Everything else is noise we must not warn about.
                if !t.contains('=') && t.len() > 2 {
                    // Real .properties keys are single tokens (dots/dashes/
                    // underscores, no spaces). Anything with spaces is prose
                    // (headers, license text) — never warn about it.
                    let looks_like_key = t
                        .chars()
                        .next()
                        .map(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
                        .unwrap_or(false)
                        && t.chars().all(|c| {
                            c.is_ascii_alphanumeric() || "_-.".contains(c)
                        });
                    if looks_like_key {
                        issues.push(serde_json::json!({"severity":"warning","code":"PROPERTY_NO_EQ","message":"Line without = sign","line":line_no+1}));
                    }
                    continue;
                }
                if let Some(eq) = t.find('=') {
                    let key = t[..eq].trim();
                    if key.is_empty() {
                        issues.push(serde_json::json!({"severity":"warning","code":"EMPTY_KEY","message":"Empty key","line":line_no+1}));
                    } else if !seen_keys.insert(key.to_string()) {
                        issues.push(serde_json::json!({"severity":"warning","code":"DUPLICATE_KEY","message":format!("Duplicate key: {}", key),"line":line_no+1}));
                    }
                }
            }
        }
        "toml" => {
            if let Err(e) = toml::from_str::<toml::Value>(&content) {
                issues.push(serde_json::json!({"severity":"error","code":"TOML_SYNTAX","message":format!("TOML syntax error: {}", e),"line":null}));
            }
        }
        _ => {}
    }

    // Check for common performance-sapping server settings
    if content.contains("max-tick-time=-1") {
        issues.push(serde_json::json!({"severity":"warning","code":"MAX_TICK_TIME_DISABLED","message":"max-tick-time is -1 (off). Server won't crash on overload but may freeze indefinitely.","line":null}));
    }
    if content.contains("view-distance=") {
        for line in content.lines() {
            if line.contains("view-distance=") {
                if let Some(v) = line.split('=').last() {
                    if let Ok(n) = v.trim().parse::<u32>() {
                        if n > 16 {
                            issues.push(serde_json::json!({"severity":"warning","code":"HIGH_VIEW_DISTANCE","message":format!("View distance {} may cause lag on modded servers.", n),"line":null}));
                        }
                    }
                }
                break;
            }
        }
    }

    Ok(issues)
}

/// ── Memory cleanup / temp files ──────────────────────────────────

/// Cleans up temporary files from the project: old test runs, stale
/// snapshots, and downloaded mod jars that are no longer in the manifest.
#[tauri::command(rename_all = "camelCase")]
fn cleanup_project(path: String) -> Result<serde_json::Value, String> {
    let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
    let project_dir = manifest_parent(&path)?;
    let mut cleaned = Vec::new();

    // Remove mods not in manifest (orphaned jars)
    let mods_dir = project_dir.join("mods");
    let known_files: std::collections::HashSet<String> = manifest
        .mods
        .iter()
        .filter_map(|m| m.file_name.clone())
        .collect();
    if mods_dir.is_dir() {
        for entry in std::fs::read_dir(&mods_dir).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if entry.path().extension().map_or(false, |e| e == "jar")
                && !known_files.contains(&name)
            {
                let _ = std::fs::remove_file(entry.path());
                cleaned.push(format!("mods/{}", name));
            }
        }
    }

    // Remove old test run logs (older than 30 days)
    let test_runs = project_dir.join(".tuffbox").join("test-runs");
    if test_runs.is_dir() {
        let cutoff = std::time::SystemTime::now() - std::time::Duration::from_secs(30 * 86400);
        for entry in std::fs::read_dir(&test_runs)
            .into_iter()
            .flatten()
            .flatten()
        {
            if entry.path().is_dir() {
                if let Ok(meta) = entry.metadata() {
                    if let Ok(mod_time) = meta.modified() {
                        if mod_time < cutoff {
                            let _ = std::fs::remove_dir_all(entry.path());
                            cleaned
                                .push(format!("test-runs/{}", entry.file_name().to_string_lossy()));
                        }
                    }
                }
            }
        }
    }

    Ok(serde_json::json!({"cleaned": cleaned, "count": cleaned.len()}))
}

/// ── App version & update check ───────────────────────────────────

/// Returns the current TuffBox version.
#[tauri::command(rename_all = "camelCase")]
fn get_app_version() -> Result<String, String> {
    Ok(env!("CARGO_PKG_VERSION").to_string())
}

/// ── World preview (level.dat reader) ────────────────────────────

/// Reads a Minecraft world's metadata from saves/<name>/level.dat
/// and returns structured world info: name, seed, game type, etc.
#[tauri::command(rename_all = "camelCase")]
fn read_world_info(path: String, world_name: String) -> Result<serde_json::Value, String> {
    let project_dir = manifest_parent(&path)?;
    let world_dir = project_dir.join("saves").join(&world_name);
    let info = tuffbox_core::level_dat::read_world_info(&world_dir)
        .map_err(|e| format!("Failed to read level.dat: {}", e))?;
    Ok(serde_json::json!({
        "name": info.name, "seed": info.seed, "gameType": info.game_type,
        "lastPlayed": info.last_played, "time": info.time,
        "spawnX": info.spawn_x, "spawnY": info.spawn_y, "spawnZ": info.spawn_z,
        "difficulty": info.difficulty, "hardcore": info.hardcore,
        "cheatsEnabled": info.cheats_enabled,
        "sizeBytes": info.size_bytes, "sizeFormatted": info.size_formatted,
    }))
}

/// ── World map (Anvil region reader) ──────────────────────────

/// Returns a mcaselector-style 2D overview of a world's region files:
/// per-region 32x32 chunk grids with presence, last-modified time and a
/// coarse generation status used for coloring.
#[tauri::command(rename_all = "camelCase")]
fn read_world_map(
    path: String,
    world_name: String,
    dimension: Option<String>,
) -> Result<serde_json::Value, String> {
    let project_dir = manifest_parent(&path)?;
    let world_dir = project_dir.join("saves").join(&world_name);
    let map = tuffbox_core::region::read_world_map(&world_dir, dimension.as_deref())?;
    Ok(serde_json::to_value(&map).map_err(|e| e.to_string())?)
}

/// Lists dimensions that have a region folder for the given world.
#[tauri::command(rename_all = "camelCase")]
fn list_world_dimensions(path: String, world_name: String) -> Result<Vec<String>, String> {
    let project_dir = manifest_parent(&path)?;
    let world_dir = project_dir.join("saves").join(&world_name);
    Ok(tuffbox_core::region::list_world_dimensions(&world_dir))
}

/// A region coordinate paired with the local chunk indices (0..1024) to clear.

/// Deletes selected chunks from a world's region files, mirroring mcaselector.
/// Each selection maps a region coordinate to the local chunk indices to clear.
#[tauri::command(rename_all = "camelCase")]
fn delete_world_chunks(
    path: String,
    world_name: String,
    selections: Vec<ChunkSelection>,
    dimension: Option<String>,
) -> Result<usize, String> {
    let project_dir = manifest_parent(&path)?;
    let world_dir = project_dir.join("saves").join(&world_name);
    let pairs: Vec<(i32, i32, Vec<usize>)> = selections
        .into_iter()
        .map(|s| (s.region_x, s.region_z, s.indices))
        .collect();
    tuffbox_core::region::delete_world_chunks(&world_dir, &pairs, dimension.as_deref())
        .map_err(|e| format!("Failed to delete chunks: {}", e))
}

/// Copies selected chunks from a world's region files to a clipboard payload.
#[tauri::command(rename_all = "camelCase")]
fn copy_world_chunks(
    path: String,
    world_name: String,
    selections: Vec<ChunkSelection>,
    dimension: Option<String>,
) -> Result<tuffbox_core::region::ChunkClipboard, String> {
    let project_dir = manifest_parent(&path)?;
    let world_dir = project_dir.join("saves").join(&world_name);
    let pairs: Vec<(i32, i32, Vec<usize>)> = selections
        .into_iter()
        .map(|s| (s.region_x, s.region_z, s.indices))
        .collect();
    tuffbox_core::region::copy_world_chunks(&world_dir, &world_name, &pairs, dimension.as_deref())
        .map_err(|e| format!("Failed to copy chunks: {}", e))
}

/// Pastes chunk data from a clipboard payload into a world's region files.
/// `offset_x` / `offset_z` are **chunk** coordinate offsets (MCA Selector style).
#[tauri::command(rename_all = "camelCase")]
fn paste_world_chunks(
    path: String,
    world_name: String,
    clipboard: tuffbox_core::region::ChunkClipboard,
    offset_x: Option<i32>,
    offset_z: Option<i32>,
    dimension: Option<String>,
    overwrite: Option<bool>,
) -> Result<usize, String> {
    let project_dir = manifest_parent(&path)?;
    let world_dir = project_dir.join("saves").join(&world_name);
    tuffbox_core::region::paste_world_chunks_ex(
        &world_dir,
        &clipboard,
        offset_x.unwrap_or(0),
        offset_z.unwrap_or(0),
        dimension.as_deref(),
        overwrite.unwrap_or(true),
    )
    .map_err(|e| format!("Failed to paste chunks: {}", e))
}

/// Compacts region files (purge orphaned sectors after deletes).
#[tauri::command(rename_all = "camelCase")]
fn purge_world_regions(
    path: String,
    world_name: String,
    dimension: Option<String>,
) -> Result<usize, String> {
    let project_dir = manifest_parent(&path)?;
    let world_dir = project_dir.join("saves").join(&world_name);
    tuffbox_core::region::purge_world_regions(&world_dir, dimension.as_deref())
        .map_err(|e| format!("Failed to purge regions: {}", e))
}

/// Exports selected chunks into a destination folder (mini world).
#[tauri::command(rename_all = "camelCase")]
fn export_world_chunks(
    path: String,
    world_name: String,
    selections: Vec<ChunkSelection>,
    dest_dir: String,
    dimension: Option<String>,
) -> Result<usize, String> {
    let project_dir = manifest_parent(&path)?;
    let world_dir = project_dir.join("saves").join(&world_name);
    let pairs: Vec<(i32, i32, Vec<usize>)> = selections
        .into_iter()
        .map(|s| (s.region_x, s.region_z, s.indices))
        .collect();
    tuffbox_core::region::export_world_chunks(
        &world_dir,
        &pairs,
        dimension.as_deref(),
        std::path::Path::new(&dest_dir),
    )
    .map_err(|e| format!("Failed to export chunks: {}", e))
}

/// Import chunks from another world / export folder into the target world.
#[tauri::command(rename_all = "camelCase")]
fn import_world_chunks(
    path: String,
    world_name: String,
    source_dir: String,
    offset_x: Option<i32>,
    offset_z: Option<i32>,
    overwrite: Option<bool>,
    y_offset: Option<i32>,
    sections: Option<String>,
    source_selections: Option<Vec<ChunkSelection>>,
    target_selections: Option<Vec<ChunkSelection>>,
    source_dimension: Option<String>,
    dimension: Option<String>,
) -> Result<usize, String> {
    let project_dir = manifest_parent(&path)?;
    let world_dir = project_dir.join("saves").join(&world_name);
    let source_sels: Vec<(i32, i32, Vec<usize>)> = source_selections
        .unwrap_or_default()
        .into_iter()
        .map(|s| (s.region_x, s.region_z, s.indices))
        .collect();
    let target_sels: Option<Vec<(i32, i32, Vec<usize>)>> = target_selections.map(|v| {
        v.into_iter()
            .map(|s| (s.region_x, s.region_z, s.indices))
            .collect()
    });
    let opts = tuffbox_core::region::ImportOptions {
        offset_x: offset_x.unwrap_or(0),
        offset_z: offset_z.unwrap_or(0),
        overwrite: overwrite.unwrap_or(true),
        y_offset: y_offset.unwrap_or(0),
        sections,
    };
    tuffbox_core::region::import_world_chunks(
        &world_dir,
        std::path::Path::new(&source_dir),
        &source_sels,
        source_dimension.as_deref(),
        dimension.as_deref(),
        &opts,
        target_sels.as_deref(),
    )
    .map_err(|e| format!("Failed to import chunks: {}", e))
}

/// Render world map (or selection) to a PNG file on disk.
#[tauri::command(rename_all = "camelCase")]
fn render_world_map_png(
    path: String,
    world_name: String,
    dest_path: String,
    color_mode: Option<String>,
    scale: Option<u32>,
    selections: Option<Vec<ChunkSelection>>,
    dimension: Option<String>,
) -> Result<(u32, u32), String> {
    let project_dir = manifest_parent(&path)?;
    let world_dir = project_dir.join("saves").join(&world_name);
    let pairs: Vec<(i32, i32, Vec<usize>)> = selections
        .unwrap_or_default()
        .into_iter()
        .map(|s| (s.region_x, s.region_z, s.indices))
        .collect();
    let mode = tuffbox_core::region::MapColorMode::parse(color_mode.as_deref().unwrap_or("status"));
    tuffbox_core::region::render_world_map_png(
        &world_dir,
        dimension.as_deref(),
        &pairs,
        mode,
        scale.unwrap_or(4),
        std::path::Path::new(&dest_path),
    )
    .map_err(|e| format!("Failed to render map PNG: {}", e))
}

/// Select chunks by MCA-style map filter query.
#[tauri::command(rename_all = "camelCase")]
fn select_world_by_query(
    path: String,
    world_name: String,
    query: String,
    dimension: Option<String>,
) -> Result<Vec<tuffbox_core::region_edit::ChunkRef>, String> {
    let project_dir = manifest_parent(&path)?;
    let world_dir = project_dir.join("saves").join(&world_name);
    tuffbox_core::region_edit::select_world_by_query(&world_dir, &query, dimension.as_deref())
        .map_err(|e| format!("Failed to select by query: {}", e))
}

/// Warm world map region metadata cache.
#[tauri::command(rename_all = "camelCase")]
fn warm_world_map_cache(
    path: String,
    world_name: String,
    dimension: Option<String>,
) -> Result<usize, String> {
    let project_dir = manifest_parent(&path)?;
    let world_dir = project_dir.join("saves").join(&world_name);
    tuffbox_core::region::warm_world_map_cache(&world_dir, dimension.as_deref())
        .map_err(|e| format!("Failed to warm map cache: {}", e))
}

/// Clear world map region metadata cache.
#[tauri::command(rename_all = "camelCase")]
fn clear_world_map_cache(
    path: String,
    world_name: String,
    dimension: Option<String>,
) -> Result<usize, String> {
    let project_dir = manifest_parent(&path)?;
    let world_dir = project_dir.join("saves").join(&world_name);
    tuffbox_core::region::clear_world_map_cache(&world_dir, dimension.as_deref())
        .map_err(|e| format!("Failed to clear map cache: {}", e))
}

/// Swaps exactly two chunks (useful for repair after corruption).
#[tauri::command(rename_all = "camelCase")]
fn swap_world_chunks(
    path: String,
    world_name: String,
    a: ChunkSelection,
    b: ChunkSelection,
    dimension: Option<String>,
) -> Result<(), String> {
    let project_dir = manifest_parent(&path)?;
    let world_dir = project_dir.join("saves").join(&world_name);
    let ai = *a.indices.first().ok_or("chunk A needs one index")?;
    let bi = *b.indices.first().ok_or("chunk B needs one index")?;
    tuffbox_core::region::swap_world_chunks(
        &world_dir,
        (a.region_x, a.region_z, ai),
        (b.region_x, b.region_z, bi),
        dimension.as_deref(),
    )
    .map_err(|e| format!("Failed to swap chunks: {}", e))
}

/// Bulk NBT Changer (MCA Selector).
#[tauri::command(rename_all = "camelCase")]
fn change_world_chunks(
    path: String,
    world_name: String,
    selections: Vec<ChunkSelection>,
    change: tuffbox_core::region_edit::NbtChangeRequest,
    dimension: Option<String>,
) -> Result<usize, String> {
    let project_dir = manifest_parent(&path)?;
    let world_dir = project_dir.join("saves").join(&world_name);
    let pairs: Vec<(i32, i32, Vec<usize>)> = selections
        .into_iter()
        .map(|s| (s.region_x, s.region_z, s.indices))
        .collect();
    tuffbox_core::region_edit::change_world_chunks(
        &world_dir,
        &pairs,
        &change,
        dimension.as_deref(),
    )
    .map_err(|e| format!("Failed to change chunks: {}", e))
}

/// Read one chunk as an NBT tree for the Chunk Editor.
#[tauri::command(rename_all = "camelCase")]
fn read_chunk_editor(
    path: String,
    world_name: String,
    region_x: i32,
    region_z: i32,
    index: usize,
    dimension: Option<String>,
    layer: Option<String>,
) -> Result<tuffbox_core::region_edit::ChunkEditorData, String> {
    let project_dir = manifest_parent(&path)?;
    let world_dir = project_dir.join("saves").join(&world_name);
    tuffbox_core::region_edit::read_chunk_editor(
        &world_dir,
        region_x,
        region_z,
        index,
        dimension.as_deref(),
        layer.as_deref(),
    )
    .map_err(|e| format!("Failed to read chunk: {}", e))
}

/// Write edited NBT tree back to disk.
#[tauri::command(rename_all = "camelCase")]
fn write_chunk_editor(
    path: String,
    world_name: String,
    data: tuffbox_core::region_edit::ChunkEditorData,
    dimension: Option<String>,
) -> Result<(), String> {
    let project_dir = manifest_parent(&path)?;
    let world_dir = project_dir.join("saves").join(&world_name);
    tuffbox_core::region_edit::write_chunk_editor(&world_dir, &data, dimension.as_deref())
        .map_err(|e| format!("Failed to write chunk: {}", e))
}

/// Advanced content filter (palette / entities / structures).
#[tauri::command(rename_all = "camelCase")]
fn filter_world_chunks_advanced(
    path: String,
    world_name: String,
    filter: tuffbox_core::region_edit::AdvancedChunkFilter,
    selections: Option<Vec<ChunkSelection>>,
    dimension: Option<String>,
) -> Result<Vec<tuffbox_core::region_edit::ChunkRef>, String> {
    let project_dir = manifest_parent(&path)?;
    let world_dir = project_dir.join("saves").join(&world_name);
    let pairs: Vec<(i32, i32, Vec<usize>)> = selections
        .unwrap_or_default()
        .into_iter()
        .map(|s| (s.region_x, s.region_z, s.indices))
        .collect();
    tuffbox_core::region_edit::filter_world_chunks_advanced(
        &world_dir,
        &pairs,
        &filter,
        dimension.as_deref(),
    )
    .map_err(|e| format!("Failed to filter chunks: {}", e))
}

/// ── Export to GitHub Releases ──────────────────────────────────

/// Generates GitHub Release-compatible changelog and asset manifest.
#[tauri::command(rename_all = "camelCase")]
fn generate_github_release(
    path: String,
    tag: Option<String>,
    target: Option<String>,
) -> Result<serde_json::Value, String> {
    let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
    let project_dir = manifest_parent(&path)?;
    let version = tag.unwrap_or_else(|| manifest.project.version.clone());
    let tag_name = format!("v{}", version);
    let changelog = format!(
        "# {} {}\n\n{}",
        manifest.project.name,
        manifest.project.version,
        manifest.project.description.as_deref().unwrap_or("")
    );

    // List export artifacts
    let mut artifacts = Vec::new();
    let artifact_dir = project_dir.join(".tuffbox").join("artifacts");
    if artifact_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&artifact_dir) {
            for e in entries.flatten() {
                artifacts.push(e.file_name().to_string_lossy().to_string());
            }
        }
    }

    // Generate release body
    let body = format!(
        "## {} v{}

**Minecraft:** {} | **Loader:** {} {}

### Changelog

{}

### Installed Mods ({})

{}",
        manifest.project.name,
        version,
        manifest.minecraft.version,
        format!("{:?}", manifest.loader.kind).to_lowercase(),
        manifest.loader.version,
        changelog,
        manifest.mods.len(),
        manifest
            .mods
            .iter()
            .map(|m| format!("- {} {}", m.name, m.version))
            .take(50)
            .collect::<Vec<_>>()
            .join("\n")
    );

    let release_dir = if let Some(t) = target {
        std::path::PathBuf::from(&t)
    } else {
        project_dir.join("release")
    };
    std::fs::create_dir_all(&release_dir).map_err(|e| e.to_string())?;
    let release_json = release_dir.join("github-release.json");
    let payload = serde_json::json!({
        "tag_name": tag_name, "name": format!("{} {}", manifest.project.name, version),
        "body": body, "draft": true, "prerelease": version.contains("alpha") || version.contains("beta"),
        "artifacts": artifacts,
    });
    std::fs::write(
        &release_json,
        serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;

    Ok(serde_json::json!({
        "tagName": tag_name, "body": body,
        "releaseJsonPath": release_json.to_string_lossy().to_string(),
        "artifacts": artifacts, "artifactCount": artifacts.len(),
    }))
}

/// ── Localization helper ──────────────────────────────────────────

static L10N: once_cell::sync::Lazy<std::collections::HashMap<&str, &str>> =
    once_cell::sync::Lazy::new(|| {
        let mut m = std::collections::HashMap::new();
        // Common UI strings
        m.insert("dashboard", "Панель / Dashboard");
        m.insert("mods", "Моды");
        m.insert("graph", "Граф зависимостей");
        m.insert("settings", "Настройки");
        m.insert("launch", "Запустить / Launch");
        m.insert("open_ide", "Открыть IDE");
        m.insert("snapshots", "Снапшоты");
        m.insert("export", "Экспорт");
        m.insert("release", "Релиз");
        m.insert("diagnostics", "Диагностика");
        m.insert("configs", "Конфигурации");
        m.insert("test", "Тестовые запуски");
        m.insert("history", "История изменений");
        m.insert("back", "Назад");
        m.insert("save", "Сохранить");
        m.insert("cancel", "Отмена");
        m.insert("delete", "Удалить");
        m.insert("remove", "Убрать");
        m.insert("add", "Добавить");
        m.insert("search", "Поиск");
        m.insert("refresh", "Обновить");
        m.insert("loading", "Загрузка...");
        m.insert("error", "Ошибка");
        m.insert("success", "Успешно");
        m.insert("warning", "Предупреждение");
        m.insert("no_project", "Откройте проект");
        m
    });

/// Returns a localized string (RU/EN). Falls back to the key itself.
#[tauri::command(rename_all = "camelCase")]
fn localize(key: String) -> Result<String, String> {
    Ok(L10N.get(key.as_str()).copied().unwrap_or(&key).to_string())
}

/// Returns all localization keys (for UI reference).
#[tauri::command(rename_all = "camelCase")]
fn list_localizations() -> Result<Vec<serde_json::Value>, String> {
    Ok(L10N
        .iter()
        .map(|(k, v)| serde_json::json!({"key": k, "ru": v.split(" / ").next().unwrap_or(v)}))
        .collect())
}

/// ── Batch operations for CLI/scripting ────────────────────────────

/// Exports the dependency graph as a DOT string (Graphviz format),
/// which can be rendered to PNG/SVG with the `dot` command.
#[tauri::command(rename_all = "camelCase")]
fn export_graph_dot(path: String) -> Result<String, String> {
    let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
    let graph = DependencyGraph::from_manifest(&manifest);
    let mut dot = String::from(
        "digraph TuffBox {
",
    );
    dot.push_str(
        "  rankdir=LR;
  node [shape=box, style=filled, fillcolor=\"#18181b\", fontcolor=\"#e5e7eb\", color=\"#27272a\"];
",
    );
    dot.push_str(
        "  edge [color=\"#3f3f46\", fontcolor=\"#71717a\"];

",
    );

    for node in &graph.nodes {
        let color = match node.kind {
            tuffbox_core::graph::NodeKind::Mod => "#1bd96a22",
            tuffbox_core::graph::NodeKind::ResourcePack => "#38bdf822",
            tuffbox_core::graph::NodeKind::ShaderPack => "#c084fc22",
            tuffbox_core::graph::NodeKind::Profile => "#8b5cf622",
            _ => "#f59e0b22",
        };
        let shape = if node.kind == tuffbox_core::graph::NodeKind::Profile {
            "ellipse"
        } else if matches!(
            node.kind,
            tuffbox_core::graph::NodeKind::ResourcePack | tuffbox_core::graph::NodeKind::ShaderPack
        ) {
            "folder"
        } else {
            "box"
        };
        dot.push_str(&format!(
            "  \"{}\" [label=\"{}\", shape={}, fillcolor=\"{}\", color=\"{}\"];
",
            node.id.0,
            node.label,
            shape,
            color,
            if color.len() > 9 { &color[..7] } else { color }
        ));
    }

    for edge in &graph.edges {
        let style = if edge.kind == tuffbox_core::graph::EdgeKind::Requires {
            "solid"
        } else if edge.kind == tuffbox_core::graph::EdgeKind::Conflicts {
            "dashed, color=\"#ef4444\""
        } else {
            "dotted"
        };
        dot.push_str(&format!(
            "  \"{}\" -> \"{}\" [label=\"{:?}\", style={}];
",
            edge.from.0, edge.to.0, edge.kind, style
        ));
    }

    dot.push_str(
        "}
",
    );
    Ok(dot)
}

/// Exports the full project state as a JSON report (manifest + graph +
/// diagnostics + snapshots) for external tooling.
#[tauri::command(rename_all = "camelCase")]
fn export_project_report(path: String) -> Result<serde_json::Value, String> {
    let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
    let graph = DependencyGraph::from_manifest(&manifest);
    let diagnostics = Resolver::analyze_project(&manifest, &graph);
    let project_dir = manifest_parent(&path)?;
    let store = SnapshotStore::new(&project_dir);
    let snapshots = store.list().unwrap_or_default();

    let change_plan = Resolver::create_fix_plan(&graph, &diagnostics);

    Ok(serde_json::json!({
        "project": manifest.project,
        "minecraft": manifest.minecraft,
        "loader": { "kind": format!("{:?}", manifest.loader.kind), "version": manifest.loader.version },
        "modCount": manifest.mods.len(),
        "mods": manifest.mods.iter().map(|m| serde_json::json!({
            "id": m.id, "name": m.name, "version": m.version, "side": format!("{:?}", m.side),
            "source": format!("{:?}", m.source.kind), "contentType": format!("{:?}", m.content_type),
        })).collect::<Vec<_>>(),
        "graph": { "nodes": graph.nodes.len(), "edges": graph.edges.len() },
        "diagnostics": diagnostics.iter().map(|d| serde_json::json!({
            "severity": format!("{:?}", d.severity), "code": d.code, "message": d.message,
        })).collect::<Vec<_>>(),
        "snapshots": snapshots.len(),
        "changePlan": change_plan.map(|p| serde_json::json!({
            "summary": p.summary, "risk": format!("{:?}", p.risk),
            "actions": p.actions.len(), "requiresSnapshot": p.requires_snapshot,
        })),
        "generatedAt": tuffbox_core::time_util::rfc3339_now(),
    }))
}

/// Batch export: generates .mrpack, server pack, Prism, and CurseForge zips
/// into `<project>/export/`, recording each as a release artifact.
#[tauri::command(rename_all = "camelCase")]
async fn batch_export_all(path: String) -> Result<Vec<serde_json::Value>, String> {
    tokio::task::spawn_blocking(move || {
        let manifest_path = resolve_manifest_path(&path)?;
        let manifest =
            ProjectManifest::load_from_path(&manifest_path).map_err(|e| e.to_string())?;
        let project_dir = manifest_path
            .parent()
            .map(|p| p.to_path_buf())
            .ok_or_else(|| "manifest has no parent directory".to_string())?;
    let base = project_dir.join("export");
    std::fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    let mut results = Vec::new();
    let id = &manifest.project.id;
    let ver = &manifest.project.version;
    let manifest_path = PathBuf::from(&path);

    let zip_jobs: [(&str, &str, PathBuf); 4] = [
        ("mrpack", "mrpack", base.join(format!("{id}-{ver}.mrpack"))),
        ("server", "server", base.join(format!("{id}-{ver}-server.zip"))),
        ("prism", "prism", base.join(format!("{id}-{ver}-prism.zip"))),
        ("curseforge", "curseforge", base.join(format!("{id}-{ver}-curseforge.zip"))),
    ];

    for (kind, artifact_kind, out) in zip_jobs {
        let run = match kind {
            "mrpack" => tuffbox_core::exporter::export_modrinth_pack(
                &manifest,
                &manifest_path,
                &out,
            ),
            "server" => {
                tuffbox_core::exporter::export_server_pack(&manifest, &manifest_path, &out)
            }
            "prism" => tuffbox_core::exporter::export_prism_instance(
                &manifest,
                &manifest_path,
                &out,
            ),
            "curseforge" => tuffbox_core::exporter::export_curseforge_pack(
                &manifest,
                &manifest_path,
                &out,
            ),
            _ => unreachable!(),
        };
        match run {
            Ok(result) => {
                let _ = append_release_artifact(&path, artifact_kind, &result);
                results.push(serde_json::json!({
                    "kind": kind,
                    "path": result.path.to_string_lossy(),
                    "files": result.file_count,
                    "overrideCount": result.override_count,
                    "warnings": result.warnings,
                    "status": "ok",
                }));
            }
            Err(e) => results.push(serde_json::json!({
                "kind": kind,
                "status": "error",
                "error": e.to_string(),
            })),
        }
    }

    let packwiz_dir = base.join(format!("{id}-{ver}-packwiz"));
    match tuffbox_core::export_packwiz_pack(&manifest, &manifest_path, &packwiz_dir) {
        Ok(result) => {
            let mapped = tuffbox_core::ExportResult {
                path: result.path.clone(),
                file_count: result.file_count,
                override_count: result.override_count,
                warnings: vec![],
            };
            let _ = append_release_artifact(&path, "packwiz", &mapped);
            results.push(serde_json::json!({
                "kind": "packwiz",
                "path": result.path.to_string_lossy(),
                "files": result.file_count,
                "status": "ok",
            }));
        }
        Err(e) => results.push(serde_json::json!({
            "kind": "packwiz",
            "status": "error",
            "error": e.to_string(),
        })),
    }

    swarm_api::spawn_pack_cooccurrence(path.clone(), "pack_export");
    Ok(results)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
fn get_graph(path: String) -> Result<serde_json::Value, String> {
    let manifest_path = PathBuf::from(&path);
    let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
    let (graph, source, generated_at) =
        tuffbox_core::graph_for_click_path(&manifest_path, &manifest);
    Ok(graph_payload(graph, source, generated_at))
}

#[tauri::command(rename_all = "camelCase")]
async fn refresh_graph(app: tauri::AppHandle, path: String) -> Result<serde_json::Value, String> {
    use tauri::Emitter;

    let _ = app.emit(
        "graph-refresh-progress",
        serde_json::json!({"phase": "start", "message": "Refreshing dependency metadata"}),
    );
    let app_done = app.clone();
    let result = tokio::task::spawn_blocking(move || {
        let manifest_path = PathBuf::from(&path);
        let base = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
        let mut enriched = base.clone();
        tuffbox_core::enrich_manifest_from_installed_jars(&manifest_path, &mut enriched);
        enrich_manifest_for_graph(&mut enriched)?;
        let mut cache = tuffbox_core::GraphCache::new(&base, enriched).with_network_enriched();
        if let Some(instance_dir) = tuffbox_core::instance_dir_for_manifest(&manifest_path) {
            cache.graph.attach_disk_content_packs(&instance_dir);
        }
        cache.save(&manifest_path)?;
        Ok::<_, String>(graph_payload(
            cache.graph,
            "network",
            Some(cache.generated_at),
        ))
    })
    .await
    .map_err(|e| e.to_string())?;

    let _ = app_done.emit(
        "graph-refresh-progress",
        match &result {
            Ok(_) => serde_json::json!({"phase": "done", "message": "Dependency graph is current"}),
            Err(error) => serde_json::json!({"phase": "error", "message": error}),
        },
    );
    result
}

fn graph_payload(
    graph: DependencyGraph,
    source: &str,
    generated_at: Option<String>,
) -> serde_json::Value {
    serde_json::json!({
        "nodes": graph.nodes,
        "edges": graph.edges,
        "source": source,
        "generatedAt": generated_at,
    })
}

fn manifest_for_graph(path: &str) -> Result<ProjectManifest, String> {
    let manifest_path = PathBuf::from(path);
    let manifest = ProjectManifest::load_from_path(path).map_err(|e| e.to_string())?;
    Ok(tuffbox_core::enriched_manifest_for_click_path(&manifest_path, &manifest).0)
}

/// Fills Modrinth dependency edges and icon URLs in-memory so the graph view
/// shows real mod-to-mod links. Always refreshes dependency lists from Modrinth
/// (project id → slug normalized) so edges resolve onto installed mod nodes.
/// Also backfills provider categories (Modrinth + CurseForge) for graph clustering.
fn enrich_manifest_for_graph(manifest: &mut ProjectManifest) -> Result<(), String> {
    use rayon::prelude::*;

    let query = tuffbox_core::ProviderSearchQuery {
        query: None,
        minecraft_version: Some(manifest.minecraft.version.clone()),
        loader: Some(tuffbox_core::graph::loader_kind_slug(&manifest.loader.kind).to_string()),
        ..Default::default()
    };

    manifest.mods.par_iter_mut().for_each(|module| {
        match module.source.kind {
            tuffbox_core::manifest::SourceKind::Modrinth => {
                let provider = tuffbox_core::ModrinthProvider::new();
                let project_id = module
                    .source
                    .project_id
                    .clone()
                    .unwrap_or_else(|| module.id.clone());

                let version_id = if let Some(file_id) = module.source.file_id.clone() {
                    Some(file_id)
                } else if let Ok(versions) = provider.get_versions(&project_id, &query) {
                    versions.into_iter().next().map(|v| v.id)
                } else {
                    None
                };

                if let Some(version_id) = version_id {
                    if let Ok(deps) = provider.resolve_dependencies(&version_id) {
                        module.dependencies = deps;
                    }
                }

                // Fetch the project once to backfill both the icon and the site
                // categories (Modrinth tags). Categories drive the graph clustering,
                // so we refresh them even when the icon is already cached.
                if module.source.icon_url.is_none() || module.source.categories.is_empty() {
                    if let Ok(project) = provider.get_project(&project_id) {
                        if module.source.icon_url.is_none() {
                            module.source.icon_url = project.icon_url;
                        }
                        if !project.categories.is_empty() {
                            module.source.categories = project.categories;
                        }
                    }
                }
            }
            tuffbox_core::manifest::SourceKind::Curseforge => {
                if !module.source.categories.is_empty() && module.source.icon_url.is_some() {
                    return;
                }
                let Some(project_id_str) = module.source.project_id.as_deref() else {
                    return;
                };
                let Ok(project_id) = project_id_str.parse::<u64>() else {
                    return;
                };
                let provider = tuffbox_core::CurseForgeProvider::new();
                if !provider.is_configured() {
                    return;
                }
                if let Ok(hit) = provider.get_mod(project_id) {
                    if module.source.icon_url.is_none() {
                        module.source.icon_url = hit.icon_url;
                    }
                    if module.source.categories.is_empty() && !hit.categories.is_empty() {
                        module.source.categories = hit
                            .categories
                            .iter()
                            .map(|c| tuffbox_core::normalize_mod_category(c))
                            .collect();
                    }
                }
            }
            _ => {}
        }
    });
    Ok(())
}

fn diagnostics_cache_key(
    manifest_path: &Path,
    manifest: &ProjectManifest,
) -> String {
    format!(
        "diagnostics:{}:{}",
        tuffbox_core::manifest_fingerprint(manifest),
        tuffbox_core::installed_jars_fingerprint(manifest_path)
    )
}

fn diagnostics_for_path_cached(
    manifest_path: &Path,
    manifest: &ProjectManifest,
) -> tuffbox_core::ClickPathDiagnostics {
    let key = diagnostics_cache_key(manifest_path, manifest);
    if let Some(cached) =
        tuffbox_core::api_cache::get::<tuffbox_core::ClickPathDiagnostics>(&key)
    {
        return cached;
    }
    let _guard = DIAGNOSTICS_IO_LOCK.lock().ok();
    // Re-check after waiting: another consumer may have populated the cache
    // while this caller was blocked.
    if let Some(cached) =
        tuffbox_core::api_cache::get::<tuffbox_core::ClickPathDiagnostics>(&key)
    {
        return cached;
    }
    let result = tuffbox_core::diagnostics_for_click_path(manifest_path, manifest);
    tuffbox_core::api_cache::put_with_ttl(
        key,
        result.clone(),
        std::time::Duration::from_secs(5),
    );
    result
}

#[tauri::command]
fn get_diagnostics(path: String) -> Result<Vec<tuffbox_core::Diagnostic>, String> {
    let manifest_path = PathBuf::from(&path);
    let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
    Ok(diagnostics_for_path_cached(&manifest_path, &manifest).diagnostics)
}

#[tauri::command(rename_all = "camelCase")]
fn get_diagnostic_counts(path: String) -> Result<tuffbox_core::DiagnosticCounts, String> {
    let manifest_path = PathBuf::from(&path);
    let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
    let result = diagnostics_for_path_cached(&manifest_path, &manifest);
    Ok(tuffbox_core::diagnostic_counts(
        &result.diagnostics,
        result.cached,
    ))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PackHealthDiagnostics {
    errors: usize,
    warnings: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PackHealthIssue {
    severity: String,
    code: String,
    message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PackHealthDuplicateGroup {
    mod_id: String,
    keep_candidate: String,
    count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PackHealthLastCrash {
    at: String,
    exit_code: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum PackHealthOverall {
    Healthy,
    Warnings,
    Errors,
}

/// Aggregate pack-health snapshot for one project. Composes the same core
/// checks the individual screens run: diagnostics, Modrinth export
/// validation, wrong-loader scan, duplicate-jar scan, quest book validation,
/// plus the newest crash from the launch journal.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PackHealthReport {
    diagnostics: PackHealthDiagnostics,
    export_issues: Vec<PackHealthIssue>,
    wrong_loader_count: usize,
    duplicate_groups: Vec<PackHealthDuplicateGroup>,
    quest_issues: usize,
    last_crash: Option<PackHealthLastCrash>,
    overall: PackHealthOverall,
}

/// Best-effort per section: only manifest load failures fail the command;
/// an unreadable quest book or empty launch journal simply contributes nothing.
fn get_pack_health_impl(path: &str) -> Result<PackHealthReport, String> {
    let manifest_path = PathBuf::from(path);
    let manifest = ProjectManifest::load_from_path(path).map_err(|e| e.to_string())?;
    let project_dir = manifest_parent(path)?;

    // Same core helper as get_diagnostics.
    let diagnostics = diagnostics_for_path_cached(&manifest_path, &manifest).diagnostics;
    let diag_errors = diagnostics
        .iter()
        .filter(|d| d.severity == tuffbox_core::DiagnosticSeverity::Error)
        .count();
    let diag_warnings = diagnostics
        .iter()
        .filter(|d| d.severity == tuffbox_core::DiagnosticSeverity::Warning)
        .count();

    // Same core helper as validate_modrinth_export.
    let export_issues: Vec<PackHealthIssue> = tuffbox_core::validate_modrinth_export(&manifest)
        .into_iter()
        .map(|issue| PackHealthIssue {
            severity: match issue.severity {
                tuffbox_core::ExportIssueSeverity::Error => "error",
                tuffbox_core::ExportIssueSeverity::Warning => "warning",
            }
            .to_string(),
            code: issue.code,
            message: issue.message,
        })
        .collect();

    // Shared scanner with detect_wrong_loader_mods.
    let project_loader = tuffbox_core::graph::loader_kind_slug(&manifest.loader.kind).to_string();
    let tracked: Vec<String> = manifest
        .mods
        .iter()
        .filter_map(|m| m.file_name.clone())
        .collect();
    let wrong_loader_count =
        scan_wrong_loader_jars(&project_dir.join("mods"), &project_loader, &tracked).len();

    // Shared scanner with detect_duplicate_mod_jars.
    let tracked_set: std::collections::HashSet<String> = tracked.into_iter().collect();
    let duplicate_groups: Vec<PackHealthDuplicateGroup> =
        scan_duplicate_mod_jars(&project_dir.join("mods"), &tracked_set)
            .into_iter()
            .map(|g| PackHealthDuplicateGroup {
                count: g.jars.len(),
                mod_id: g.mod_id,
                keep_candidate: g.keep_candidate,
            })
            .collect();

    // Same validation as validate_quest_book; a missing/unreadable book is not
    // itself a quest issue.
    let quest_issues = tuffbox_core::unified::QuestBook::load_from_project(&project_dir)
        .map(|book| {
            let available = collect_catalog_item_ids_click_path(&manifest_path);
            if available.is_empty() {
                book.validate()
            } else {
                book.validate_with_items(Some(&available))
            }
            .len()
        })
        .unwrap_or(0);

    // Newest non-clean exit from the launch journal: crashes are recorded via
    // archive_crashed_session with exit_code != Some(0); healthy exits record
    // Some(0). None when there is no history yet.
    let last_crash = tuffbox_core::launch_history::list_launch_history_default(&project_dir)
        .into_iter()
        .find(|e| e.exit_code != Some(0))
        .map(|e| PackHealthLastCrash {
            at: e.ended_at,
            exit_code: e.exit_code,
        });

    // A recent non-zero launch exit is a real health failure even when the
    // static graph is clean. Previously `last_crash` was displayed in the
    // report but did not affect the overall verdict, so the badge could say
    // Healthy immediately after a crash.
    let export_errors = export_issues.iter().any(|issue| issue.severity == "error");
    let overall = if diag_errors > 0
        || wrong_loader_count > 0
        || last_crash.is_some()
        || export_errors
    {
        PackHealthOverall::Errors
    } else if diag_warnings > 0
        || !export_issues.is_empty()
        || !duplicate_groups.is_empty()
        || quest_issues > 0
    {
        PackHealthOverall::Warnings
    } else {
        PackHealthOverall::Healthy
    };

    Ok(PackHealthReport {
        diagnostics: PackHealthDiagnostics {
            errors: diag_errors,
            warnings: diag_warnings,
        },
        export_issues,
        wrong_loader_count,
        duplicate_groups,
        quest_issues,
        last_crash,
        overall,
    })
}

/// Aggregated pack health across all per-screen checks (backend for the
/// Pack Health panel). Heavy scans run on the blocking pool.
#[tauri::command(rename_all = "camelCase")]
async fn get_pack_health(path: String) -> Result<PackHealthReport, String> {
    // The header badge and Diagnose can request the same report at once. Use
    // a short project-scoped cache so opening Diagnose does not start a second
    // full quest/export/JAR scan while the badge is still rendering.
    let manifest_path = PathBuf::from(&path);
    let project_dir = manifest_path.parent().map(Path::to_path_buf);
    let manifest_mtime = file_mtime_nanos(&manifest_path).unwrap_or(0);
    let mods_mtime = project_dir
        .as_ref()
        .map(|dir| dir_mtime_nanos(&dir.join("mods")).unwrap_or(0))
        .unwrap_or(0);
    let history_mtime = project_dir
        .as_ref()
        .map(|dir| file_mtime_nanos(&dir.join(".tuffbox/history/launches.jsonl")).unwrap_or(0))
        .unwrap_or(0);
    let cache_key = format!(
        "pack-health:{path}:{manifest_mtime}:{mods_mtime}:{history_mtime}"
    );
    if let Some(cached) = tuffbox_core::api_cache::get::<PackHealthReport>(&cache_key) {
        return Ok(cached);
    }

    let report = tokio::task::spawn_blocking(move || -> Result<PackHealthReport, String> {
            let _guard = PACK_HEALTH_IO_LOCK.lock().ok();
        // Re-check after waiting so simultaneous badge/Diagnose requests
        // share one cold scan instead of all running export/JAR/quest checks.
        if let Some(cached) = tuffbox_core::api_cache::get::<PackHealthReport>(&cache_key) {
            return Ok(cached);
        }
        let report = get_pack_health_impl(&path)?;
        tuffbox_core::api_cache::put_with_ttl(
            cache_key,
            report.clone(),
            std::time::Duration::from_secs(10),
        );
        Ok(report)
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(report)
}

/// Aggregated health report for a project. Combines the dependency-graph
/// diagnostics with crash flags, Modrinth export blockers and missing-file
/// counts so the Health screen can render one verdict instead of stitching
/// together several unrelated endpoints (spec: «Один агрегат HealthReport»).
#[tauri::command(rename_all = "camelCase")]
async fn get_health_report(path: String) -> Result<serde_json::Value, String> {
    // Sync Tauri commands run on the MAIN thread; this one walked the graph
    // and sha1-hashed every non-local jar, freezing the event loop and
    // stalling delivery of every other IPC response (Diagnose spinners
    // never settled). Heavy work belongs on the blocking pool (same
    // treatment as get_crash_diagnosis / run_crash_assistant_full).
    tokio::task::spawn_blocking(move || get_health_report_impl(path))
        .await
        .map_err(|e| e.to_string())?
}

fn get_health_report_impl(path: String) -> Result<serde_json::Value, String> {
    let manifest_path = resolve_manifest_path(&path).map_err(|e| e.to_string())?;
    let manifest = ProjectManifest::load_from_path(&manifest_path).map_err(|e| e.to_string())?;
    let graph = DependencyGraph::from_manifest(&manifest);
    let diagnostics = Resolver::analyze_project(&manifest, &graph);
    let project_dir = manifest_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_default();

    let error_count = diagnostics
        .iter()
        .filter(|d| d.severity == tuffbox_core::DiagnosticSeverity::Error)
        .count();
    let warning_count = diagnostics
        .iter()
        .filter(|d| d.severity == tuffbox_core::DiagnosticSeverity::Warning)
        .count();

    // Crash flags: any crash-report file present under crash-reports/.
    let crash_reports = project_dir.join("crash-reports");
    let has_crash = crash_reports.is_dir()
        && std::fs::read_dir(&crash_reports)
            .map(|rd| {
                rd.flatten().any(|e| {
                    e.path().is_file()
                        && e.path()
                            .extension()
                            .map_or(false, |x| x.eq_ignore_ascii_case("txt"))
                })
            })
            .unwrap_or(false);

    // Modrinth export blockers (error-severity only are blockers).
    let export_issues = tuffbox_core::validate_modrinth_export(&manifest);
    let export_blockers: Vec<serde_json::Value> = export_issues
        .iter()
        .filter(|i| matches!(i.severity, tuffbox_core::exporter::ExportIssueSeverity::Error))
        .map(|i| {
            serde_json::json!({
                "code": i.code,
                "message": i.message,
                "target": i.target,
            })
        })
        .collect();

    // Missing / hash-mismatched content files (best-effort: resolve against
    // the instance dir; only a count + failed names so this stays cheap).
    let mut missing_files: Vec<String> = Vec::new();
    let mut missing_hashes: Vec<String> = Vec::new();
    let instance_dir = tuffbox_core::instance_dir_for_manifest(&manifest_path)
        .unwrap_or_else(|| project_dir.clone());
    for module in &manifest.mods {
        let Some(file_name) = module.file_name.as_deref().filter(|f| !f.is_empty()) else {
            continue;
        };
        if module.source.kind == tuffbox_core::manifest::SourceKind::Local {
            continue;
        }
        let content_dir = tuffbox_core::content_dir_for(&instance_dir, module.content_type);
        let target = content_dir.join(file_name);
        if !target.is_file() {
            missing_files.push(file_name.to_string());
            continue;
        }
        if let Some(expected) = module
            .hashes
            .as_ref()
            .and_then(|h| h.sha1.as_deref())
            .filter(|h| !h.is_empty())
        {
            if let Ok(actual) = tuffbox_core::mc_install::sha1_file(&target) {
                if !actual.eq_ignore_ascii_case(expected) {
                    missing_hashes.push(file_name.to_string());
                }
            }
        }
    }

    let diag_json: Vec<serde_json::Value> = diagnostics
        .iter()
        .map(|d| {
            serde_json::json!({
                "severity": format!("{:?}", d.severity).to_lowercase(),
                "code": d.code,
                "message": d.message,
                "relatedNodes": d.related_nodes,
            })
        })
        .collect();

    Ok(serde_json::json!({
        "manifestPath": manifest_path,
        "diagnostics": diag_json,
        "errorCount": error_count,
        "warningCount": warning_count,
        "hasCrash": has_crash,
        "crashReports": if crash_reports.is_dir() {
            std::fs::read_dir(&crash_reports)
                .map(|rd| {
                    rd.flatten()
                        .filter(|e| {
                            e.path().is_file()
                                && e.path().extension().map_or(false, |x| x.eq_ignore_ascii_case("txt"))
                        })
                        .map(|e| e.file_name().to_string_lossy().to_string())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        },
        "exportBlockers": export_blockers,
        "missingFiles": missing_files,
        "missingHashes": missing_hashes,
        "missingCount": missing_files.len(),
        "hashMismatchCount": missing_hashes.len(),
    }))
}

#[tauri::command(rename_all = "camelCase")]
async fn get_resolve_change_plan(path: String) -> Result<Option<tuffbox_core::ChangePlan>, String> {
    // Blocking-pool conversion: sync commands run on the main thread (see
    // get_health_report); heavy I/O must not freeze IPC delivery.
    tokio::task::spawn_blocking(move || get_resolve_change_plan_impl(path))
        .await
        .map_err(|e| e.to_string())?
}

fn get_resolve_change_plan_impl(path: String) -> Result<Option<tuffbox_core::ChangePlan>, String> {
    let manifest = manifest_for_graph(&path)?;
    let graph = DependencyGraph::from_manifest(&manifest);
    let diagnostics = Resolver::analyze_project(&manifest, &graph);
    Ok(Resolver::create_fix_plan(&graph, &diagnostics))
}

#[tauri::command(rename_all = "camelCase")]
async fn apply_resolve_action(
    app: tauri::AppHandle,
    path: String,
    action_index: usize,
) -> Result<Vec<String>, String> {
    tokio::task::spawn_blocking(move || {
        let manifest_path = PathBuf::from(&path);
        let mut manifest = manifest_for_graph(&path)?;
        let expected_fingerprint = tuffbox_core::manifest_fingerprint(&manifest);
        let graph = DependencyGraph::from_manifest(&manifest);
        let diagnostics = Resolver::analyze_project(&manifest, &graph);
        let Some(plan) = Resolver::create_fix_plan(&graph, &diagnostics) else {
            return Ok(Vec::new());
        };
        let Some(action) = plan.actions.get(action_index).cloned() else {
            return Err(format!("action index {action_index} out of range"));
        };
        let current = manifest_for_graph(&path)?;
        if tuffbox_core::manifest_fingerprint(&current) != expected_fingerprint {
            return Err("Project changed while diagnostics were running; refresh diagnostics and review the new plan.".into());
        }

        if plan.requires_snapshot {
            auto_snapshot(&manifest_path, "apply-resolve-action").map_err(|e| e.to_string())?;
        }
        let mut applied = Vec::new();
        apply_change_action(&manifest_path, &mut manifest, action, &mut applied)?;
        let current = manifest_for_graph(&path)?;
        if tuffbox_core::manifest_fingerprint(&current) != expected_fingerprint {
            return Err("Project changed while applying the plan; no changes were written. Refresh diagnostics.".into());
        }
        save_manifest(&manifest_path, &manifest).map_err(|e| e.to_string())?;
        download_project_mods_tracked(&app, &manifest_path, &manifest, None, true);
        Ok(applied)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
async fn apply_resolve_change_plan(
    app: tauri::AppHandle,
    path: String,
) -> Result<Vec<String>, String> {
    tokio::task::spawn_blocking(move || {
        let manifest_path = PathBuf::from(&path);
        let mut manifest = manifest_for_graph(&path)?;
        let expected_fingerprint = tuffbox_core::manifest_fingerprint(&manifest);
        let graph = DependencyGraph::from_manifest(&manifest);
        let diagnostics = Resolver::analyze_project(&manifest, &graph);
        let Some(plan) = Resolver::create_fix_plan(&graph, &diagnostics) else {
            return Ok(Vec::new());
        };
        let current = manifest_for_graph(&path)?;
        if tuffbox_core::manifest_fingerprint(&current) != expected_fingerprint {
            return Err("Project changed while diagnostics were running; refresh diagnostics and review the new plan.".into());
        }

        if plan.requires_snapshot {
            auto_snapshot(&manifest_path, "apply-resolve-plan").map_err(|e| e.to_string())?;
        }
        let mut applied = Vec::new();
        for action in plan.actions {
            apply_change_action(&manifest_path, &mut manifest, action, &mut applied)?;
        }
        let current = manifest_for_graph(&path)?;
        if tuffbox_core::manifest_fingerprint(&current) != expected_fingerprint {
            return Err("Project changed while applying the plan; no changes were written. Refresh diagnostics.".into());
        }
        save_manifest(&manifest_path, &manifest).map_err(|e| e.to_string())?;
        download_project_mods_tracked(&app, &manifest_path, &manifest, None, true);
        Ok(applied)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
async fn resolve_missing_dependencies(
    app: tauri::AppHandle,
    path: String,
) -> Result<Vec<String>, String> {
    tokio::task::spawn_blocking(move || {
        let manifest_path = PathBuf::from(&path);
        // Use the same cached dependency edges the graph view shows.
        let mut manifest = manifest_for_graph(&path)?;
        let existing_ids = manifest
            .mods
            .iter()
            .map(|module| module.id.clone())
            .collect::<std::collections::HashSet<_>>();
        let graph = DependencyGraph::from_manifest(&manifest);
        let diagnostics = Resolver::analyze_project(&manifest, &graph);
        let mut missing = diagnostics
            .iter()
            .filter(|d| d.code == "MISSING_DEPENDENCY")
            .filter_map(|d| d.related_nodes.last())
            .filter_map(|id| id.0.strip_prefix("mod:").map(|s| s.to_string()))
            .collect::<Vec<_>>();
        missing.sort();
        missing.dedup();
        if missing.is_empty() {
            return Ok(Vec::new());
        }
        auto_snapshot(&manifest_path, "resolve-dependencies").map_err(|e| e.to_string())?;
        // Use recursive resolution: install direct deps + transitive deps
        let installed = install_modrinth_with_dependencies(&mut manifest, &missing, "auto", None)?;
        save_manifest(&manifest_path, &manifest).map_err(|e| e.to_string())?;
        let installed_ids = manifest
            .mods
            .iter()
            .filter(|module| !existing_ids.contains(&module.id))
            .map(|module| module.id.clone())
            .collect::<std::collections::HashSet<_>>();
        download_project_mods_tracked(&app, &manifest_path, &manifest, Some(&installed_ids), true);
        Ok(installed)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Installs a single missing dependency from the graph. The `mod_id` can be
/// either a Modrinth project ID (e.g. "AANobbMI") or a slug (e.g. "malilib").
/// Used by the graph "Install" button on ghost/missing nodes.
#[tauri::command(rename_all = "camelCase")]
async fn install_graph_dep(
    app: tauri::AppHandle,
    path: String,
    mod_id: String,
) -> Result<Vec<String>, String> {
    tokio::task::spawn_blocking(move || {
        let manifest_path = PathBuf::from(&path);
        let mut manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
        // Skip if already installed (by slug or project_id)
        if manifest
            .mods
            .iter()
            .any(|m| m.id == mod_id || m.source.project_id.as_deref() == Some(mod_id.as_str()))
        {
            return Ok(Vec::new());
        }
        let existing_ids = manifest
            .mods
            .iter()
            .map(|module| module.id.clone())
            .collect::<std::collections::HashSet<_>>();
        auto_snapshot(&manifest_path, "install-graph-dep").map_err(|e| e.to_string())?;
        // Recursive: install the dep + all its transitive dependencies
        let installed = install_modrinth_with_dependencies(&mut manifest, &[mod_id], "auto", None)?;
        if installed.is_empty() {
            return Err(format!(
                "Failed to install dependency: not found on Modrinth or already installed"
            ));
        }
        save_manifest(&manifest_path, &manifest).map_err(|e| e.to_string())?;
        let installed_ids = manifest
            .mods
            .iter()
            .filter(|module| !existing_ids.contains(&module.id))
            .map(|module| module.id.clone())
            .collect::<std::collections::HashSet<_>>();
        download_project_mods_tracked(&app, &manifest_path, &manifest, Some(&installed_ids), true);
        Ok(installed)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Downloads files for mods that are in the manifest but whose jar/resource
/// file is missing from disk. Returns the list of mod IDs that were
/// successfully downloaded.
#[tauri::command(rename_all = "camelCase")]
async fn download_missing_files(
    app: tauri::AppHandle,
    path: String,
) -> Result<Vec<String>, String> {
    tokio::task::spawn_blocking(move || {
        let manifest_path = PathBuf::from(&path);
        let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
        let report = download_project_mods_tracked(&app, &manifest_path, &manifest, None, true);
        Ok(report.downloaded)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
async fn get_crash_diagnosis(
    app: tauri::AppHandle,
    path: String,
    report_id: Option<String>,
) -> Result<tuffbox_core::crash::CrashDiagnosis, String> {
    // Task #66: heavy log/jar analysis must not block the IPC thread — run on
    // the blocking pool so the UI stays responsive and other commands queue up.
    let result = tokio::task::spawn_blocking(move || get_crash_diagnosis_impl(Some(&app), path, report_id))
        .await
        .map_err(|e| e.to_string())?;
    // Task lifecycle is closed by the impl on both paths; nothing here.
    result
}

/// TaskProgress id for the shared Diagnose pipeline. Both the diagnosis sweep
/// and the follow-up assistant/AI stages publish stage labels under it, so the
/// TaskProgressPanel (and the Diagnose status bar, which also polls it) shows
/// what the backend is actually doing instead of a bare spinner.
pub const DIAGNOSE_TASK_ID: &str = "diagnose";

/// Publish a diagnose stage label (indeterminate progress) to the task bus and
/// mirror it as a webview event for instant UI feedback. Errors are swallowed:
/// progress reporting must never break the diagnosis itself.
fn diagnose_stage(app: &tauri::AppHandle, stage: &str) {
    use tauri::Emitter;
    tuffbox_core::task_progress::set_running_stage(DIAGNOSE_TASK_ID, stage);
    let _ = app.emit("diagnose-progress", serde_json::json!({ "stage": stage }));
}

/// Same as [`diagnose_stage`] but works without an AppHandle (task-bus only).
fn diagnose_stage_or_task(app: Option<&tauri::AppHandle>, stage: &str) {
    tuffbox_core::task_progress::set_running_stage(DIAGNOSE_TASK_ID, stage);
    if let Some(app) = app {
        use tauri::Emitter;
        let _ = app.emit("diagnose-progress", serde_json::json!({ "stage": stage }));
    }
}

/// Emits measurable phase data instead of relying on subjective spinner time.
/// The event is intentionally additive: older frontends can ignore it while
/// profiling builds can collect per-stage timings without changing results.
fn diagnose_timing(
    app: Option<&tauri::AppHandle>,
    phase: &str,
    started: std::time::Instant,
    cache_hit: bool,
) {
    let elapsed_ms = started.elapsed().as_millis() as u64;
    if let Some(app) = app {
        use tauri::Emitter;
        let _ = app.emit(
            "diagnose-timing",
            serde_json::json!({
                "phase": phase,
                "elapsedMs": elapsed_ms,
                "cacheHit": cache_hit,
            }),
        );
    }
    // Keep a concise native trace for packaged builds where the webview event
    // stream is not being observed.
    eprintln!("[diagnose] phase={phase} elapsed_ms={elapsed_ms} cache_hit={cache_hit}");
}

fn diagnose_finish(_app: &tauri::AppHandle, ok: bool, detail: &str) {
    if ok {
        tuffbox_core::task_progress::succeed(DIAGNOSE_TASK_ID, Some(detail.to_string()));
    } else {
        tuffbox_core::task_progress::fail(DIAGNOSE_TASK_ID, detail.to_string());
    }
}

fn get_crash_diagnosis_impl(
    app: Option<&tauri::AppHandle>,
    path: String,
    report_id: Option<String>,
) -> Result<tuffbox_core::crash::CrashDiagnosis, String> {
    let total_started = std::time::Instant::now();
    tuffbox_core::task_progress::start_task(DIAGNOSE_TASK_ID, "Crash diagnosis");
    if let Some(app) = app {
        diagnose_stage(app, "Reading logs and pack graph…");
    } else {
        tuffbox_core::task_progress::set_running_stage(
            DIAGNOSE_TASK_ID,
            "Reading logs and pack graph…",
        );
    }
    // Reopening the Diagnose tab re-ran the full log/jar analysis every time.
    // Cache keyed by the input files' mtimes: any change to latest.log /
    // crash-reports / mods folder produces a new key (correctness), while
    // repeated tab switches within the same state hit the cache instantly.
    let cache_key = crash_diagnosis_cache_key(&path, report_id.as_deref());
    if let Some(cached) =
        tuffbox_core::api_cache::get::<tuffbox_core::crash::CrashDiagnosis>(&cache_key)
    {
        if let Some(app) = app {
            diagnose_finish(app, true, "Loaded from cache");
        } else {
            tuffbox_core::task_progress::succeed(DIAGNOSE_TASK_ID, Some("Loaded from cache".into()));
        }
        diagnose_timing(app, "diagnosis_total", total_started, true);
        return Ok(cached);
    }
    let result = get_crash_diagnosis_uncached(app, &path, report_id.clone());
    diagnose_timing(app, "diagnosis_total", total_started, false);
    // Cache the computed diagnosis under the same mtime key the lookup above
    // reads. The `get` existed but the result was never stored, so the
    // "Loaded from cache" fast path was dead code and every tab open /
    // refresh / source switch re-ran the full log + jar + graph analysis.
    // The key already covers all file inputs (mtimes); the short TTL only
    // bounds memory (each entry carries multi-hundred-KB log tails).
    if let Ok(diagnosis) = &result {
        tuffbox_core::api_cache::put_with_ttl(
            cache_key,
            diagnosis.clone(),
            std::time::Duration::from_secs(60),
        );
    }
    let finish_detail = match &result {
        Ok(d) => format!("{} hint(s), {} suspect(s)", d.hints.len(), d.suspected_mods.len()),
        Err(e) => e.clone(),
    };
    match app {
        Some(app) => diagnose_finish(app, result.is_ok(), &finish_detail),
        None => {
            if result.is_ok() {
                tuffbox_core::task_progress::succeed(DIAGNOSE_TASK_ID, Some(finish_detail));
            } else {
                tuffbox_core::task_progress::fail(DIAGNOSE_TASK_ID, finish_detail);
            }
        }
    }
    result
}

/// Cache key: project + selected source + mtimes of everything the analysis
/// reads (latest.log, newest crash report, manifest, mods folder timestamp).
fn crash_diagnosis_cache_key(path: &str, report_id: Option<&str>) -> String {
    let manifest_mtime = resolve_manifest_path(path)
        .ok()
        .and_then(|p| file_mtime_nanos(&p))
        .unwrap_or(0);
    let project_dir = resolve_manifest_path(path)
        .ok()
        .and_then(|p| manifest_parent(&p.to_string_lossy().to_string()).ok())
        .unwrap_or_default();
    let latest_mtime = file_mtime_nanos(&project_dir.join("logs").join("latest.log")).unwrap_or(0);
    let crash_mtime = newest_crash_report_mtime(&project_dir).unwrap_or(0);
    let mods_mtime = dir_mtime_nanos(&project_dir.join("mods")).unwrap_or(0);
    format!(
        "crashdiag:{}:{:?}:{}:{}:{}:{}",
        path, report_id, manifest_mtime, latest_mtime, crash_mtime, mods_mtime
    )
}

fn file_mtime_nanos(path: &Path) -> Option<u128> {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
}

fn dir_mtime_nanos(path: &Path) -> Option<u128> {
    let mut latest = file_mtime_nanos(path);
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let m = file_mtime_nanos(&entry.path()).unwrap_or(0);
            latest = Some(latest.unwrap_or(0).max(m));
        }
    }
    latest
}

fn newest_crash_report_mtime(project_dir: &Path) -> Option<u128> {
    let dir = project_dir.join("crash-reports");
    let mut latest: Option<u128> = None;
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            if entry.path().extension().and_then(|e| e.to_str()) == Some("txt") {
                let m = file_mtime_nanos(&entry.path());
                latest = match (latest, m) {
                    (Some(a), Some(b)) => Some(a.max(b)),
                    (None, Some(b)) => Some(b),
                    (a, None) => a,
                };
            }
        }
    }
    latest
}

fn get_crash_diagnosis_uncached(
    app: Option<&tauri::AppHandle>,
    path: &str,
    report_id: Option<String>,
) -> Result<tuffbox_core::crash::CrashDiagnosis, String> {
    let manifest_path = resolve_manifest_path(&path)?;
    let path_str = manifest_path.to_string_lossy().to_string();
    let manifest = ProjectManifest::load_from_path(&manifest_path).map_err(|e| e.to_string())?;
    let project_dir = manifest_parent(&path_str)?;
    let mut snapshots = SnapshotStore::new(&project_dir).list().unwrap_or_default();
    snapshots.reverse();
    snapshots.truncate(6);
    diagnose_stage_or_task(app, "Analyzing crash report and logs…");
    let base_started = std::time::Instant::now();
    let mut diagnosis = tuffbox_core::crash::build_crash_diagnosis(
        &project_dir,
        &manifest,
        report_id.as_deref(),
        snapshots,
    )
    .map_err(|e| e.to_string())?;
    diagnose_timing(app, "base_crash_diagnosis", base_started, false);

    // Merge Crash Assistant log-phrase findings into hints so each detect
    // gets one-by-one FixAction buttons in the Problems / Recommended panels.
    // Skip when the live session is healthy — those detectors often match
    // leftover ERROR lines from a previously fixed crash.
    let assistant_started = std::time::Instant::now();
    if !diagnosis.session_healthy {
        diagnose_stage_or_task(app, "Scanning mod jars for suspects…");
        if let Ok(assistant) =
            run_crash_assistant_analysis(&path_str, &manifest, &project_dir, report_id.as_deref())
        {
            for finding in assistant.findings {
                let id = format!("ca:{}", finding.code);
                if diagnosis.hints.iter().any(|h| h.id == id) {
                    continue;
                }
                let mut detail = finding.description.clone();
                if let Some(ev) = finding.evidence.as_ref() {
                    detail.push_str("\n\nLog evidence:\n");
                    detail.push_str(ev);
                }
                let steps = finding
                    .auto_fix
                    .clone()
                    .into_iter()
                    .collect::<Vec<_>>();
                let related: Vec<String> = finding
                    .fixes
                    .iter()
                    .filter_map(|f| f.mod_id.clone())
                    .collect();
                let fix = finding.fixes.first().cloned();
                diagnosis.hints.push(tuffbox_core::crash::DiagnosisHint {
                    id,
                    title: finding.title,
                    severity: finding.severity,
                    detail,
                    steps,
                    related_mods: related,
                    fix,
                    fixes: finding.fixes,
                });
            }
        }
    }
    diagnose_timing(app, "crash_assistant_rules", assistant_started, false);

    Ok(diagnosis)
}

#[tauri::command(rename_all = "camelCase")]
async fn import_external_crash(
    path: String,
    file_name: String,
    content: String,
) -> Result<String, String> {
    // Blocking-pool conversion: sync commands run on the main thread (see
    // get_health_report); heavy I/O must not freeze IPC delivery.
    tokio::task::spawn_blocking(move || import_external_crash_impl(path, file_name, content))
        .await
        .map_err(|e| e.to_string())?
}

fn import_external_crash_impl(
    path: String,
    file_name: String,
    content: String,
) -> Result<String, String> {
    let project_dir = manifest_parent(&path)?;
    tuffbox_core::crash::import_external_crash(&project_dir, &file_name, &content)
        .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
fn export_diagnose_support_pack(
    path: String,
    report_id: Option<String>,
    findings_summary: String,
    recent_events_summary: String,
    action_plan_json: Option<String>,
) -> Result<tuffbox_core::crash::SupportPackResult, String> {
    let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
    let project_dir = manifest_parent(&path)?;
    let mod_ids: Vec<String> = manifest.mods.iter().map(|m| m.id.clone()).collect();
    tuffbox_core::crash::export_diagnose_support_pack(
        &project_dir,
        report_id.as_deref(),
        &findings_summary,
        &recent_events_summary,
        action_plan_json.as_deref(),
        &mod_ids,
    )
    .map_err(|e| e.to_string())
}

fn run_crash_assistant_analysis(
    path: &str,
    manifest: &ProjectManifest,
    project_dir: &Path,
    report_id: Option<&str>,
) -> Result<tuffbox_core::crash_assistant::CrashAnalysisReport, String> {
    let cache_key = format!(
        "crash-assistant-report:{}",
        crash_diagnosis_cache_key(path, report_id)
    );
    if let Some(cached) =
        tuffbox_core::api_cache::get::<tuffbox_core::crash_assistant::CrashAnalysisReport>(
            &cache_key,
        )
    {
        return Ok(cached);
    }

    // Scope: selected crash report (or newest) + latest.log + current mods.
    // Do not dump every historical crash-report into the analyzer.
    // If latest.log is newer than the crash report (successful relaunch), skip
    // the stale crash text unless the user explicitly selected that report.
    let installed: Vec<String> = manifest.mods.iter().map(|m| m.id.clone()).collect();
    let mut latest_log = String::new();
    let lp = project_dir.join("logs").join("latest.log");
    if lp.is_file() {
        latest_log = tuffbox_core::process::read_log_tail(&lp, 2000).unwrap_or_default();
    }

    let explicit = report_id.filter(|id| !id.is_empty());
    let mut crash_content = Vec::new();
    if let Some(id) = explicit {
        if let Some(text) = load_scoped_crash_report(project_dir, Some(id)) {
            crash_content.push(text);
        }
    } else if let Some((report_path, text)) = load_newest_crash_report(project_dir) {
        let stale = tuffbox_core::crash::latest_log_supersedes_crash(
            project_dir,
            Some(report_path.as_path()),
            &latest_log,
        );
        if !stale {
            crash_content.push(text);
        }
    }

    let mut launcher_log = String::new();
    let la = project_dir.join("logs").join("launcher.log");
    if la.is_file() {
        launcher_log = tuffbox_core::process::read_log_tail(&la, 400).unwrap_or_default();
    }

    let jv = manifest
        .java
        .as_ref()
        .and_then(|j| j.path.clone())
        .unwrap_or_default();
    let java_version = if !jv.is_empty() {
        tuffbox_core::jre::check_java_at_path(&PathBuf::from(&jv))
            .map(|r| r.version)
            .unwrap_or_default()
    } else {
        String::new()
    };

    let ctx = tuffbox_core::crash_assistant::AnalysisCtx {
        crash_content,
        latest_log,
        launcher_log,
        installed_mods: installed,
        installed_versions: Some(
            manifest
                .mods
                .iter()
                .map(|m| (m.id.clone(), m.version.clone()))
                .collect(),
        ),
        previous_mods: Vec::new(),
        java_version,
        java_vendor: String::new(),
        os_name: std::env::consts::OS.to_string(),
        mc_version: manifest.minecraft.version.clone(),
        loader: format!("{:?}", manifest.loader.kind).to_lowercase(),
        loader_version: manifest.loader.version.clone(),
        cpu_name: String::new(),
        gpu_names: Vec::new(),
        total_ram_mb: 0,
        is_offline: false,
        win_events: Vec::new(),
        combined_lines: std::cell::OnceCell::new(),
    };
    // get_crash_diagnosis and run_crash_assistant_full consume the same
    // expensive rule-based report during one Diagnose refresh. Share it by
    // the same input fingerprint so the second caller does not reopen every
    // log and re-run all crash patterns.
    let report = tuffbox_core::crash_assistant::run_full_analysis(&ctx);
    tuffbox_core::api_cache::put_with_ttl(
        cache_key,
        report.clone(),
        std::time::Duration::from_secs(30),
    );
    Ok(report)
}

#[tauri::command(rename_all = "camelCase")]
async fn create_crash_fix_plan(
    path: String,
    report_id: Option<String>,
) -> Result<tuffbox_core::ChangePlan, String> {
    // Blocking-pool conversion: sync commands run on the main thread (see
    // get_health_report); heavy I/O must not freeze IPC delivery.
    tokio::task::spawn_blocking(move || create_crash_fix_plan_impl(path, report_id))
        .await
        .map_err(|e| e.to_string())?
}

fn create_crash_fix_plan_impl(
    path: String,
    report_id: Option<String>,
) -> Result<tuffbox_core::ChangePlan, String> {
    Ok(get_crash_diagnosis_impl(None, path, report_id)?.fix_plan)
}

/// Actually applies the crash-diagnosis fix plan (update/disable suspected
/// mod, install missing dependency, etc.), the same way the Graph tab's
/// "Apply full plan" does for resolver plans.
///
/// Previously the Diagnostics UI had a "Fix Issue" button that only set a
/// success message in the frontend without calling into the backend at
/// all — no snapshot, no manifest change, nothing. This command gives that
/// button (renamed "Apply fix plan") a real effect: it recomputes the plan
/// server-side (so the UI can't apply a stale/tampered plan), snapshots
/// first when the plan calls for it, and returns what was actually done so
/// the UI can report a truthful result instead of an assumed one.
#[tauri::command(rename_all = "camelCase")]
async fn apply_crash_fix_plan(
    path: String,
    report_id: Option<String>,
    option_index: Option<usize>,
) -> Result<Vec<String>, String> {
    let result = tokio::task::spawn_blocking(move || {
        let manifest_path = resolve_manifest_path(&path)?;
        let path_str = manifest_path.to_string_lossy().to_string();
        let project_dir = manifest_parent(&path_str)?;
        let diagnosis = get_crash_diagnosis_impl(None, path_str.clone(), report_id.clone())?;
        let plan = diagnosis.fix_plan;

        // When the user picked a radio option, apply exactly that option's
        // actions instead of the whole default plan.
        let actions_to_apply = match option_index {
            Some(idx) => plan
                .options
                .get(idx)
                .filter(|o| !o.actions.is_empty())
                .map(|o| o.actions.clone())
                .unwrap_or_else(|| plan.actions.clone()),
            None => plan.actions.clone(),
        };

        if actions_to_apply.is_empty() {
            return Ok((path_str, Vec::new()));
        }

        let manifest = ProjectManifest::load_from_path(&manifest_path).map_err(|e| e.to_string())?;
        let loader = format!("{:?}", manifest.loader.kind).to_lowercase();
        let crash = load_scoped_crash_report(&project_dir, report_id.as_deref()).unwrap_or_default();
        let fingerprint = tuffbox_core::crash_kb::fingerprint_from_text(
            &crash,
            &manifest.minecraft.version,
            &loader,
        );

        let launcher_actions = swarm_api::change_actions_to_launcher(&actions_to_apply);

        if plan.requires_snapshot {
            swarm_api::auto_snapshot_crash_fix_heuristic(
                &manifest_path,
                Some(fingerprint.key.as_str()),
                &plan.summary,
                report_id.as_deref(),
                launcher_actions.clone(),
            )?;
        }

        let mut manifest = ProjectManifest::load_from_path(&manifest_path).map_err(|e| e.to_string())?;
        let mut applied = Vec::new();
        for action in actions_to_apply {
            apply_change_action(&manifest_path, &mut manifest, action, &mut applied)?;
        }
        save_manifest(&manifest_path, &manifest).map_err(|e| e.to_string())?;
        download_project_mods(&manifest_path, &manifest);

        let explanation = if applied.is_empty() {
            plan.summary.clone()
        } else {
            format!("Applied fix plan: {}", applied.join("; "))
        };
        let _ = swarm_api::record_user_fix_attempt(
            &manifest_path,
            "crash_assistant",
            &explanation,
            launcher_actions,
            Some(fingerprint.key.as_str()),
        );

        let _ = swarm_api::record_project_cooccurrence(path_str.clone());
        Ok::<_, String>((path_str, applied))
    })
    .await
    .map_err(|e| e.to_string())??;
    let (path, applied) = result;
    // Best-effort Supabase upload (local already recorded above).
    let _ = swarm_api::record_and_upload_cooccurrence_opts(
        &path,
        &[],
        "crash_assistant_fix",
        false,
    )
    .await;
    Ok(applied)
}

#[tauri::command(rename_all = "camelCase")]
fn get_history_settings(path: String) -> Result<HistorySettings, String> {
    let project_dir = manifest_parent(&path)?;
    let settings_path = project_dir.join(".tuffbox").join("history.json");
    if settings_path.is_file() {
        let raw = std::fs::read_to_string(settings_path).map_err(|e| e.to_string())?;
        serde_json::from_str(&raw).map_err(|e| e.to_string())
    } else {
        Ok(default_history_settings())
    }
}

#[tauri::command(rename_all = "camelCase")]
fn update_history_settings(
    path: String,
    settings: HistorySettings,
) -> Result<HistorySettings, String> {
    let project_dir = manifest_parent(&path)?;
    let settings_dir = project_dir.join(".tuffbox");
    std::fs::create_dir_all(&settings_dir).map_err(|e| e.to_string())?;
    let settings_path = settings_dir.join("history.json");
    let json = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    std::fs::write(settings_path, json).map_err(|e| e.to_string())?;
    Ok(settings)
}

#[tauri::command(rename_all = "camelCase")]
fn history_episode_fields(
    fingerprint: &Option<String>,
    plan_source: &Option<String>,
    episode_id: Option<String>,
) -> (Option<String>, Option<String>) {
    let episode = episode_id.or_else(|| {
        fingerprint
            .as_ref()
            .filter(|k| !k.trim().is_empty() && *k != "unknown")
            .map(|k| pack_events::episode_id_for_fingerprint(k))
    });
    let method = pack_events::normalize_fix_method(plan_source.as_deref());
    let fix_method = if method == "unknown" {
        None
    } else {
        Some(method.to_string())
    };
    (episode, fix_method)
}

fn is_crash_history_entry(entry: &ProjectChangeEntry) -> bool {
    entry.episode_id.is_some()
        || entry.crash_fingerprint_key.is_some()
        || entry.op.contains("crash")
        || entry.kind.contains("crash")
        || entry
            .tags
            .iter()
            .any(|t| t.contains("crash") || t == "crash_fix")
}

#[allow(dead_code)]
fn entry_group_key(entry: &ProjectChangeEntry) -> Option<String> {
    if let Some(ep) = entry
        .episode_id
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        return Some(ep.to_string());
    }
    if let Some(fp) = entry
        .crash_fingerprint_key
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty() && *s != "unknown")
    {
        return Some(pack_events::episode_id_for_fingerprint(fp));
    }
    None
}

fn is_crash_detected_entry(e: &ProjectChangeEntry) -> bool {
    e.op == "crash_detected" || e.kind == "crash_detected"
}

fn outcome_is_closed(outcome: &str) -> bool {
    matches!(outcome, "fixed" | "broke" | "rolled_back")
}

fn episode_id_for_segment(fp: Option<&str>, started_at: &str, first_id: &str) -> String {
    let ts_suffix = if started_at.len() >= 16 {
        started_at[0..16].replace(':', "").replace('T', "-")
    } else {
        first_id.chars().filter(|c| c.is_ascii_alphanumeric()).take(12).collect()
    };
    match fp {
        Some(fp) if !fp.trim().is_empty() && fp != "unknown" => {
            format!("{}-{}", pack_events::episode_id_for_fingerprint(fp), ts_suffix)
        }
        _ => format!("ep-orphan-{ts_suffix}"),
    }
}

fn finalize_episode_segment(
    entries: &[ProjectChangeEntry],
    idxs: &[usize],
) -> Option<HistoryEpisode> {
    if idxs.is_empty() {
        return None;
    }
    let mut idxs = idxs.to_vec();
    idxs.sort_by(|&a, &b| {
        entries[a]
            .created_at
            .cmp(&entries[b].created_at)
            .then_with(|| a.cmp(&b))
    });
    let refs: Vec<&ProjectChangeEntry> = idxs.iter().map(|&i| &entries[i]).collect();
    let outcome = episode_outcome_for(&refs);
    let fix_method = episode_fix_method_for(&refs);
    let fingerprint_key = refs
        .iter()
        .find_map(|e| e.crash_fingerprint_key.clone())
        .filter(|s| !s.is_empty());
    let plan_source = refs.iter().rev().find_map(|e| e.plan_source.clone());
    let snapshot_id = refs.iter().rev().find_map(|e| {
        if e.snapshot_id.is_empty() {
            None
        } else {
            Some(e.snapshot_id.clone())
        }
    });
    let log_path = refs.iter().find_map(|e| e.log_path.clone());
    let resolution_summary = refs.iter().rev().find_map(|e| {
        if e.op == "crash_resolved"
            || e.kind == "crash_resolved"
            || e.tags.iter().any(|t| t == "crash_resolved")
        {
            let text = if !e.preview.trim().is_empty() {
                e.preview.as_str()
            } else {
                e.operation.as_str()
            };
            let trimmed = tuffbox_core::crash_kb::truncate_at_char_boundary(text, 280);
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        } else {
            None
        }
    });
    let started_at = refs.first().map(|e| e.created_at.clone()).unwrap_or_default();
    let ended_at = if outcome_is_closed(&outcome) {
        refs.last().map(|e| e.created_at.clone())
    } else {
        None
    };
    let summary = episode_summary_for(&outcome, &fix_method, &refs);
    let id = episode_id_for_segment(
        fingerprint_key.as_deref(),
        &started_at,
        &refs.first().map(|e| e.id.as_str()).unwrap_or("x"),
    );
    let action_ids = idxs.iter().map(|&i| entries[i].id.clone()).collect();
    Some(HistoryEpisode {
        id,
        outcome,
        fix_method,
        fingerprint_key,
        started_at,
        ended_at,
        summary,
        action_ids,
        plan_source,
        snapshot_id,
        resolution_summary,
        log_path,
    })
}

/// Group crash-related history entries into time-bounded episodes.
fn build_history_episodes(entries: &[ProjectChangeEntry]) -> Vec<HistoryEpisode> {
    use std::collections::BTreeMap;

    let mut by_fp: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (idx, entry) in entries.iter().enumerate() {
        if !is_crash_history_entry(entry) {
            continue;
        }
        let key = entry
            .crash_fingerprint_key
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && s != "unknown")
            .unwrap_or_else(|| "_orphan_".into());
        by_fp.entry(key).or_default().push(idx);
    }

    let orphan_idxs = by_fp.remove("_orphan_").unwrap_or_default();
    let mut segments: Vec<Vec<usize>> = Vec::new();

    for (_fp, mut bucket) in by_fp {
        bucket.sort_by(|&a, &b| {
            entries[a]
                .created_at
                .cmp(&entries[b].created_at)
                .then_with(|| a.cmp(&b))
        });
        let mut current: Vec<usize> = Vec::new();
        for i in bucket {
            let e = &entries[i];
            if is_crash_detected_entry(e) && !current.is_empty() {
                let current_refs: Vec<&ProjectChangeEntry> =
                    current.iter().map(|&j| &entries[j]).collect();
                let cur_outcome = episode_outcome_for(&current_refs);
                if outcome_is_closed(&cur_outcome) {
                    segments.push(std::mem::take(&mut current));
                    current.push(i);
                    continue;
                }
                let mut provisional = current_refs;
                provisional.push(e);
                if episode_outcome_for(&provisional) == "broke" {
                    current.push(i);
                    segments.push(std::mem::take(&mut current));
                    continue;
                }
            }
            current.push(i);
        }
        if !current.is_empty() {
            segments.push(current);
        }
    }

    // Attach orphans only to open episodes within ±2h; else provisional segment.
    for idx in orphan_idxs {
        let ts = &entries[idx].created_at;
        let mut attached = false;
        for seg in &mut segments {
            let refs: Vec<&ProjectChangeEntry> = seg.iter().map(|&j| &entries[j]).collect();
            if episode_outcome_for(&refs) != "open" {
                continue;
            }
            if seg
                .iter()
                .any(|&j| timestamps_within_hours(ts, &entries[j].created_at, 2))
            {
                seg.push(idx);
                attached = true;
                break;
            }
        }
        if !attached {
            segments.push(vec![idx]);
        }
    }

    let mut episodes: Vec<HistoryEpisode> = segments
        .iter()
        .filter_map(|idxs| finalize_episode_segment(entries, idxs))
        .collect();
    episodes.sort_by(|a, b| b.started_at.cmp(&a.started_at).then_with(|| a.id.cmp(&b.id)));
    episodes
}

fn episode_outcome_for(entries: &[&ProjectChangeEntry]) -> String {
    let has_resolved = entries.iter().any(|e| {
        e.op == "crash_resolved"
            || e.kind == "crash_resolved"
            || e.tags.iter().any(|t| t == "crash_resolved")
    });
    if has_resolved {
        return "fixed".into();
    }
    let has_rollback = entries.iter().any(|e| {
        e.op.contains("rollback")
            || e.kind.contains("rollback")
            || e.tags.iter().any(|t| t.contains("rollback"))
    });
    if has_rollback {
        return "rolled_back".into();
    }
    let has_reject = entries.iter().any(|e| {
        e.op.contains("reject")
            || e.kind.contains("reject")
            || e.tags.iter().any(|t| t.contains("reject"))
    });
    let has_fix = entries.iter().any(|e| {
        e.op == "crash_fix"
            || e.kind == "crash_fix"
            || e.tags.iter().any(|t| t == "crash_fix")
            || matches!(
                e.op.as_str(),
                "mod_removed" | "mod_added" | "mod_updated" | "file_changed" | "file_edit"
            )
    });
    let has_detected = entries
        .iter()
        .any(|e| e.op == "crash_detected" || e.kind == "crash_detected");

    // Chronological: if a crash lands after fix/actions without resolve → broke.
    let mut chrono: Vec<_> = entries.to_vec();
    chrono.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    let mut saw_actions = false;
    for e in &chrono {
        let is_detected = e.op == "crash_detected" || e.kind == "crash_detected";
        let is_action = e.op == "crash_fix"
            || e.kind == "crash_fix"
            || e.tags.iter().any(|t| t == "crash_fix")
            || matches!(
                e.op.as_str(),
                "mod_removed" | "mod_added" | "mod_updated" | "file_changed" | "file_edit"
            );
        if is_action {
            saw_actions = true;
        } else if is_detected && saw_actions {
            return "broke".into();
        }
    }
    if has_reject && has_fix {
        return "broke".into();
    }
    if has_detected || has_fix {
        return "open".into();
    }
    "open".into()
}

fn episode_fix_method_for(entries: &[&ProjectChangeEntry]) -> String {
    for e in entries.iter().rev() {
        if let Some(m) = e.fix_method.as_ref().filter(|s| !s.is_empty() && *s != "unknown") {
            return m.clone();
        }
        let from_plan = pack_events::normalize_fix_method(e.plan_source.as_deref());
        if from_plan != "unknown" {
            return from_plan.to_string();
        }
    }
    // Manual: user actor edits without plan source.
    if entries.iter().any(|e| e.actor == "user") {
        return "manual".into();
    }
    "unknown".into()
}

fn short_history_action_label(entry: &ProjectChangeEntry) -> String {
    let paths = if entry.path.is_empty() {
        Vec::new()
    } else {
        vec![entry.path.clone()]
    };
    let prefer = if !entry.operation.trim().is_empty() {
        entry.operation.as_str()
    } else {
        entry.preview.as_str()
    };
    // Prefer Install/Remove/Update/Disable/Enable operation lines as-is.
    let trimmed = prefer.trim();
    if trimmed.starts_with("Install ")
        || trimmed.starts_with("Remove ")
        || trimmed.starts_with("Update ")
        || trimmed.starts_with("Disable ")
        || trimmed.starts_with("Enable ")
    {
        return tuffbox_core::crash_kb::truncate_at_char_boundary(trimmed, 60).to_string();
    }
    pack_events::concise_event_summary(trimmed, &paths, &entry.op)
}

fn episode_summary_for(outcome: &str, method: &str, entries: &[&ProjectChangeEntry]) -> String {
    let method_label = match method {
        "ai" => "AI",
        "heuristic" => "Heuristic",
        "kb" => "KB",
        "swarm" => "Swarm",
        "manual" => "Manual",
        _ => "Unknown",
    };
    let top = entries
        .iter()
        .find(|e| {
            e.op == "crash_fix"
                || e.kind == "crash_fix"
                || e.op == "crash_resolved"
                || e.kind == "crash_resolved"
                || e.op == "crash_detected"
        })
        .or_else(|| {
            // Prefer human Install/Remove/… lines over opaque file dumps.
            entries.iter().find(|e| {
                let s = e.operation.trim();
                s.starts_with("Install ")
                    || s.starts_with("Remove ")
                    || s.starts_with("Update ")
                    || s.starts_with("Disable ")
                    || s.starts_with("Enable ")
            })
        })
        .or_else(|| entries.first())
        .map(|e| short_history_action_label(e))
        .unwrap_or_else(|| "Crash episode".into());
    match outcome {
        "fixed" => format!("{method_label} plan fixed · {top}"),
        "broke" => format!("{method_label} changes → next launch crashed · {top}"),
        "rolled_back" => format!("{method_label} fix rolled back · {top}"),
        "activity" => top,
        _ => format!("{method_label} · open · {top}"),
    }
}

/// Day-bucketed non-crash pack edits so Episodes mode is not empty when Flat has data.
fn build_activity_episodes(entries: &[ProjectChangeEntry]) -> Vec<HistoryEpisode> {
    use std::collections::BTreeMap;

    let mut by_day: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (idx, entry) in entries.iter().enumerate() {
        if is_crash_history_entry(entry) {
            continue;
        }
        let day = if entry.created_at.len() >= 10 {
            entry.created_at[..10].to_string()
        } else {
            "unknown".into()
        };
        by_day.entry(day).or_default().push(idx);
    }

    let mut out = Vec::new();
    for (day, mut idxs) in by_day {
        idxs.sort_by(|&a, &b| {
            entries[a]
                .created_at
                .cmp(&entries[b].created_at)
                .then_with(|| a.cmp(&b))
        });
        for chunk in idxs.chunks(40) {
            let refs: Vec<&ProjectChangeEntry> = chunk.iter().map(|&i| &entries[i]).collect();
            let started_at = refs
                .first()
                .map(|e| e.created_at.clone())
                .unwrap_or_default();
            let ended_at = refs.last().map(|e| e.created_at.clone());
            let n = chunk.len();
            let highlight = refs
                .iter()
                .find(|e| {
                    let s = e.operation.trim();
                    s.starts_with("Install ")
                        || s.starts_with("Remove ")
                        || s.starts_with("Update ")
                        || s.starts_with("Disable ")
                        || s.starts_with("Enable ")
                })
                .or_else(|| refs.first())
                .map(|e| short_history_action_label(e))
                .unwrap_or_default();
            let summary = if n == 1 {
                highlight
            } else {
                let head = format!("{n} pack changes · ");
                let budget = 60usize.saturating_sub(head.len());
                let top = tuffbox_core::crash_kb::truncate_at_char_boundary(&highlight, budget);
                format!("{head}{top}")
            };
            let first_id = refs.first().map(|e| e.id.as_str()).unwrap_or("x");
            out.push(HistoryEpisode {
                id: format!("ep-activity-{day}-{first_id}"),
                outcome: "activity".into(),
                fix_method: "unknown".into(),
                fingerprint_key: None,
                started_at,
                ended_at,
                summary,
                action_ids: chunk.iter().map(|&i| entries[i].id.clone()).collect(),
                plan_source: None,
                snapshot_id: refs.iter().rev().find_map(|e| {
                    if e.snapshot_id.is_empty() {
                        None
                    } else {
                        Some(e.snapshot_id.clone())
                    }
                }),
                resolution_summary: None,
                log_path: None,
            });
        }
    }
    out
}

fn timestamps_within_hours(a: &str, b: &str, hours: i64) -> bool {
    fn parse_approx(s: &str) -> Option<i64> {
        // Accept RFC3339-ish: take YYYY-MM-DDTHH:MM
        if s.len() < 16 {
            return None;
        }
        let date = &s[0..10];
        let hour: i64 = s[11..13].parse().ok()?;
        let min: i64 = s[14..16].parse().ok()?;
        let y: i64 = date[0..4].parse().ok()?;
        let m: i64 = date[5..7].parse().ok()?;
        let d: i64 = date[8..10].parse().ok()?;
        Some((((y * 12 + m) * 31 + d) * 24 + hour) * 60 + min)
    }
    match (parse_approx(a), parse_approx(b)) {
        (Some(x), Some(y)) => (x - y).abs() <= hours * 60,
        _ => a.get(0..10) == b.get(0..10),
    }
}

#[cfg(test)]
mod history_episode_tests {
    use super::*;

    fn entry(
        id: &str,
        op: &str,
        created_at: &str,
        fingerprint: Option<&str>,
        plan_source: Option<&str>,
        actor: &str,
    ) -> ProjectChangeEntry {
        let fingerprint = fingerprint.map(|s| s.to_string());
        let plan_source = plan_source.map(|s| s.to_string());
        let (episode_id, fix_method) = history_episode_fields(&fingerprint, &plan_source, None);
        ProjectChangeEntry {
            id: id.into(),
            snapshot_id: String::new(),
            operation: op.into(),
            reason: String::new(),
            created_at: created_at.into(),
            path: String::new(),
            category: "Resolutions".into(),
            kind: op.into(),
            preview: op.into(),
            diff: String::new(),
            can_open: false,
            tags: vec!["crash".into()],
            crash_fingerprint_key: fingerprint,
            plan_source,
            actor: actor.into(),
            op: op.into(),
            episode_id,
            fix_method,
            log_path: None,
        }
    }

    #[test]
    fn history_list_omits_full_diffs() {
        let mut entries = vec![entry(
            "a",
            "file_changed",
            "2026-01-01T00:00:00Z",
            None,
            None,
            "user",
        )];
        entries[0].diff = "--- a\n+++ b\n".repeat(40);
        entries[0].preview = "Edited config".into();
        omit_history_list_diffs(&mut entries);
        assert!(entries[0].diff.is_empty());
        assert_eq!(entries[0].preview, "Edited config");
    }

    #[test]
    fn episode_ai_fixed_path() {
        let entries = vec![
            entry(
                "1",
                "crash_detected",
                "2026-08-01T10:00:00Z",
                Some("fp-mixin"),
                None,
                "launcher",
            ),
            entry(
                "2",
                "crash_fix",
                "2026-08-01T10:05:00Z",
                Some("fp-mixin"),
                Some("ai"),
                "ai",
            ),
            entry(
                "3",
                "crash_resolved",
                "2026-08-01T10:10:00Z",
                Some("fp-mixin"),
                Some("ai"),
                "ai",
            ),
        ];
        let eps = build_history_episodes(&entries);
        assert_eq!(eps.len(), 1);
        assert_eq!(eps[0].outcome, "fixed");
        assert_eq!(eps[0].fix_method, "ai");
        assert_eq!(eps[0].action_ids.len(), 3);
    }

    #[test]
    fn episode_heuristic_open_then_broke() {
        let open_entries = vec![
            entry(
                "1",
                "crash_detected",
                "2026-08-01T10:00:00Z",
                Some("fp-h"),
                None,
                "launcher",
            ),
            entry(
                "2",
                "crash_fix",
                "2026-08-01T10:05:00Z",
                Some("fp-h"),
                Some("heuristic"),
                "launcher",
            ),
        ];
        let open_eps = build_history_episodes(&open_entries);
        assert_eq!(open_eps[0].outcome, "open");
        assert_eq!(open_eps[0].fix_method, "heuristic");

        let broke_entries = vec![
            entry(
                "1",
                "crash_detected",
                "2026-08-01T10:00:00Z",
                Some("fp-h"),
                None,
                "launcher",
            ),
            entry(
                "2",
                "crash_fix",
                "2026-08-01T10:05:00Z",
                Some("fp-h"),
                Some("heuristic"),
                "launcher",
            ),
            entry(
                "3",
                "crash_detected",
                "2026-08-01T10:20:00Z",
                Some("fp-h"),
                None,
                "launcher",
            ),
        ];
        let broke_eps = build_history_episodes(&broke_entries);
        assert_eq!(broke_eps[0].outcome, "broke");
    }

    #[test]
    fn episode_manual_actor_and_rollback() {
        let entries = vec![
            entry(
                "1",
                "crash_detected",
                "2026-08-01T11:00:00Z",
                Some("fp-m"),
                None,
                "launcher",
            ),
            entry(
                "2",
                "mod_removed",
                "2026-08-01T11:05:00Z",
                Some("fp-m"),
                None,
                "user",
            ),
            entry(
                "3",
                "crash_fix_rollback",
                "2026-08-01T11:10:00Z",
                Some("fp-m"),
                Some("manual"),
                "user",
            ),
        ];
        let eps = build_history_episodes(&entries);
        assert_eq!(eps.len(), 1);
        assert_eq!(eps[0].outcome, "rolled_back");
        assert_eq!(eps[0].fix_method, "manual");
    }

    #[test]
    fn recurring_crash_splits_episodes() {
        let entries = vec![
            entry(
                "1",
                "crash_detected",
                "2026-08-01T10:00:00Z",
                Some("fp-recur"),
                None,
                "launcher",
            ),
            entry(
                "2",
                "crash_fix",
                "2026-08-01T10:05:00Z",
                Some("fp-recur"),
                Some("ai"),
                "ai",
            ),
            entry(
                "3",
                "crash_resolved",
                "2026-08-01T10:10:00Z",
                Some("fp-recur"),
                Some("ai"),
                "ai",
            ),
            entry(
                "4",
                "crash_detected",
                "2026-08-02T12:00:00Z",
                Some("fp-recur"),
                None,
                "launcher",
            ),
            entry(
                "5",
                "crash_fix",
                "2026-08-02T12:05:00Z",
                Some("fp-recur"),
                Some("heuristic"),
                "launcher",
            ),
        ];
        let eps = build_history_episodes(&entries);
        assert_eq!(eps.len(), 2, "expected split after closed fix");
        let fixed = eps.iter().find(|e| e.outcome == "fixed").expect("fixed");
        let open = eps.iter().find(|e| e.outcome == "open").expect("open");
        assert_eq!(fixed.fix_method, "ai");
        assert_eq!(open.fix_method, "heuristic");
        assert_ne!(fixed.id, open.id);
        assert!(fixed.id.contains("2026-08-01"), "{}", fixed.id);
        assert!(open.id.contains("2026-08-02"), "{}", open.id);
    }

    #[test]
    fn activity_episodes_group_non_crash_pack_edits() {
        let pack = |id: &str, op: &str, at: &str, actor: &str| ProjectChangeEntry {
            id: id.into(),
            snapshot_id: String::new(),
            operation: op.into(),
            reason: String::new(),
            created_at: at.into(),
            path: format!("{op}.txt"),
            category: "Configs".into(),
            kind: op.into(),
            preview: op.into(),
            diff: String::new(),
            can_open: false,
            tags: vec![],
            crash_fingerprint_key: None,
            plan_source: None,
            actor: actor.into(),
            op: op.into(),
            episode_id: None,
            fix_method: None,
            log_path: None,
        };
        let entries = vec![
            pack("a1", "config_changed", "2026-08-03T09:00:00Z", "scan"),
            pack("a2", "mod_added", "2026-08-03T09:05:00Z", "user"),
            entry(
                "c1",
                "crash_detected",
                "2026-08-03T10:00:00Z",
                Some("fp-x"),
                None,
                "launcher",
            ),
        ];
        let activity = build_activity_episodes(&entries);
        assert_eq!(activity.len(), 1);
        assert_eq!(activity[0].outcome, "activity");
        assert_eq!(activity[0].action_ids, vec!["a1".to_string(), "a2".to_string()]);
    }
}

#[tauri::command(rename_all = "camelCase")]
fn list_project_change_history(path: String) -> Result<HistoryListResult, String> {
    let manifest_path = PathBuf::from(&path);
    let project_dir = manifest_parent(&path)?;
    let store = SnapshotStore::new(&project_dir);
    let snapshots = store.list().map_err(|e| e.to_string())?;
    let mut entries = Vec::new();
    let manifest_mods = ProjectManifest::load_from_path(&path)
        .map(|m| m.mods)
        .unwrap_or_default();

    // Pack activity journal (launcher + external scan + AI).
    for ev in pack_events::list_pack_events(&project_dir, Some(500)) {
        let path_text = ev
            .paths
            .first()
            .cloned()
            .filter(|p| !p.is_empty() && *p != ev.op)
            .unwrap_or_else(|| {
                if ev.op == "mod_change" {
                    String::new()
                } else {
                    ev.op.clone()
                }
            });
        let can_open = {
            let p = project_dir.join(&path_text);
            !path_text.is_empty() && p.is_file() && is_editable_config_path(&p)
        };
        let meta_preview = ev
            .meta
            .as_ref()
            .and_then(|m| m.get("preview"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let summary = {
            let s = humanize_history_summary(&ev.summary, &manifest_mods);
            pack_events::concise_event_summary(&s, &ev.paths, &ev.op)
        };
        let preview = meta_preview
            .map(|p| {
                let p = humanize_history_summary(&p, &manifest_mods);
                pack_events::concise_event_summary(&p, &ev.paths, &ev.op)
            })
            .unwrap_or_else(|| summary.clone());
        let crash_fingerprint_key = pack_events::meta_str(&ev.meta, "fingerprintKey")
            .or_else(|| pack_events::meta_str(&ev.meta, "fingerprint"));
        let plan_source = pack_events::meta_str(&ev.meta, "planSource");
        let episode_from_meta = pack_events::meta_str(&ev.meta, "episodeId");
        let (episode_id, fix_method) =
            history_episode_fields(&crash_fingerprint_key, &plan_source, episode_from_meta);
        let fix_method = fix_method.or_else(|| {
            pack_events::meta_str(&ev.meta, "fixMethod").map(|m| {
                pack_events::normalize_fix_method(Some(&m)).to_string()
            })
        });
        let log_path = pack_events::meta_str(&ev.meta, "logPath");
        let actor_lc = ev.actor.to_lowercase();
        let actor_label = match actor_lc.as_str() {
            "scan" => "Disk",
            "ai" => "AI",
            "user" => "You",
            "launcher" => "Launcher",
            other if other.is_empty() => "Launcher",
            other => other,
        };
        entries.push(ProjectChangeEntry {
            id: ev.id.clone(),
            snapshot_id: ev.snapshot_id.clone().unwrap_or_default(),
            operation: summary,
            reason: format!("{} · {}", actor_label, humanize_history_op(&ev.op)),
            created_at: ev.ts.clone(),
            path: if path_text.is_empty() {
                ev.paths.first().cloned().unwrap_or_default()
            } else {
                path_text
            },
            category: ev.category.clone(),
            kind: ev.op.clone(),
            preview,
            diff: String::new(),
            can_open,
            tags: ev.tags.clone(),
            crash_fingerprint_key,
            plan_source,
            actor: ev.actor.clone(),
            op: ev.op.clone(),
            episode_id,
            fix_method,
            log_path,
        });
    }

    // Resolved crash fixes (successful relaunch after apply).
    let mut seen_resolution_keys = std::collections::HashSet::new();
    if let Ok(resolutions) = swarm_api::list_crash_resolutions(&project_dir) {
        for rec in resolutions {
            seen_resolution_keys.insert(rec.fingerprint_key.clone());
            let how = if rec.actions_summary.is_empty() {
                rec.human_explanation.clone()
            } else {
                rec.actions_summary.join("; ")
            };
            let summary = if rec.actions_summary.is_empty() {
                rec.human_explanation.clone()
            } else {
                format!(
                    "{}\nHow: {}",
                    rec.human_explanation,
                    rec.actions_summary.join(", ")
                )
            };
            let fingerprint = Some(rec.fingerprint_key.clone());
            let plan_source = rec.plan_source.clone();
            let (episode_id, fix_method) =
                history_episode_fields(&fingerprint, &plan_source, None);
            let actor = pack_events::actor_for_plan_source(plan_source.as_deref()).to_string();
            entries.push(ProjectChangeEntry {
                id: rec.id.clone(),
                snapshot_id: rec.snapshot_id.clone(),
                operation: "Crash resolved".to_string(),
                reason: format!("Verified by {} · {}", rec.verified_by, how),
                created_at: rec.resolved_at.clone(),
                path: format!("crash://{}", rec.fingerprint_key),
                category: "Resolutions".to_string(),
                kind: "crash_resolved".to_string(),
                preview: tuffbox_core::crash_kb::truncate_at_char_boundary(&summary, 240)
                    .to_string(),
                diff: summary,
                can_open: false,
                tags: vec!["crash_resolved".into(), "crash_fix".into()],
                crash_fingerprint_key: fingerprint,
                plan_source,
                actor,
                op: "crash_resolved".into(),
                episode_id,
                fix_method,
                log_path: None,
            });
        }
    }

    for (index, snapshot) in snapshots.iter().enumerate() {
        let after_manifest_path = snapshots
            .get(index + 1)
            .map(|next| next.manifest_path.as_path())
            .unwrap_or(manifest_path.as_path());
        if let (Ok(before), Ok(after)) = (
            ProjectManifest::load_from_path(&snapshot.manifest_path),
            ProjectManifest::load_from_path(after_manifest_path),
        ) {
            entries.extend(mod_change_entries(snapshot, &before, &after));
        }

        // Explicit card for crash_resolved snapshots even without file diffs
        // (skip if already covered by resolutions.jsonl).
        if snapshot.tags.iter().any(|t| t == "crash_resolved")
            && snapshot.changed_files.is_empty()
            && snapshot
                .crash_fingerprint_key
                .as_ref()
                .map(|k| !seen_resolution_keys.contains(k))
                .unwrap_or(true)
        {
            let (episode_id, fix_method) = history_episode_fields(
                &snapshot.crash_fingerprint_key,
                &snapshot.plan_source,
                None,
            );
            let actor =
                pack_events::actor_for_plan_source(snapshot.plan_source.as_deref()).to_string();
            entries.push(ProjectChangeEntry {
                id: format!("{}:crash-resolved", snapshot.id),
                snapshot_id: snapshot.id.clone(),
                operation: snapshot.name.clone(),
                reason: snapshot.reason.clone(),
                created_at: snapshot.created_at.clone(),
                path: "crash-resolution".to_string(),
                category: "Resolutions".to_string(),
                kind: "crash_resolved".to_string(),
                preview: snapshot.reason.clone(),
                diff: snapshot.reason.clone(),
                can_open: false,
                tags: snapshot.tags.clone(),
                crash_fingerprint_key: snapshot.crash_fingerprint_key.clone(),
                plan_source: snapshot.plan_source.clone(),
                actor,
                op: "crash_resolved".into(),
                episode_id,
                fix_method,
                log_path: None,
            });
        }

        for relative in &snapshot.changed_files {
            let relative_text = relative.to_string_lossy().replace('\\', "/");
            // Prefer journal entries when present for the same snapshot+path.
            let dup = entries.iter().any(|e| {
                e.snapshot_id == snapshot.id && e.path == relative_text && e.kind == "file_edit"
            });
            if dup {
                continue;
            }
            let after_path = project_dir.join(relative);
            let actor = if snapshot.plan_source.is_some() {
                pack_events::actor_for_plan_source(snapshot.plan_source.as_deref()).to_string()
            } else {
                pack_events::actor_for_operation(&snapshot.name).into()
            };
            let (episode_id, fix_method) = history_episode_fields(
                &snapshot.crash_fingerprint_key,
                &snapshot.plan_source,
                None,
            );
            entries.push(ProjectChangeEntry {
                id: format!("{}:{}", snapshot.id, relative_text),
                snapshot_id: snapshot.id.clone(),
                operation: snapshot.name.clone(),
                reason: snapshot.reason.clone(),
                created_at: snapshot.created_at.clone(),
                path: relative_text.clone(),
                category: change_category(&relative_text).to_string(),
                kind: "file_changed".to_string(),
                preview: relative_text.clone(),
                diff: String::new(),
                can_open: after_path.is_file() && is_editable_config_path(&after_path),
                tags: snapshot.tags.clone(),
                crash_fingerprint_key: snapshot.crash_fingerprint_key.clone(),
                plan_source: snapshot.plan_source.clone(),
                actor,
                op: "file_changed".into(),
                episode_id,
                fix_method,
                log_path: None,
            });
        }
    }

    entries.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| a.path.cmp(&b.path))
    });
    let mut episodes = build_history_episodes(&entries);
    episodes.extend(build_activity_episodes(&entries));
    episodes.sort_by(|a, b| b.started_at.cmp(&a.started_at).then_with(|| a.id.cmp(&b.id)));
    // Backfill episode_id on entries from grouped episodes when missing.
    let mut entries = entries;
    for ep in &episodes {
        for action_id in &ep.action_ids {
            if let Some(entry) = entries.iter_mut().find(|e| e.id == *action_id) {
                if entry.episode_id.is_none() {
                    entry.episode_id = Some(ep.id.clone());
                }
                if entry.fix_method.is_none() && ep.fix_method != "unknown" {
                    entry.fix_method = Some(ep.fix_method.clone());
                }
            }
        }
    }
    omit_history_list_diffs(&mut entries);
    Ok(HistoryListResult { entries, episodes })
}

fn omit_history_list_diffs(entries: &mut [ProjectChangeEntry]) {
    for entry in entries {
        entry.diff.clear();
    }
}

fn snapshot_file_diff_text(project_dir: &Path, snapshot_id: &str, relative: &str) -> String {
    let before_path = project_dir
        .join(".tuffbox")
        .join("snapshots")
        .join(snapshot_id)
        .join("changed_files")
        .join(relative);
    let after_path = project_dir.join(relative);
    let before_text = read_small_text_file(&before_path).unwrap_or_default();
    let after_text = read_small_text_file(&after_path).unwrap_or_default();
    unified_text_diff(&before_text, &after_text)
}

#[tauri::command(rename_all = "camelCase")]
fn get_history_entry_diff(path: String, entry_id: String) -> Result<String, String> {
    let project_dir = manifest_parent(&path)?;
    if let Some(ev) = pack_events::event_by_id(&project_dir, &entry_id) {
        return Ok(pack_events::event_diff_text(&ev));
    }
    if let Some((snapshot_id, relative)) = entry_id.split_once(':') {
        if !relative.starts_with("mod-added:")
            && !relative.starts_with("mod-removed:")
            && !relative.starts_with("mod-updated:")
            && relative != "crash-resolved"
        {
            let diff = snapshot_file_diff_text(&project_dir, snapshot_id, relative);
            if !diff.trim().is_empty() {
                return Ok(diff);
            }
        }
    }
    Ok(String::new())
}

#[tauri::command(rename_all = "camelCase")]
async fn scan_project_changes(path: String) -> Result<pack_events::ScanProjectChangesResult, String> {
    tokio::task::spawn_blocking(move || {
        let project_dir = manifest_parent(&path)?;
        let settings = get_history_settings(path)?;
        pack_events::scan_project_changes(&project_dir, &settings.tracked)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
async fn list_recent_pack_events(
    path: String,
    limit: Option<usize>,
) -> Result<Vec<pack_events::PackEvent>, String> {
    // Blocking-pool conversion: sync commands run on the main thread (see
    // get_health_report); heavy I/O must not freeze IPC delivery.
    tokio::task::spawn_blocking(move || list_recent_pack_events_impl(path, limit))
        .await
        .map_err(|e| e.to_string())?
}

fn list_recent_pack_events_impl(
    path: String,
    limit: Option<usize>,
) -> Result<Vec<pack_events::PackEvent>, String> {
    let project_dir = manifest_parent(&path)?;
    Ok(pack_events::list_pack_events(
        &project_dir,
        Some(limit.unwrap_or(20)),
    ))
}

#[tauri::command(rename_all = "camelCase")]
fn explain_pack_change(path: String, event_id: String) -> Result<serde_json::Value, String> {
    let project_dir = manifest_parent(&path)?;
    let events = pack_events::list_pack_events(&project_dir, Some(500));
    let Some(ev) = events.into_iter().find(|e| e.id == event_id) else {
        return Err(format!("event {event_id} not found"));
    };
    let mut excerpts = Vec::new();
    for rel in ev.paths.iter().take(3) {
        let p = project_dir.join(rel);
        if p.is_file() && is_editable_config_path(&p) {
            if let Ok(raw) = std::fs::read_to_string(&p) {
                let take: String = raw.chars().take(1200).collect();
                excerpts.push(serde_json::json!({ "path": rel, "excerpt": take }));
            }
        } else if p.is_file() {
            excerpts.push(serde_json::json!({
                "path": rel,
                "excerpt": format!("(binary/large file, {} bytes)", p.metadata().map(|m| m.len()).unwrap_or(0)),
            }));
        }
    }
    let neighbors: Vec<_> = pack_events::list_pack_events(&project_dir, Some(8))
        .into_iter()
        .filter(|e| e.id != event_id)
        .take(5)
        .map(|e| {
            serde_json::json!({
                "id": e.id,
                "ts": e.ts,
                "actor": e.actor,
                "op": e.op,
                "summary": e.summary,
            })
        })
        .collect();
    let explanation = format!(
        "Change by {} ({}) at {}: {}. Category: {}. {}",
        ev.actor,
        ev.op,
        ev.ts,
        ev.summary,
        ev.category,
        if ev.tags.iter().any(|t| t == "jar_drift") {
            "This jar is on disk but not in the project manifest — import it or remove the orphan file."
        } else if ev.actor == "scan" {
            "Detected by delta scan (edit outside the launcher)."
        } else if ev.actor == "ai" {
            "Associated with an AI/swarm fix or crash resolution. Review Diagnose for the ActionPlan."
        } else {
            "Recorded from a launcher operation (auto-snapshot)."
        }
    );
    Ok(serde_json::json!({
        "eventId": ev.id,
        "explanation": explanation,
        "excerpts": excerpts,
        "neighbors": neighbors,
        "canOpenDiagnose": ev.tags.iter().any(|t| t == "crash_fix" || t == "crash_resolved")
            || ev.op.contains("crash"),
    }))
}

#[tauri::command(rename_all = "camelCase")]
fn explain_history_episode(path: String, episode_id: String) -> Result<serde_json::Value, String> {
    let list = list_project_change_history(path)?;
    let Some(episode) = list
        .episodes
        .iter()
        .find(|e| e.id == episode_id)
        .cloned()
    else {
        return Err(format!("episode {episode_id} not found"));
    };
    let actions: Vec<&ProjectChangeEntry> = episode
        .action_ids
        .iter()
        .filter_map(|id| list.entries.iter().find(|e| e.id == *id))
        .collect();
    let mut lines = Vec::new();
    lines.push(format!(
        "Episode {} · outcome {} · method {}.",
        episode.id, episode.outcome, episode.fix_method
    ));
    if let Some(fp) = episode.fingerprint_key.as_ref() {
        lines.push(format!("Fingerprint: {fp}"));
    }
    if let Some(ps) = episode.plan_source.as_ref() {
        lines.push(format!("Plan source: {ps}"));
    }
    if let Some(rs) = episode.resolution_summary.as_ref() {
        lines.push(format!("Resolution: {rs}"));
    }
    lines.push(format!("Summary: {}", episode.summary));
    lines.push("Actions:".into());
    for (i, a) in actions.iter().take(8).enumerate() {
        let label = if a.operation.trim().is_empty() {
            a.preview.as_str()
        } else {
            a.operation.as_str()
        };
        let short = tuffbox_core::crash_kb::truncate_at_char_boundary(label, 140);
        lines.push(format!(
            "  {}. [{}] {} — {}",
            i + 1,
            a.op,
            a.actor,
            short
        ));
    }
    if actions.len() > 8 {
        lines.push(format!("  …and {} more", actions.len() - 8));
    }
    let neighbors: Vec<_> = list
        .episodes
        .iter()
        .filter(|e| e.id != episode.id)
        .take(4)
        .map(|e| {
            serde_json::json!({
                "id": e.id,
                "outcome": e.outcome,
                "summary": e.summary,
                "startedAt": e.started_at,
            })
        })
        .collect();
    let excerpts: Vec<_> = actions
        .iter()
        .filter(|a| a.can_open)
        .take(3)
        .map(|a| {
            serde_json::json!({
                "path": a.path,
                "excerpt": tuffbox_core::crash_kb::truncate_at_char_boundary(&a.diff, 400),
            })
        })
        .collect();
    Ok(serde_json::json!({
        "episodeId": episode.id,
        "explanation": lines.join("\n"),
        "outcome": episode.outcome,
        "fixMethod": episode.fix_method,
        "fingerprintKey": episode.fingerprint_key,
        "logPath": episode.log_path,
        "resolutionSummary": episode.resolution_summary,
        "excerpts": excerpts,
        "neighbors": neighbors,
        "canOpenDiagnose": true,
    }))
}

#[tauri::command(rename_all = "camelCase")]
fn read_project_history_file(
    path: String,
    relative_path: String,
) -> Result<HistoryFileContent, String> {
    let project_dir = manifest_parent(&path)?;
    let target = safe_project_file(&project_dir, &relative_path)?;
    let metadata = std::fs::metadata(&target).map_err(|e| e.to_string())?;
    if metadata.len() > 2 * 1024 * 1024 {
        return Err("file is too large for the built-in editor".to_string());
    }
    Ok(HistoryFileContent {
        path: relative_path,
        content: std::fs::read_to_string(target).map_err(|e| e.to_string())?,
    })
}

#[tauri::command(rename_all = "camelCase")]
fn create_tracked_history_snapshot(
    path: String,
    roots: Vec<String>,
) -> Result<tuffbox_core::Snapshot, String> {
    let manifest_path = PathBuf::from(&path);
    let project_dir = manifest_parent(&path)?;
    let mut changed_files = Vec::new();
    for root in roots {
        match root.as_str() {
            "options.txt" | "servers.dat" => {
                let file = project_dir.join(&root);
                if file.is_file() {
                    changed_files.push(PathBuf::from(root));
                }
            }
            _ => {
                let dir = project_dir.join(&root);
                if dir.is_dir() {
                    collect_tracked_project_files(&project_dir, &dir, &mut changed_files)
                        .map_err(|e| e.to_string())?;
                }
            }
        }
    }
    changed_files.sort();
    changed_files.dedup();
    auto_snapshot_with_changed_files(&manifest_path, "track-history", &changed_files)
        .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
fn rollback_history_file(
    path: String,
    snapshot_id: String,
    relative_path: String,
) -> Result<(), String> {
    let project_dir = manifest_parent(&path)?;
    let relative = validate_relative_snapshot_path(&relative_path)?;
    let src = project_dir
        .join(".tuffbox")
        .join("snapshots")
        .join(&snapshot_id)
        .join("changed_files")
        .join(&relative);
    if !src.is_file() {
        return Err("file is not stored in this snapshot".to_string());
    }
    let dst = project_dir.join(&relative);
    let canonical_project = std::fs::canonicalize(&project_dir).map_err(|e| e.to_string())?;
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let canonical_parent =
        std::fs::canonicalize(dst.parent().unwrap_or(&project_dir)).map_err(|e| e.to_string())?;
    if !canonical_parent.starts_with(&canonical_project) {
        return Err("file is outside project directory".to_string());
    }
    // copy_replacing: the destination may be a dedup-store hardlink — an
    // in-place copy would corrupt every pack sharing that object.
    tuffbox_core::fs_util::copy_replacing(&src, &dst).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn get_project_dir(path: String) -> Result<String, String> {
    // Accept either a `.tuffbox.json` path or a project directory.
    let resolved = resolve_manifest_path(&path)?;
    resolved
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .ok_or_else(|| "manifest has no parent directory".to_string())
}

#[tauri::command]
fn list_snapshots(project_dir: String) -> Result<Vec<tuffbox_core::Snapshot>, String> {
    let store = SnapshotStore::new(&project_dir);
    let _ = store.ensure_sizes();
    store.list().map_err(|e| e.to_string())
}

#[tauri::command]
fn create_snapshot(
    project_dir: String,
    name: String,
    reason: String,
) -> Result<tuffbox_core::Snapshot, String> {
    let store = SnapshotStore::new(&project_dir);
    let manifest_path = find_manifest_in_project_dir(&project_dir)?;
    let lockfile_path = manifest_path.with_extension("lock.json");
    let lockfile_path = if lockfile_path.exists() {
        Some(lockfile_path)
    } else {
        None
    };
    store
        .create_with_meta(
            &name,
            &reason,
            &manifest_path,
            lockfile_path.as_ref(),
            &[] as &[&Path],
            tuffbox_core::SnapshotMeta {
                operation: "manual".into(),
                actions_summary: vec![if reason.trim().is_empty() {
                    "Manual snapshot".into()
                } else {
                    reason.clone()
                }],
                actor: Some("user".into()),
                ..Default::default()
            },
        )
        .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
fn diff_snapshots(
    project_dir: String,
    from: String,
    to: String,
) -> Result<tuffbox_core::SnapshotDiff, String> {
    let store = SnapshotStore::new(&project_dir);
    store.diff(from, to).map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
fn rollback_snapshot(
    app: tauri::AppHandle,
    project_dir: String,
    id: String,
) -> Result<tuffbox_core::Snapshot, String> {
    let store = SnapshotStore::new(&project_dir);
    let snapshot = store.rollback(id.clone()).map_err(|e| e.to_string())?;
    let _ = swarm_api::note_snapshot_rollback(&app, Path::new(&project_dir), &id);
    Ok(snapshot)
}

#[tauri::command(rename_all = "camelCase")]
fn delete_snapshot(project_dir: String, id: String) -> Result<(), String> {
    let store = SnapshotStore::new(&project_dir);
    store.delete(id).map_err(|e| e.to_string())
}

/// Compare a snapshot's stored manifest/file copies against the live project
/// state so the UI can answer "what changed in this project since this
/// checkpoint?".
#[tauri::command(rename_all = "camelCase")]
fn diff_snapshot_vs_current(
    project_dir: String,
    id: String,
) -> Result<crate::types::SnapshotVsCurrent, String> {
    let store = SnapshotStore::new(&project_dir);
    store
        .get(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("snapshot {id} not found"))?;

    // File-level diff of the tracked changed-files set.
    let file_diff = store.diff_current(&id).map_err(|e| e.to_string())?;
    // Files stored in the snapshot that are now missing on disk.
    let snapshot_gone: Vec<String> = file_diff
        .removed_files
        .iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();
    let snapshot_changed: Vec<String> = file_diff
        .modified_files
        .iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();
    let current_added: Vec<String> = file_diff
        .added_files
        .iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();

    // Manifest comparison between the snapshot's manifest copy and now.
    let mut manifest_diff = String::new();
    let mut manifest_compared = false;
    if let Ok(project_manifest) = find_manifest_in_project_dir(&project_dir) {
        let snapshot_manifest = store.snapshot_dir(&id).join("manifest.json");
        if snapshot_manifest.is_file() {
            manifest_compared = true;
            let before = read_small_text_file(&snapshot_manifest).unwrap_or_default();
            let after = read_small_text_file(&project_manifest).unwrap_or_default();
            manifest_diff = unified_text_diff(&before, &after);
        }
    }

    Ok(crate::types::SnapshotVsCurrent {
        snapshot_changed_files: snapshot_changed,
        snapshot_gone_files: snapshot_gone,
        current_added_files: current_added,
        manifest_compared,
        manifest_diff,
    })
}

/// Inline text diff of one stored snapshot file vs its current on-disk state.
#[tauri::command(rename_all = "camelCase")]
fn get_snapshot_file_vs_current_diff(
    project_dir: String,
    id: String,
    relative_path: String,
) -> Result<crate::types::SnapshotFileDiff, String> {
    let relative = validate_relative_snapshot_path(&relative_path)?;
    let base = PathBuf::from(&project_dir)
        .join(".tuffbox")
        .join("snapshots");
    let from_path = base.join(&id).join("changed_files").join(&relative);
    let to_path = PathBuf::from(&project_dir).join(&relative);
    let from_exists = from_path.is_file();
    let to_exists = to_path.is_file();
    let from_text = read_small_text_file(&from_path)?;
    let to_text = read_small_text_file(&to_path)?;
    Ok(crate::types::SnapshotFileDiff {
        path: relative_path,
        from_exists,
        to_exists,
        text: unified_text_diff(&from_text, &to_text),
    })
}

/// Delete automatic snapshots older than `older_than_days`, keeping
/// user/manual and crash-fix snapshots. Returns removed ids + freed bytes.
#[tauri::command(rename_all = "camelCase")]
fn prune_auto_snapshots(
    project_dir: String,
    older_than_days: u64,
) -> Result<crate::types::PruneResult, String> {
    let store = SnapshotStore::new(&project_dir);
    let (removed_ids, total_bytes) = store
        .prune_auto_snapshots(older_than_days)
        .map_err(|e| e.to_string())?;
    Ok(crate::types::PruneResult {
        removed_ids,
        total_bytes,
    })
}

#[tauri::command(rename_all = "camelCase")]
fn get_snapshot_detail(project_dir: String, id: String) -> Result<SnapshotDetail, String> {
    let store = SnapshotStore::new(&project_dir);
    let snapshot = store
        .get(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("snapshot {id} not found"))?;

    let related_events: Vec<_> = pack_events::list_pack_events(Path::new(&project_dir), Some(500))
        .into_iter()
        .filter(|ev| ev.snapshot_id.as_deref() == Some(id.as_str()))
        .collect();

    let mut plan_actions = Vec::new();
    let mut human_explanation = None;
    let mut actions_summary = snapshot.actions_summary.clone();

    // 1) plan.json on the snapshot directory
    if let Some(plan) = swarm_api::load_snapshot_plan(Path::new(&project_dir), &id) {
        if actions_summary.is_empty() {
            actions_summary = plan
                .actions
                .iter()
                .map(swarm_api::format_launcher_action_summary)
                .collect();
            if actions_summary.is_empty() && !plan.human_explanation.trim().is_empty() {
                actions_summary.push(plan.human_explanation.clone());
            }
        }
        human_explanation = Some(plan.human_explanation.clone());
        plan_actions = plan.actions;
    }

    // 2) resolutions.jsonl by snapshotId
    if actions_summary.is_empty() {
        if let Ok(resolutions) = swarm_api::list_crash_resolutions(Path::new(&project_dir)) {
            if let Some(rec) = resolutions.iter().find(|r| r.snapshot_id == id) {
                actions_summary = rec.actions_summary.clone();
                if actions_summary.is_empty() && !rec.human_explanation.trim().is_empty() {
                    actions_summary.push(rec.human_explanation.clone());
                }
                if human_explanation.is_none() {
                    human_explanation = Some(rec.human_explanation.clone());
                }
            }
        }
    }

    // 3) last_crash_fix marker only if snapshot_id matches
    if actions_summary.is_empty() {
        if let Ok(Some(marker)) = swarm_api::peek_last_crash_fix_marker(Path::new(&project_dir))
        {
            if marker.snapshot_id == id {
                actions_summary = marker
                    .actions
                    .iter()
                    .map(swarm_api::format_launcher_action_summary)
                    .collect();
                if actions_summary.is_empty() && !marker.human_explanation.trim().is_empty() {
                    actions_summary.push(marker.human_explanation.clone());
                }
                if human_explanation.is_none() {
                    human_explanation = Some(marker.human_explanation.clone());
                }
                if plan_actions.is_empty() {
                    plan_actions = marker.actions;
                }
            }
        }
    }

    // 4) pack events
    if actions_summary.is_empty() {
        for ev in &related_events {
            if !ev.summary.trim().is_empty() {
                actions_summary.push(ev.summary.clone());
            }
        }
    }

    // 5) parse auto-before-{op} / reason fallback
    if actions_summary.is_empty() {
        let op = if !snapshot.operation.is_empty() {
            snapshot.operation.clone()
        } else if let Some(rest) = snapshot.name.strip_prefix("auto-before-") {
            rest.to_string()
        } else {
            String::new()
        };
        if !op.is_empty() {
            actions_summary.push(format!("Safety point before {op}"));
        } else if !snapshot.reason.trim().is_empty() {
            actions_summary.push(snapshot.reason.clone());
        } else {
            actions_summary.push("Manual snapshot".into());
        }
    }

    let changed_files: Vec<SnapshotChangedFile> = snapshot
        .changed_files
        .iter()
        .map(|p| {
            let path = p.to_string_lossy().replace('\\', "/");
            SnapshotChangedFile {
                category: pack_events::category_for_path(&path).to_string(),
                path,
            }
        })
        .collect();
    let manifest_only = changed_files.is_empty();

    Ok(SnapshotDetail {
        snapshot,
        actions_summary,
        related_events,
        plan_actions,
        human_explanation,
        changed_files,
        manifest_only,
    })
}

#[tauri::command(rename_all = "camelCase")]
fn get_snapshot_file_diff(
    project_dir: String,
    from: String,
    to: String,
    relative_path: String,
) -> Result<SnapshotFileDiff, String> {
    let relative = validate_relative_snapshot_path(&relative_path)?;
    let base = PathBuf::from(project_dir)
        .join(".tuffbox")
        .join("snapshots");
    let from_path = base.join(&from).join("changed_files").join(&relative);
    let to_path = base.join(&to).join("changed_files").join(&relative);
    let from_exists = from_path.is_file();
    let to_exists = to_path.is_file();
    let from_text = read_small_text_file(&from_path)?;
    let to_text = read_small_text_file(&to_path)?;
    Ok(SnapshotFileDiff {
        path: relative_path,
        from_exists,
        to_exists,
        text: unified_text_diff(&from_text, &to_text),
    })
}

#[tauri::command(rename_all = "camelCase")]
async fn validate_modrinth_export(path: String) -> Result<Vec<tuffbox_core::ExportIssue>, String> {
    // Blocking-pool conversion: sync commands run on the main thread (see
    // get_health_report); zipping a multi-GB pack must not freeze the app.
    tokio::task::spawn_blocking(move || validate_modrinth_export_impl(path))
        .await
        .map_err(|e| e.to_string())?
}

fn validate_modrinth_export_impl(path: String) -> Result<Vec<tuffbox_core::ExportIssue>, String> {
    let manifest_path = resolve_manifest_path(&path)?;
    let manifest =
        ProjectManifest::load_from_path(&manifest_path).map_err(|e| e.to_string())?;
    Ok(tuffbox_core::validate_modrinth_export(&manifest))
}

#[tauri::command(rename_all = "camelCase")]
fn validate_curseforge_export(path: String) -> Result<Vec<tuffbox_core::ExportIssue>, String> {
    let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
    Ok(tuffbox_core::validate_curseforge_export(&manifest))
}

#[tauri::command(rename_all = "camelCase")]
fn generate_release_changelog(path: String) -> Result<String, String> {
    let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
    let graph = DependencyGraph::from_manifest(&manifest);
    let diagnostics = Resolver::analyze_project(&manifest, &graph);
    let project_dir = manifest_parent(&path)?;
    let snapshots = SnapshotStore::new(&project_dir).list().unwrap_or_default();
    let mut out = String::new();
    out.push_str(&format!(
        "# {} {}\n\n",
        manifest.project.name, manifest.project.version
    ));
    if let Some(description) = &manifest.project.description {
        out.push_str(description);
        out.push_str("\n\n");
    }
    if let Some(brief) = &manifest.brief {
        if !brief.goal.trim().is_empty() {
            out.push_str(&format!("## Goal\n\n{}\n\n", brief.goal.trim()));
        }
    }
    out.push_str("## Platform\n\n");
    out.push_str(&format!("- Minecraft: {}\n", manifest.minecraft.version));
    out.push_str(&format!(
        "- Loader: {:?} {}\n",
        manifest.loader.kind, manifest.loader.version
    ));
    out.push_str(&format!("- Mods: {}\n\n", manifest.mods.len()));
    out.push_str("## Included mods\n\n");
    for module in &manifest.mods {
        out.push_str(&format!(
            "- {} `{}` ({:?})\n",
            module.name, module.version, module.side
        ));
    }
    out.push_str("\n## Diagnostics\n\n");
    if diagnostics.is_empty() {
        out.push_str("- No current diagnostics.\n");
    } else {
        for diagnostic in diagnostics {
            out.push_str(&format!(
                "- {:?}: {} — {}\n",
                diagnostic.severity, diagnostic.code, diagnostic.message
            ));
        }
    }
    out.push_str("\n## Recent snapshots\n\n");
    for snapshot in snapshots.iter().rev().take(5) {
        out.push_str(&format!(
            "- {} — {} ({})\n",
            snapshot.created_at, snapshot.name, snapshot.reason
        ));
    }
    Ok(out)
}

#[tauri::command(rename_all = "camelCase")]
fn update_project_version(
    app: tauri::AppHandle,
    path: String,
    version: String,
) -> Result<ProjectSummary, String> {
    if version.trim().is_empty() {
        return Err("version cannot be empty".to_string());
    }
    let manifest_path = PathBuf::from(&path);
    auto_snapshot(&manifest_path, "version-bump").map_err(|e| e.to_string())?;
    let mut manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
    manifest.project.version = version;
    save_manifest(&manifest_path, &manifest).map_err(|e| e.to_string())?;
    validate_project(app, path)
}

#[tauri::command(rename_all = "camelCase")]
fn create_release_snapshot(
    path: String,
    changelog: String,
) -> Result<ReleaseSnapshotResult, String> {
    let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
    let manifest_path = PathBuf::from(&path);
    let project_dir = manifest_parent(&path)?;
    let changelog_dir = project_dir.join("releases");
    std::fs::create_dir_all(&changelog_dir).map_err(|e| e.to_string())?;
    let changelog_path = changelog_dir.join(format!("{}-CHANGELOG.md", manifest.project.version));
    std::fs::write(&changelog_path, changelog).map_err(|e| e.to_string())?;
    let snapshot = auto_snapshot_with_changed_files(
        &manifest_path,
        "release",
        &[PathBuf::from("releases").join(format!("{}-CHANGELOG.md", manifest.project.version))],
    )
    .map_err(|e| e.to_string())?;
    Ok(ReleaseSnapshotResult {
        snapshot,
        changelog_path: changelog_path.to_string_lossy().to_string(),
    })
}

fn resolve_export_output(
    manifest_path: &Path,
    manifest: &ProjectManifest,
    target_path: Option<String>,
    suffix: &str,
    ext: &str,
) -> PathBuf {
    if let Some(tp) = target_path.filter(|s| !s.trim().is_empty()) {
        return PathBuf::from(tp);
    }
    let dir = manifest_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("export");
    let _ = std::fs::create_dir_all(&dir);
    let name = if suffix.is_empty() {
        format!("{}-{}.{}", manifest.project.id, manifest.project.version, ext)
    } else {
        format!(
            "{}-{}-{}.{}",
            manifest.project.id, manifest.project.version, suffix, ext
        )
    };
    dir.join(name)
}

#[tauri::command(rename_all = "camelCase")]
async fn export_modrinth_pack(
    path: String,
    target_path: Option<String>,
) -> Result<tuffbox_core::ExportResult, String> {
    // Blocking-pool conversion: sync commands run on the main thread (see
    // get_health_report); zipping a multi-GB pack must not freeze the app.
    tokio::task::spawn_blocking(move || export_modrinth_pack_impl(path, target_path))
        .await
        .map_err(|e| e.to_string())?
}

fn export_modrinth_pack_impl(
    path: String,
    target_path: Option<String>,
) -> Result<tuffbox_core::ExportResult, String> {
    let manifest_path = resolve_manifest_path(&path)?;
    let manifest =
        ProjectManifest::load_from_path(&manifest_path).map_err(|e| e.to_string())?;
    // Hard-block known fatal issues before writing a broken pack.
    let issues = tuffbox_core::validate_modrinth_export(&manifest);
    if issues
        .iter()
        .any(|i| matches!(i.severity, tuffbox_core::ExportIssueSeverity::Error))
    {
        let msgs: Vec<_> = issues
            .iter()
            .filter(|i| matches!(i.severity, tuffbox_core::ExportIssueSeverity::Error))
            .map(|i| i.message.clone())
            .collect();
        return Err(format!("export blocked: {}", msgs.join("; ")));
    }
    let output = resolve_export_output(&manifest_path, &manifest, target_path, "", "mrpack");
    let result = tuffbox_core::export_modrinth_pack(&manifest, &manifest_path, &output)
        .map_err(|e| e.to_string())?;
    let mp = manifest_path.to_string_lossy().to_string();
    append_release_artifact(&mp, "mrpack", &result).map_err(|e| e.to_string())?;
    swarm_api::spawn_pack_cooccurrence(mp, "pack_export");
    Ok(result)
}

/// Export preview: composition of the would-be archive without writing it.
/// Reuses the same manifest walk as the real exporter so counts match what a
/// subsequent export produces.
#[tauri::command(rename_all = "camelCase")]
async fn export_preview(path: String, kind: String) -> Result<serde_json::Value, String> {
    // Blocking-pool conversion: sync commands run on the main thread (see
    // get_health_report); zipping a multi-GB pack must not freeze the app.
    tokio::task::spawn_blocking(move || export_preview_impl(path, kind))
        .await
        .map_err(|e| e.to_string())?
}

fn export_preview_impl(path: String, kind: String) -> Result<serde_json::Value, String> {
    let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
    let manifest_dir = PathBuf::from(&path);
    let project_dir = manifest_dir
        .parent()
        .ok_or_else(|| "manifest has no parent directory".to_string())?
        .to_path_buf();

    let remote_mods = manifest
        .mods
        .iter()
        .filter(|m| m.source.url.is_some())
        .count();
    let local_mods = manifest.mods.iter().filter(|m| m.source.url.is_none()).count();

    // Count override files on disk per top-level content folder.
    let count_tree = |sub: &str| -> usize {
        fn walk(dir: &Path, out: &mut usize) {
            let Ok(rd) = std::fs::read_dir(dir) else { return };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p.is_file() {
                    *out += 1;
                }
            }
        }
        let mut n = 0usize;
        walk(&project_dir.join(sub), &mut n);
        n
    };

    let overrides = ["config", "defaultconfigs", "kubejs", "scripts", "shaderpacks", "resourcepacks"]
        .iter()
        .map(|s| count_tree(s))
        .sum::<usize>();

    Ok(serde_json::json!({
        "kind": kind,
        "modCount": manifest.mods.len(),
        "remoteMods": remote_mods,
        "localMods": local_mods,
        "overrideFiles": overrides,
        "mcVersion": manifest.minecraft.version,
        "loaderKind": format!("{:?}", manifest.loader.kind).to_lowercase(),
        "loaderVersion": manifest.loader.version,
    }))
}

#[tauri::command(rename_all = "camelCase")]
async fn export_server_pack(
    path: String,
    target_path: Option<String>,
) -> Result<tuffbox_core::ExportResult, String> {
    // Blocking-pool conversion: sync commands run on the main thread (see
    // get_health_report); zipping a multi-GB pack must not freeze the app.
    tokio::task::spawn_blocking(move || export_server_pack_impl(path, target_path))
        .await
        .map_err(|e| e.to_string())?
}

fn export_server_pack_impl(
    path: String,
    target_path: Option<String>,
) -> Result<tuffbox_core::ExportResult, String> {
    let manifest_path = resolve_manifest_path(&path)?;
    let manifest =
        ProjectManifest::load_from_path(&manifest_path).map_err(|e| e.to_string())?;
    let output =
        resolve_export_output(&manifest_path, &manifest, target_path, "server", "zip");
    let result = tuffbox_core::export_server_pack(&manifest, &manifest_path, &output)
        .map_err(|e| e.to_string())?;
    let mp = manifest_path.to_string_lossy().to_string();
    append_release_artifact(&mp, "server", &result).map_err(|e| e.to_string())?;
    swarm_api::spawn_pack_cooccurrence(mp, "pack_export");
    Ok(result)
}

#[tauri::command(rename_all = "camelCase")]
async fn export_prism_instance(
    path: String,
    target_path: Option<String>,
) -> Result<tuffbox_core::ExportResult, String> {
    // Blocking-pool conversion: sync commands run on the main thread (see
    // get_health_report); zipping a multi-GB pack must not freeze the app.
    tokio::task::spawn_blocking(move || export_prism_instance_impl(path, target_path))
        .await
        .map_err(|e| e.to_string())?
}

fn export_prism_instance_impl(
    path: String,
    target_path: Option<String>,
) -> Result<tuffbox_core::ExportResult, String> {
    let manifest_path = resolve_manifest_path(&path)?;
    let manifest =
        ProjectManifest::load_from_path(&manifest_path).map_err(|e| e.to_string())?;
    let output =
        resolve_export_output(&manifest_path, &manifest, target_path, "prism", "zip");
    let result = tuffbox_core::export_prism_instance(&manifest, &manifest_path, &output)
        .map_err(|e| e.to_string())?;
    let mp = manifest_path.to_string_lossy().to_string();
    append_release_artifact(&mp, "prism", &result).map_err(|e| e.to_string())?;
    swarm_api::spawn_pack_cooccurrence(mp, "pack_export");
    Ok(result)
}

#[tauri::command(rename_all = "camelCase")]
async fn export_curseforge_pack(
    path: String,
    target_path: Option<String>,
) -> Result<tuffbox_core::ExportResult, String> {
    // Blocking-pool conversion: sync commands run on the main thread (see
    // get_health_report); zipping a multi-GB pack must not freeze the app.
    tokio::task::spawn_blocking(move || export_curseforge_pack_impl(path, target_path))
        .await
        .map_err(|e| e.to_string())?
}

fn export_curseforge_pack_impl(
    path: String,
    target_path: Option<String>,
) -> Result<tuffbox_core::ExportResult, String> {
    let manifest_path = resolve_manifest_path(&path)?;
    let manifest =
        ProjectManifest::load_from_path(&manifest_path).map_err(|e| e.to_string())?;
    let output =
        resolve_export_output(&manifest_path, &manifest, target_path, "curseforge", "zip");
    let result = tuffbox_core::export_curseforge_pack(&manifest, &manifest_path, &output)
        .map_err(|e| e.to_string())?;
    let mp = manifest_path.to_string_lossy().to_string();
    append_release_artifact(&mp, "curseforge", &result).map_err(|e| e.to_string())?;
    swarm_api::spawn_pack_cooccurrence(mp, "pack_export");
    Ok(result)
}

/// Open the exported file or its parent folder in the OS file manager.
/// Creates the directory when a non-existent folder path is requested
/// (e.g. first-time "Open export folder").
#[tauri::command(rename_all = "camelCase")]
#[allow(deprecated)]
fn reveal_export_path(app: tauri::AppHandle, path: String) -> Result<(), String> {
    use tauri_plugin_shell::ShellExt;
    let pb = PathBuf::from(&path);
    let target = if pb.is_file() {
        pb.parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| pb.clone())
    } else if pb.is_dir() {
        pb
    } else if path.ends_with('/')
        || path.ends_with('\\')
        || pb.extension().is_none()
    {
        // Treat as a directory that may not exist yet.
        std::fs::create_dir_all(&pb).map_err(|e| e.to_string())?;
        pb
    } else {
        // File path that doesn't exist yet — open parent.
        let parent = pb
            .parent()
            .map(|p| p.to_path_buf())
            .ok_or_else(|| "invalid path".to_string())?;
        std::fs::create_dir_all(&parent).map_err(|e| e.to_string())?;
        parent
    };
    app.shell()
        .open(target.to_string_lossy().to_string(), None)
        .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
fn export_packwiz_pack(
    path: String,
    target_path: Option<String>,
) -> Result<tuffbox_core::ExportResult, String> {
    let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
    let output = target_path.map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(&path)
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(format!(
                "{}-{}-packwiz",
                manifest.project.id, manifest.project.version
            ))
    });
    let result =
        tuffbox_core::export_packwiz_pack(&manifest, Path::new(&path), &output).map_err(|e| e.to_string())?;
    let mapped = tuffbox_core::ExportResult {
        path: result.path,
        file_count: result.file_count,
        override_count: result.override_count,
        warnings: Vec::new(),
    };
    append_release_artifact(&path, "packwiz", &mapped).map_err(|e| e.to_string())?;
    swarm_api::spawn_pack_cooccurrence(path, "pack_export");
    Ok(mapped)
}

#[tauri::command(rename_all = "camelCase")]
fn list_release_artifacts(path: String) -> Result<Vec<ReleaseArtifactRecord>, String> {
    let project_dir = manifest_parent(&path)?;
    let artifacts_path = project_dir.join(".tuffbox").join("artifacts.json");
    if !artifacts_path.is_file() {
        return Ok(Vec::new());
    }
    let raw = std::fs::read_to_string(artifacts_path).map_err(|e| e.to_string())?;
    let mut artifacts: Vec<ReleaseArtifactRecord> = serde_json::from_str(&raw).unwrap_or_default();
    artifacts.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(artifacts)
}

#[tauri::command(rename_all = "camelCase")]
fn create_release_draft(path: String, changelog: String) -> Result<ReleaseDraftResult, String> {
    let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
    let project_dir = manifest_parent(&path)?;
    let artifacts = list_release_artifacts(path.clone()).unwrap_or_default();
    let releases_dir = project_dir.join("releases");
    std::fs::create_dir_all(&releases_dir).map_err(|e| e.to_string())?;
    let draft_path = releases_dir.join(format!("{}-DRAFT.md", manifest.project.version));
    let metadata_dir = project_dir.join(".tuffbox");
    std::fs::create_dir_all(&metadata_dir).map_err(|e| e.to_string())?;
    let metadata_path = metadata_dir.join("release-draft.json");

    let mut markdown = String::new();
    markdown.push_str(&format!(
        "# {} {} release draft

",
        manifest.project.name, manifest.project.version
    ));
    markdown.push_str(
        "## Changelog

",
    );
    markdown.push_str(changelog.trim());
    markdown.push_str(
        "

## Artifacts

",
    );
    if artifacts.is_empty() {
        markdown.push_str(
            "- No artifacts exported yet.
",
        );
    } else {
        for artifact in &artifacts {
            markdown.push_str(&format!(
                "- **{}**: `{}` ({} files, {} overrides)
",
                artifact.kind, artifact.path, artifact.file_count, artifact.override_count
            ));
        }
    }
    markdown.push_str(
        "
## Publish checklist

",
    );
    markdown.push_str(
        "- [ ] Upload artifacts to target platform
",
    );
    markdown.push_str(
        "- [ ] Verify game/loader versions
",
    );
    markdown.push_str(
        "- [ ] Verify server pack starts
",
    );
    markdown.push_str(
        "- [ ] Announce known issues
",
    );
    std::fs::write(&draft_path, markdown).map_err(|e| e.to_string())?;

    let artifact_count = artifacts.len();
    let publish_config = integrations::get_publish_config(path.clone()).unwrap_or_default();
    let metadata = serde_json::json!({
        "projectId": manifest.project.id.clone(),
        "version": manifest.project.version.clone(),
        "draftPath": draft_path.to_string_lossy().to_string(),
        "artifacts": artifacts,
        "createdAt": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .to_string(),
        "targets": {
            "modrinth": {
                "configured": !publish_config.modrinth_project_id.is_empty(),
                "projectId": publish_config.modrinth_project_id,
            },
            "curseforge": {
                "configured": !publish_config.curseforge_project_id.is_empty(),
                "projectId": publish_config.curseforge_project_id,
                "gameVersionIds": publish_config.curseforge_game_version_ids,
            },
            "githubReleases": {
                "configured": !publish_config.github_repository.is_empty(),
                "repository": publish_config.github_repository,
            }
        }
    });
    std::fs::write(
        &metadata_path,
        serde_json::to_string_pretty(&metadata).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;

    Ok(ReleaseDraftResult {
        draft_path: draft_path.to_string_lossy().to_string(),
        metadata_path: metadata_path.to_string_lossy().to_string(),
        artifact_count,
    })
}

#[tauri::command]
fn generate_lockfile(path: String) -> Result<TuffboxLockfile, String> {
    let manifest_path = PathBuf::from(&path);
    let manifest = ProjectManifest::load_from_path(&manifest_path).map_err(|e| e.to_string())?;
    persist_lockfile_for_manifest(&manifest_path, &manifest).map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
fn capture_test_run_logs(path: String, run_id: String) -> Result<String, String> {
    let project_dir = manifest_parent(&path)?;
    let target_dir = project_dir.join(".tuffbox").join("test-runs").join(&run_id);
    std::fs::create_dir_all(&target_dir).map_err(|e| e.to_string())?;
    let candidates = [
        (project_dir.join("logs").join("latest.log"), "latest.log"),
        (
            project_dir.join("logs").join("tuffbox-console.log"),
            "tuffbox-console.log",
        ),
        (project_dir.join("launcher.log"), "launcher.log"),
        (project_dir.join("launcher_log.txt"), "launcher_log.txt"),
        (
            project_dir.join("logs").join("launcher.log"),
            "logs-launcher.log",
        ),
        (
            project_dir.join("logs").join("launcher_log.txt"),
            "logs-launcher_log.txt",
        ),
    ];
    let mut captured_paths: Vec<String> = Vec::new();
    for (src, name) in candidates {
        if src.is_file() {
            std::fs::copy(&src, target_dir.join(name)).map_err(|e| e.to_string())?;
            captured_paths.push(name.to_string());
        }
    }

    // Copy recent crash-reports if present.
    let crash_src = project_dir.join("crash-reports");
    let crash_dst = target_dir.join("crash-reports");
    if crash_src.is_dir() {
        let _ = std::fs::create_dir_all(&crash_dst);
        if let Ok(entries) = std::fs::read_dir(&crash_src) {
            let mut reports: Vec<_> = entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().map_or(false, |x| x == "txt"))
                .collect();
            reports.sort_by_key(|e| {
                e.metadata()
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0)
            });
            for entry in reports.into_iter().rev().take(5) {
                let name = entry.file_name();
                let dest = crash_dst.join(&name);
                if std::fs::copy(entry.path(), &dest).is_ok() {
                    captured_paths.push(format!(
                        "crash-reports/{}",
                        name.to_string_lossy()
                    ));
                }
            }
        }
    }

    if captured_paths.is_empty() {
        return Err("no logs found to capture".to_string());
    }

    // Persist captured paths onto the run record when present.
    let runs_path = project_dir.join(".tuffbox").join("test-runs.json");
    if runs_path.is_file() {
        if let Ok(raw) = std::fs::read_to_string(&runs_path) {
            if let Ok(mut runs) = serde_json::from_str::<Vec<TestRunRecord>>(&raw) {
                if let Some(run) = runs.iter_mut().find(|r| r.id == run_id) {
                    run.captured_paths = captured_paths.clone();
                    let _ = std::fs::write(
                        &runs_path,
                        serde_json::to_string_pretty(&runs).unwrap_or_default(),
                    );
                }
            }
        }
    }

    Ok(target_dir.to_string_lossy().to_string())
}

#[tauri::command(rename_all = "camelCase")]
fn finalize_test_run(
    path: String,
    run_id: String,
    status: String,
    duration_seconds: Option<u64>,
    verdict_reason: Option<String>,
    peak_proc_mb: Option<u64>,
    peak_host_mb: Option<u64>,
    host_total_mb: Option<u64>,
    xmx_mb: Option<u64>,
) -> Result<TestRunRecord, String> {
    let project_dir = manifest_parent(&path)?;
    let runs_path = project_dir.join(".tuffbox").join("test-runs.json");
    if !runs_path.is_file() {
        return Err("no test runs recorded".to_string());
    }
    let raw = std::fs::read_to_string(&runs_path).map_err(|e| e.to_string())?;
    let mut runs: Vec<TestRunRecord> = serde_json::from_str(&raw).unwrap_or_default();
    let run = runs
        .iter_mut()
        .find(|r| r.id == run_id)
        .ok_or_else(|| format!("run {run_id} not found"))?;
    run.status = status;
    if let Some(secs) = duration_seconds {
        run.duration_seconds = Some(secs);
    } else if run.duration_seconds.is_none() {
        if let Ok(started) = run.started_at.parse::<u64>() {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            run.duration_seconds = Some(now.saturating_sub(started));
        }
    }
    if verdict_reason.is_some() {
        run.verdict_reason = verdict_reason;
    }
    if peak_proc_mb.is_some() {
        run.peak_proc_mb = peak_proc_mb;
    }
    if peak_host_mb.is_some() {
        run.peak_host_mb = peak_host_mb;
    }
    if let (Some(proc), Some(host), Some(total)) = (run.peak_proc_mb, run.peak_host_mb, host_total_mb)
    {
        let advice = tuffbox_core::test_load::recommend_ram(
            proc,
            xmx_mb.unwrap_or(4096),
            host,
            total,
        );
        run.recommended_ram_gb = Some(advice.recommended_gb);
    }
    let finished = run.clone();
    std::fs::write(
        &runs_path,
        serde_json::to_string_pretty(&runs).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(finished)
}

#[tauri::command(rename_all = "camelCase")]
fn list_test_runs(path: String) -> Result<Vec<TestRunRecord>, String> {
    let project_dir = manifest_parent(&path)?;
    let runs_path = project_dir.join(".tuffbox").join("test-runs.json");
    if !runs_path.is_file() {
        return Ok(Vec::new());
    }
    let raw = std::fs::read_to_string(&runs_path).map_err(|e| e.to_string())?;
    let mut runs: Vec<TestRunRecord> = serde_json::from_str(&raw).unwrap_or_default();
    for run in &mut runs {
        // Only soft-update in-flight runs; finalized verdicts are authoritative.
        if run.status != "started" {
            continue;
        }
        let log_path = PathBuf::from(&run.log_path);
        if let Ok(log) = tuffbox_core::process::read_log_tail(&log_path, 200) {
            if log.contains("# Launch error:") {
                run.status = "fail".to_string();
            } else if log.contains("Minecraft has crashed") || log.contains("Exception in thread") {
                run.status = "fail".to_string();
            } else if log.contains("Process exited") || log.contains("Stopping!") {
                run.status = "finished".to_string();
            }
            if run.status != "started" && run.duration_seconds.is_none() {
                if let Ok(started) = run.started_at.parse::<u64>() {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    run.duration_seconds = Some(now.saturating_sub(started));
                }
            }
        }
    }
    runs.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    Ok(runs)
}

/// Returns true if crashes were detected in the project.
#[tauri::command(rename_all = "camelCase")]
fn has_crashed(path: String) -> Result<bool, String> {
    let project_dir = manifest_parent(&path)?;
    let crash_dir = project_dir.join("crash-reports");
    if crash_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&crash_dir) {
            if entries
                .filter_map(|e| e.ok())
                .any(|e| e.path().extension().map_or(false, |x| x == "txt"))
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

#[tauri::command(rename_all = "camelCase")]
async fn launch_with_quick_play(
    app: tauri::AppHandle,
    path: String,
    profile: String,
    quick_play_type: Option<String>,
    quick_play_value: Option<String>,
    memory_mb_override: Option<u32>,
) -> Result<tuffbox_core::LaunchResult, LaunchErrorInfo> {
    launch_profile_impl(
        app,
        path,
        profile,
        quick_play_type,
        quick_play_value,
        memory_mb_override,
        None,
        false,
        false,
    )
    .await
}

#[tauri::command(rename_all = "camelCase")]
async fn launch_profile(
    app: tauri::AppHandle,
    path: String,
    profile: String,
    memory_mb_override: Option<u32>,
) -> Result<tuffbox_core::LaunchResult, LaunchErrorInfo> {
    launch_profile_impl(
        app,
        path,
        profile,
        None,
        None,
        memory_mb_override,
        None,
        false,
        false,
    )
    .await
}

/// Emit the one lifecycle contract consumed by every frontend Play control.
/// The command return only means spawn preparation finished; this event stream
/// carries the user-visible state through preflight, JVM startup and exit.
fn emit_launch_phase(
    app: &tauri::AppHandle,
    id: &str,
    profile: &str,
    phase: LaunchPhase,
    message: impl Into<String>,
    log_path: Option<&Path>,
    pid: Option<u32>,
    started_at: Option<u64>,
    exit_code: Option<i32>,
    stopped: bool,
    error: Option<LaunchErrorInfo>,
) {
    let _ = app.emit(
        "launch-phase",
        LaunchLifecycleEvent {
            id: id.to_string(),
            profile: profile.to_string(),
            phase,
            message: message.into(),
            log_path: log_path.map(|path| path.to_string_lossy().into_owned()),
            pid,
            started_at,
            exit_code,
            stopped,
            error,
        },
    );
}

/// Convenience wrapper used by arena-style 4-arg calls in the launch path.
/// Converts string phase to `LaunchPhase` enum and fills defaults for the
/// remaining parameters.
fn emit_launch_phase_simple(app: &tauri::AppHandle, path: &str, phase: &str, message: &str) {
    let launch_phase = match phase {
        "preparing" => LaunchPhase::Preflight,
        "preflight" => LaunchPhase::Preflight,
        "resolving_java" => LaunchPhase::ResolvingJava,
        "downloading" => LaunchPhase::Downloading,
        "starting" => LaunchPhase::Starting,
        "running" => LaunchPhase::Running,
        "stopping" => LaunchPhase::Stopping,
        "exited" => LaunchPhase::Exited,
        "failed" => LaunchPhase::Failed,
        _ => LaunchPhase::Preflight,
    };
    emit_launch_phase(
        app,
        path,
        "",
        launch_phase,
        message,
        None,
        None,
        None,
        None,
        false,
        None,
    );
}

/// Outer error boundary for every client/profile launch. It deliberately
/// converts *all* failures (including setup before the blocking task starts)
/// into the same structured `LaunchErrorInfo` event used by asynchronous JVM
/// crashes, so the frontend never has to guess whether a spinner may clear.
async fn launch_profile_impl(
    app: tauri::AppHandle,
    path: String,
    profile: String,
    quick_play_type: Option<String>,
    quick_play_value: Option<String>,
    memory_mb_override: Option<u32>,
    game_dir_override: Option<PathBuf>,
    show_console: bool,
    skip_client_bridges: bool,
) -> Result<tuffbox_core::LaunchResult, LaunchErrorInfo> {
    let id_hint = path.clone();
    let profile_hint = profile.clone();
    emit_launch_phase(
        &app,
        &id_hint,
        &profile_hint,
        LaunchPhase::Preflight,
        "Preparing launch…",
        None,
        None,
        None,
        None,
        false,
        None,
    );
    let result = launch_profile_impl_inner(
        app.clone(),
        path,
        profile,
        quick_play_type,
        quick_play_value,
        memory_mb_override,
        game_dir_override,
        show_console,
        skip_client_bridges,
    )
    .await;

    if let Err(info) = &result {
        emit_launch_phase(
            &app,
            &id_hint,
            &profile_hint,
            LaunchPhase::Failed,
            info.message.clone(),
            info.log_path.as_deref().map(|log_path| Path::new(log_path)),
            None,
            None,
            None,
            false,
            Some(info.clone()),
        );
    }

    result
}

async fn launch_profile_impl_inner(
    app: tauri::AppHandle,
    path: String,
    profile: String,
    quick_play_type: Option<String>,
    quick_play_value: Option<String>,
    memory_mb_override: Option<u32>,
    game_dir_override: Option<PathBuf>,
    show_console: bool,
    skip_client_bridges: bool,
) -> Result<tuffbox_core::LaunchResult, LaunchErrorInfo> {
    let path = resolve_manifest_path(&path)
        .map_err(|error| LaunchErrorInfo::new(LaunchErrorKind::Install, error))?
        .to_string_lossy()
        .to_string();

    let project_dir = PathBuf::from(&path)
        .parent()
        .map(|parent| parent.to_path_buf())
        .ok_or_else(|| {
            LaunchErrorInfo::new(
                LaunchErrorKind::Unknown,
                "manifest has no parent directory",
            )
        })?;
    let game_dir = game_dir_override
        .clone()
        .unwrap_or_else(|| project_dir.clone());
    let logs_dir = game_dir.join("logs");
    // Minecraft (log4j) owns `latest.log`. We must NOT truncate it — that wiped
    // real crash evidence and raced the game writer. Console capture goes to a
    // separate TuffBox file; diagnose still reads `logs/latest.log`.
    let console_log = logs_dir.join("tuffbox-console.log");
    let latest_log = logs_dir.join("latest.log");

    std::fs::create_dir_all(&logs_dir).map_err(|error| {
        LaunchErrorInfo::new(LaunchErrorKind::Unknown, error.to_string()).with_log(&console_log)
    })?;
    {
        use std::io::Write;
        let mut console = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&console_log)
            .map_err(|error| {
                LaunchErrorInfo::new(LaunchErrorKind::Unknown, error.to_string())
                    .with_log(&console_log)
            })?;
        writeln!(console, "# TuffBox launching profile {profile}").ok();
        if game_dir_override.is_some() {
            writeln!(console, "# Game directory override: {}", game_dir.display()).ok();
        }
        if let Some(mb) = memory_mb_override {
            writeln!(console, "# Memory override: {mb} MB").ok();
        }
        if let (Some(ref launch_type), Some(ref value)) = (&quick_play_type, &quick_play_value) {
            writeln!(console, "# Quick Play: {launch_type} → {value}").ok();
        }
        let launcher_log = project_dir.join("launcher_log.txt");
        let mut launcher = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&launcher_log)
            .map_err(|error| {
                LaunchErrorInfo::new(LaunchErrorKind::Unknown, error.to_string())
                    .with_log(&console_log)
            })?;
        writeln!(launcher, "# TuffBox launching profile {profile}").ok();
    }

    emit_launch_phase(
        &app,
        &path,
        &profile,
        LaunchPhase::Preflight,
        "Checking launch prerequisites…",
        Some(&console_log),
        None,
        None,
        None,
        false,
        None,
    );

    append_test_run_record(&path, &profile, &latest_log).map_err(|error| {
        LaunchErrorInfo::new(LaunchErrorKind::Unknown, error.to_string()).with_log(&console_log)
    })?;

    let console_log_clone = console_log.clone();
    let latest_log_clone = latest_log.clone();
    let game_dir_clone = game_dir_override.clone();
    let app_for_task = app.clone();
    // Run the (blocking) install + spawn on a blocking thread, then await the
    // result so install/prepare failures surface to the UI as a structured,
    // categorized error instead of being swallowed into the log file.
    let result = tokio::task::spawn_blocking(move || {
        build_and_spawn(
            path,
            profile,
            console_log_clone,
            latest_log_clone,
            app_for_task,
            quick_play_type,
            quick_play_value,
            memory_mb_override,
            game_dir_clone,
            show_console,
            skip_client_bridges,
        )
    })
    .await
    .map_err(|error| {
        LaunchErrorInfo::new(
            LaunchErrorKind::Unknown,
            format!("launch task panicked: {error}"),
        )
        .with_log(&latest_log)
    })?;

    match result {
        Ok(running) => Ok(tuffbox_core::LaunchResult {
            exit_code: None,
            log_path: latest_log,
            instance_id: running.id,
            profile: running.profile_id,
            pid: running.pid,
            started_at: running.started_at,
        }),
        Err(info) => Err(info),
    }
}

fn emit_launch_progress(
    app: &tauri::AppHandle,
    phase: &str,
    message: &str,
    percent: Option<u32>,
) {
    use tauri::Emitter;
    let _ = app.emit(
        "launch-progress",
        serde_json::json!({
            "phase": phase,
            "message": message,
            "percent": percent,
        }),
    );
}

/// A jar found in the instance's mods folder that was built for a different
/// mod loader than the project uses.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PreflightWrongLoaderJar {
    file_name: String,
    /// Loader the project expects (e.g. "fabric").
    expected: String,
    /// Loader(s) the jar was actually built for.
    found: String,
}

/// Result of the pre-spawn 'Play does not lie' sanity check.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct PreflightReport {
    wrong_loader: Vec<PreflightWrongLoaderJar>,
    /// Manifest-tracked files absent on disk. Non-fatal: the caller announces
    /// them as pending downloads instead of failing the launch.
    missing_jars: Vec<String>,
}

/// Pre-spawn preflight ('Play does not lie'): before any long install work,
/// verify that loose jars in the instance's mods folder match the project's
/// loader and that manifest-tracked files actually exist on disk.
///
/// - Wrong-loader jars fail this launch: they crash the game mid-boot with a
///   confusing loader error. The report names each offender so the UI can
///   point at Mods → Wrong loader / Repair.
/// - Missing tracked files are non-fatal: `ensure_project_mods_downloaded`
///   fetches them right after this check runs.
fn launch_preflight(
    game_dir: &Path,
    manifest: &ProjectManifest,
) -> Result<PreflightReport, String> {
    let project_loader = tuffbox_core::graph::loader_kind_slug(&manifest.loader.kind).to_string();

    // Tracked-but-missing files: same presence rule as
    // tuffbox_core::materialize_mod_file (non-empty file inside its
    // content-type folder). Entries disabled in the manifest are skipped so
    // we never announce a download that sync won't attempt.
    let mut missing_jars: Vec<String> = Vec::new();
    for module in &manifest.mods {
        let Some(file_name) = module.file_name.as_deref() else {
            continue;
        };
        if module
            .status
            .iter()
            .any(|s| s.eq_ignore_ascii_case("disabled"))
        {
            continue;
        }
        let target = tuffbox_core::content_dir_for(game_dir, module.content_type).join(file_name);
        let present = target.is_file()
            && std::fs::metadata(&target).map(|m| m.len() > 0).unwrap_or(false);
        if !present && !missing_jars.iter().any(|m| m == file_name) {
            missing_jars.push(file_name.to_string());
        }
    }

    // Loose-jar wrong-loader scan: exact same heuristic as the Diagnostics
    // command (scan_wrong_loader_jars).
    let tracked: Vec<String> = manifest
        .mods
        .iter()
        .filter_map(|m| m.file_name.clone())
        .collect();
    let wrong_loader = scan_wrong_loader_jars(&game_dir.join("mods"), &project_loader, &tracked)
        .into_iter()
        .map(|f| PreflightWrongLoaderJar {
            expected: project_loader.clone(),
            found: f.jar_loaders,
            file_name: f.file_name,
        })
        .collect();

    Ok(PreflightReport {
        wrong_loader,
        missing_jars,
    })
}
fn build_and_spawn(
    path: String,
    profile: String,
    console_log: PathBuf,
    latest_log: PathBuf,
    app: tauri::AppHandle,
    quick_play_type: Option<String>,
    quick_play_value: Option<String>,
    memory_mb_override: Option<u32>,
    game_dir_override: Option<PathBuf>,
    show_console: bool,
    skip_client_bridges: bool,
) -> Result<tuffbox_core::process::RunningProcess, LaunchErrorInfo> {
    use tuffbox_core::{LaunchOptions, TestLauncher};

    emit_launch_progress(&app, "preparing", "Preparing…", Some(5));

    let manifest_path = resolve_manifest_path(&path).map_err(|e| {
        LaunchErrorInfo::new(LaunchErrorKind::Install, e).with_log(&console_log)
    })?;
    let path = manifest_path.to_string_lossy().to_string();

    if tuffbox_core::process::is_instance_running(&path) {
        return Err(LaunchErrorInfo::new(
            LaunchErrorKind::Install,
            "This instance is already running. Stop it before launching again.",
        ));
    }
    emit_launch_phase_simple(&app, &path, "preparing", "Preparing profile…");

    let manifest = ProjectManifest::load_from_path(&manifest_path).map_err(|e| {
        LaunchErrorInfo::new(LaunchErrorKind::Install, e.to_string()).with_log(&console_log)
    })?;
    let project_profile = manifest
        .profiles
        .iter()
        .find(|p| p.id == profile)
        .ok_or_else(|| {
            LaunchErrorInfo::new(LaunchErrorKind::Install, format!("profile {profile} not found"))
                .with_log(&console_log)
        })?
        .clone();

    let launch_settings = launcher_settings::load_launcher_settings();

    let java_path = manifest
        .java
        .as_ref()
        .and_then(|j| j.path.clone())
        .or_else(|| {
            launch_settings
                .default_java_path
                .as_ref()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        });
    let progress = tuffbox_core::mc_install::InstallProgress {
        log_path: console_log.clone(),
    };

    emit_launch_phase(
        &app,
        &path,
        &profile,
        LaunchPhase::ResolvingJava,
        "Resolving a compatible Java runtime…",
        Some(&console_log),
        None,
        None,
        None,
        false,
        None,
    );

    let java = if let Some(java_path) = java_path {
        tuffbox_core::jre::check_java_at_path(&PathBuf::from(&java_path)).map_err(|e| {
            LaunchErrorInfo::new(LaunchErrorKind::JavaMissing, e.to_string()).with_log(&console_log)
        })?
    } else {
        // Auto-detect the best Java for this Minecraft version instead of
        // always grabbing whatever JVM happens to be newest on the system
        // — using e.g. Java 21 for Forge 1.20.1 (which needs Java 17)
        // fails deep inside Forge's bootstrap launcher with a confusing
        // module-system error instead of launching at all.
        // If nothing matching is installed, download a managed JDK
        // (GraalVM for modern majors, Temurin 8/16 for legacy versions).
        emit_launch_progress(&app, "java", "Installing Java…", Some(20));
        tuffbox_core::jre::ensure_java_for_minecraft_with_log(
            &manifest.minecraft.version,
            |line| progress.log(line),
        )
        .map_err(|e| {
            let kind = match e {
                tuffbox_core::jre::JreError::NotFound
                | tuffbox_core::jre::JreError::Download(_)
                | tuffbox_core::jre::JreError::Install(_) => LaunchErrorKind::JavaMissing,
                _ => LaunchErrorKind::Install,
            };
            LaunchErrorInfo::new(kind, e.to_string()).with_log(&console_log)
        })?
    };

    progress.log(&format!("# Java: {} (major {})", java.path, java.major));
    progress.log(&format!("# Java version: {}", java.version));
    let required_java = tuffbox_core::jre::required_java_major(&manifest.minecraft.version);
    TestLauncher::preflight_java_runtime(&manifest.minecraft.version, &java).map_err(|error| {
        LaunchErrorInfo::new(LaunchErrorKind::JavaMissing, error.to_string()).with_log(&console_log)
    })?;
    if java.major != required_java {
        progress.log(&format!(
            "# WARNING: Minecraft {} typically needs Java {required_java}, but the selected runtime is Java {}. \
             If the game fails to start, install Java {required_java} and select it in Project Settings.",
            manifest.minecraft.version, java.major
        ));
    }

    // game_dir = instance folder (project pack or staged server dir)
    let project_dir = PathBuf::from(&path)
        .parent()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| {
            LaunchErrorInfo::new(LaunchErrorKind::Unknown, "manifest has no parent directory")
                .with_log(&console_log)
        })?;
    let game_dir = game_dir_override.unwrap_or_else(|| project_dir.clone());

    // launcher_dir = shared game data (versions, libraries, assets)
    let launcher_dir = launcher_settings::resolve_runtime_path();

    std::fs::create_dir_all(&launcher_dir).map_err(|e| {
        LaunchErrorInfo::new(LaunchErrorKind::Install, e.to_string()).with_log(&console_log)
    })?;
    std::fs::create_dir_all(&game_dir).map_err(|e| {
        LaunchErrorInfo::new(LaunchErrorKind::Install, e.to_string()).with_log(&console_log)
    })?;

    progress.log(&format!("# Game directory: {}", game_dir.display()));
    progress.log(&format!("# Launcher directory: {}", launcher_dir.display()));

    if let Err(e) = launcher_settings::run_hook(launch_settings.pre_launch_hook.as_deref(), "pre-launch hook") {
        return Err(LaunchErrorInfo::new(LaunchErrorKind::Unknown, e).with_log(&console_log));
    }

    emit_launch_phase(
        &app,
        &path,
        &profile,
        LaunchPhase::Downloading,
        "Verifying mods, libraries, natives and assets…",
        Some(&console_log),
        None,
        None,
        None,
        false,
        None,
    );

    // Safety net: make sure every mod declared in the manifest actually has
    // its .jar on disk before we launch. Mods can end up missing here if
    // they were added while offline, if a previous download failed, or if
    // the manifest was hand-edited/imported without a download step.
    // Without this, TuffBox would happily launch vanilla Minecraft while the
    // UI still shows a full mod list.
    // For server runs, verify against the author project (source of jars);
    // the staged server dir already has a filtered copy.
    emit_launch_progress(&app, "mods", "Checking mods…", Some(35));
    emit_launch_phase_simple(&app, &path, "downloading", "Verifying content files…");
    progress.log("# Verifying mod files...");
    // 'Play does not lie' preflight: catch wrong-loader jars here instead of
    // after a failed boot, and announce pending downloads up front.
    emit_launch_progress(&app, "mods", "Preflight: checking mods…", Some(35));
    let preflight = launch_preflight(&game_dir, &manifest).map_err(|e| {
        LaunchErrorInfo::new(LaunchErrorKind::Install, e).with_log(&console_log)
    })?;
    if !preflight.wrong_loader.is_empty() {
        emit_launch_progress(
            &app,
            "mods",
            &format!(
                "Preflight: {} wrong-loader mod(s)",
                preflight.wrong_loader.len()
            ),
            Some(35),
        );
        let offenders: Vec<String> = preflight
            .wrong_loader
            .iter()
            .map(|w| {
                format!(
                    "{} (built for {}, project uses {})",
                    w.file_name, w.found, w.expected
                )
            })
            .collect();
        return Err(LaunchErrorInfo::new(
            LaunchErrorKind::Install,
            format!(
                "{} wrong-loader mod(s) cannot load in this project: {}. Disable it in Mods → Wrong loader (or run Repair).",
                preflight.wrong_loader.len(),
                offenders.join("; ")
            ),
        )
        .with_log(&console_log));
    }
    if !preflight.missing_jars.is_empty() {
        emit_launch_progress(
            &app,
            "mods",
            &format!("{} mods will be downloaded", preflight.missing_jars.len()),
            Some(35),
        );
        progress.log(&format!(
            "# Preflight: {} mod file(s) missing from disk, downloading…",
            preflight.missing_jars.len()
        ));
    }
    let sync_report = tuffbox_core::ensure_project_mods_downloaded(&manifest, &project_dir);

    // Shared options.txt sync (docs/17, Part 2): pull the version-group
    // template into this managed project before the game reads it. Runs
    // AFTER mod downloads (so a failed download never leaves a half-synced
    // options file behind a retry) and BEFORE Java starts. Never fails the
    // launch: options sync is a convenience, any error just gets logged.
    {
        let outcome = tuffbox_core::options_sync::sync_before_launch(
            &game_dir,
            &manifest.minecraft.version,
        );
        match outcome {
            tuffbox_core::options_sync::SyncOutcome::SeededFromGroup => {
                progress.log("# Options: seeded from shared settings for this MC version group.");
            }
            tuffbox_core::options_sync::SyncOutcome::Merged => {
                progress.log("# Options: merged shared settings (local edits kept).");
            }
            tuffbox_core::options_sync::SyncOutcome::Error(e) => {
                progress.log(&format!("# WARNING: options sync failed: {e}"));
            }
            _ => {}
        }
    }

    if !sync_report.downloaded.is_empty() {
        progress.log(&format!(
            "# Downloaded {} missing mod file(s): {}",
            sync_report.downloaded.len(),
            sync_report.downloaded.join(", ")
        ));
    }
    if !sync_report.failed.is_empty() {
        for failure in &sync_report.failed {
            progress.log(&format!(
                "# WARNING: failed to prepare mod '{}': {}",
                failure.mod_id, failure.error
            ));
        }
        let preview: Vec<String> = sync_report
            .failed
            .iter()
            .take(5)
            .map(|f| format!("{} ({})", f.mod_id, f.error))
            .collect();
        return Err(LaunchErrorInfo::new(
            LaunchErrorKind::ModDownload,
            format!(
                "Could not download {} missing mod file(s) before launch: {}. Fix network / Modrinth access, then Retry.",
                sync_report.failed.len(),
                preview.join("; ")
            ),
        )
        .with_log(&console_log));
    }

    emit_launch_progress(&app, "install", "Installing Minecraft…", Some(55));
    emit_launch_phase_simple(&app, &path, "starting", "Installing Minecraft & starting JVM…");
    progress.log("# Installing Minecraft (this may take a while)...");

    let mut launch_jvm_args = project_profile.jvm_args.clone();
    launch_jvm_args.extend(launcher_settings::split_custom_jvm_args(
        launch_settings.java_custom_args.as_deref(),
    ));
    // Auto-tune (jvm_tuning): loader/version-aware GC profile plus a
    // category-aware heap estimate. Profile/user JVM args always win via
    // GC-aware dedupe; the heap itself stays on the memory setting.
    let loader_slug = tuffbox_core::graph::loader_kind_slug(&manifest.loader.kind);
    let mod_categories: Vec<Vec<String>> = manifest
        .mods
        .iter()
        .filter(|m| {
            matches!(
                m.content_type,
                tuffbox_core::manifest::ContentType::Mod
            )
        })
        .map(|m| m.source.categories.clone())
        .collect();
    let auto_total_ram_mb = {
        let mut sys = sysinfo::System::new();
        sys.refresh_memory();
        sys.total_memory() / 1024 / 1024
    };
    let heap_rec = tuffbox_core::jvm_tuning::recommend_heap_mb(
        auto_total_ram_mb,
        loader_slug,
        &mod_categories,
    );
    let heavy_pack = heap_rec.category_mb >= 2048 || heap_rec.mod_count >= 100;
    let jvm_rec = tuffbox_core::jvm_tuning::recommend_jvm_args(
        loader_slug,
        &manifest.minecraft.version,
        java.major,
        heavy_pack,
    );
    for arg in &jvm_rec.args {
        launcher_settings::append_tuned_jvm_arg(&mut launch_jvm_args, arg.clone());
    }
    progress.log(&format!(
        "# JVM auto-tune ({}): {}",
        jvm_rec.profile, jvm_rec.note
    ));
    progress.log(&format!(
        "# Heap estimate: {} MB for {} mods (base {} + categories {})",
        heap_rec.memory_mb, heap_rec.mod_count, heap_rec.base_mb, heap_rec.category_mb
    ));
    launcher_settings::append_stability_jvm_args(
        &mut launch_jvm_args,
        launch_settings.potato_pc,
    );
    if launch_settings.potato_pc {
        progress.log("# Potato PC: using lighter JVM GC / thread defaults.");
    }
    let mut cleanup_paths = Vec::new();
    let mut overlay_env: Option<(String, String)> = None;

    if !skip_client_bridges {
        let bridge = match tuffbox_core::prepare_recipe_bridge(&manifest, &game_dir) {
            Ok(bridge) => bridge,
            Err(error) => {
                progress.log(&format!("# WARNING: JEI live recipe bridge unavailable: {error}"));
                None
            }
        };
        if let Some(bridge) = bridge {
            progress.log("# JEI live recipe bridge enabled.");
            launch_jvm_args.extend(bridge.jvm_args);
            cleanup_paths.extend(bridge.cleanup_paths);
        }
    }

    // Try to load real MC access token / identity from stored auth
    let identity = auth::load_active_launch_identity();
    let (mc_token, auth_uuid, auth_user_type, auth_name) = match &identity {
        Some((uuid, name, token, user_type, _authority)) => (
            Some(token.as_str()),
            Some(uuid.as_str()),
            Some(user_type.as_str()),
            Some(name.as_str()),
        ),
        None => (None, None, None, None),
    };

    // Cosmetics stack (CSL + tuffbox-cosmetics) — after identity so we know username
    if !skip_client_bridges {
        let uname = auth_name.unwrap_or("Player");
        let uid = auth_uuid.unwrap_or("offline");
        let extras = cosmetics_local::merge_gui_extras(
            &game_dir,
            cosmetics_local::active_extras(uid),
        );
        match tuffbox_core::prepare_cosmetics_bridge(&manifest, &game_dir, uname, uid, extras) {
            Ok(Some(cos)) => {
                progress.log(&format!("# Appearance: {}", cos.message));
                cleanup_paths.extend(cos.cleanup_paths);
            }
            Ok(None) => {}
            Err(error) => {
                progress.log(&format!("# WARNING: cosmetics inject: {error}"));
            }
        }

        // In-game overlay — session for GL hook IPC (+ optional legacy JVM jar).
        if launcher_settings::overlay_enabled() {
            let overlay_secret = cosmetics_local::active_extras(uid).write_secret;
            match tuffbox_core::prepare_overlay_bridge(
                &manifest,
                &game_dir,
                uname,
                uid,
                &overlay_secret,
            ) {
                Ok(Some(ov)) => {
                    progress.log(&format!("# Overlay: {}", ov.message));
                    cleanup_paths.extend(ov.cleanup_paths);
                    match overlay_hook::ensure_ipc_server(&ov.session_path) {
                        Ok(ep) => {
                            progress.log(&format!("# Overlay IPC: {ep}"));
                            overlay_env = Some((ep, ov.session_path.display().to_string()));
                        }
                        Err(e) => progress.log(&format!("# WARNING: overlay IPC: {e}")),
                    }
                }
                Ok(None) => {}
                Err(error) => {
                    progress.log(&format!("# WARNING: overlay session: {error}"));
                }
            }
        }
    }

    // authlib-injector for Yggdrasil accounts
    if let Some((_, _, _, _, Some(authority))) = &identity {
        if let Ok(agent) = ensure_authlib_injector_agent(authority) {
            launch_jvm_args.push(agent);
            progress.log("# authlib-injector enabled for third-party auth.");
        }
    }

    let options = LaunchOptions {
        profile_id: profile.clone(),
        instance_dir: game_dir.clone(),
        memory_mb: launcher_settings::resolve_launch_memory_mb(
            project_profile.memory_mb,
            &launch_settings,
            memory_mb_override,
        ),
        jvm_args: launch_jvm_args,
        quick_play_type,
        quick_play_value,
    };

    let (mut cmd, _) = TestLauncher::build_command(
        &manifest,
        &project_profile,
        &options,
        &java,
        &launcher_dir,
        &progress,
        mc_token,
        auth_uuid,
        auth_user_type,
        auth_name,
    )
    .map_err(|e| {
        let msg = e.to_string();
        let kind = tuffbox_core::launch_error::classify_build_error_kind(&msg);
        LaunchErrorInfo::new(kind, msg).with_log(&console_log)
    })?;

    if let Some(res) = &launch_settings.game_resolution {
        cmd.arg("--width").arg(res.width.to_string());
        cmd.arg("--height").arg(res.height.to_string());
    }

    let mut cmd = launcher_settings::wrap_java_command(cmd, launch_settings.wrapper_command.as_deref());
    if let Some((ep, session)) = &overlay_env {
        cmd.env("TUFFBOX_OVERLAY_IPC", ep);
        cmd.env("TUFFBOX_OVERLAY_SESSION", session);
    }

    // GPU preference (discrete by default on hybrid systems) — best effort, never blocks the game.
    match gpu::detect_gpus() {
        Ok(gpus) => {
            if let Some(target) = gpu::resolve_target_gpu(&launch_settings.gpu_preference, &gpus) {
                progress.log(&format!(
                    "# GPU: {} ({}, {})",
                    target.name, target.kind, target.vendor
                ));
                #[cfg(target_os = "linux")]
                for (key, value) in gpu::linux_prime_env(target, gpus.len()) {
                    cmd.env(&key, &value);
                }
                #[cfg(target_os = "windows")]
                if let Some(value) = gpu::windows_gpu_preference_value(&target.kind) {
                    if let Err(e) = gpu::apply_windows_gpu_preference(&java.path, value) {
                        progress.log(&format!("# WARNING: GPU preference: {e}"));
                    }
                }
            } else {
                progress.log("# GPU: no adapters detected \u{2014} OS default renderer");
            }
        }
        Err(e) => progress.log(&format!("# WARNING: GPU detection: {e}")),
    }

    emit_launch_progress(&app, "starting", "Starting…", Some(95));
    progress.log("# Starting Java process...");
    emit_launch_phase(
        &app,
        &path,
        &profile,
        LaunchPhase::Starting,
        "Starting the Java process…",
        Some(&console_log),
        None,
        None,
        None,
        false,
        None,
    );

    // Crash callback + playtime + Discord presence cleanup
    let crash_ctx = CrashExitCtx {
        log_path: latest_log.clone(),
        mc_version: manifest.minecraft.version.clone(),
        java_version: java.version.clone(),
        loader_kind: tuffbox_core::graph::loader_kind_slug(&manifest.loader.kind).to_string(),
        loader_version: manifest.loader.version.clone(),
        game_dir: game_dir.clone(),
    };
    let app_for_exit = app.clone();
    let stats_path_for_exit = path.clone();
    let profile_for_exit = profile.clone();
    let latest_log_for_exit = latest_log.clone();
    let post_exit_hook = launch_settings.post_exit_hook.clone();
    let instance_label = manifest.project.name.clone();
    // Options write-back context: push this session's local settings edits
    // into the shared version-group template after the game closes.
    let options_game_dir = game_dir.clone();
    let options_mc_version = manifest.minecraft.version.clone();
    let _ = presence::set_playing_activity(&instance_label, "In Minecraft");
    let _ = record_launch(path.clone());
    launcher_presence::spawn_game_session_start(instance_label.clone());
    let on_exit: Option<OnExit> = Some(Box::new(move |exit: ProcessExit| {
        let _ = presence::clear_activity();
        tuffbox_core::options_sync::write_back_after_exit(&options_game_dir, &options_mc_version);
        launcher_presence::spawn_game_session_end(
            exit.duration_secs,
            !exit.stop_requested && exit.code != Some(0),
        );
        if let Some(ref hook) = post_exit_hook {
            let _ = launcher_settings::run_hook(Some(hook), "post-exit hook");
        }
        // Accumulate playtime for every session (including crashes / manual stops).
        if let Ok(project_dir) = manifest_parent(&stats_path_for_exit) {
            let mut stats = load_stats(&project_dir);
            let entry = stats
                .instances
                .entry(profile_for_exit.clone())
                .or_default();
            entry.total_playtime_seconds = entry
                .total_playtime_seconds
                .saturating_add(exit.duration_secs);
            let _ = save_stats(&project_dir, &stats);
        }

        overlay_hook::stop_ipc_server();
        let crash_info = if exit.stop_requested || exit.code == Some(0) {
            None
        } else {
            Some(classify_crash(&crash_ctx, exit.code))
        };
        let exit_message = if exit.stop_requested {
            "Game stopped."
        } else if exit.code == Some(0) {
            "Game exited normally."
        } else {
            "Game exited unexpectedly."
        };
        emit_launch_phase(
            &app_for_exit,
            &stats_path_for_exit,
            &profile_for_exit,
            LaunchPhase::Exited,
            exit_message,
            Some(&latest_log_for_exit),
            None,
            Some(exit.started_at),
            exit.code,
            exit.stop_requested,
            crash_info.clone(),
        );
        let _ = app_for_exit.emit(
            "process-exited",
            ProcessExitedEvent {
                id: stats_path_for_exit.clone(),
                profile: profile_for_exit.clone(),
                started_at: exit.started_at,
                code: exit.code,
                stopped: exit.stop_requested,
            },
        );
        emit_launch_phase_simple(
            &app_for_exit,
            &stats_path_for_exit,
            "exited",
            if exit.code == Some(0) {
                "Game exited"
            } else {
                "Game exited (crashed)"
            },
        );
        overlay_hook::stop_ipc_server();
        if exit.code == Some(0) {
            if let Ok(project_dir) = manifest_parent(&stats_path_for_exit) {
                let _ = tuffbox_core::launch_history::record_launch_exit(
                    &project_dir,
                    exit.code,
                    exit.duration_secs,
                    None,
                );
            }
        }

        let Some(info) = crash_info else {
            return;
        };
        let _ = record_crash(stats_path_for_exit.clone());
        // Start / continue a History episode for this crash.
        if let Ok(project_dir) = manifest_parent(&stats_path_for_exit) {
            let log_for_fp = info
                .log_path
                .as_deref()
                .map(PathBuf::from)
                .filter(|path| path.is_file())
                .unwrap_or_else(|| crash_ctx.log_path.clone());
            let log_text = tuffbox_core::process::read_log_tail(&log_for_fp, 1200)
                .unwrap_or_default();
            let fingerprint = tuffbox_core::crash_kb::fingerprint_from_text(
                &log_text,
                &crash_ctx.mc_version,
                &crash_ctx.loader_kind,
            );
            let crash_report_abs = info
                .log_path
                .as_deref()
                .map(PathBuf::from)
                .filter(|p| p.is_file());
            let _ = tuffbox_core::launch_history::archive_crashed_session(
                &project_dir,
                exit.code,
                exit.duration_secs,
                Some(fingerprint.key.clone()),
                crash_report_abs.as_deref(),
            );
            let _ = pack_events::append_crash_detected(
                &project_dir,
                &fingerprint.key,
                exit.code,
                info.log_path.as_deref(),
                &info.message,
            );
            let _ = swarm_api::ensure_open_crash_episode_marker(
                &project_dir,
                &fingerprint.key,
                None,
            );
        }
        let _ = app_for_exit.emit(
            "launch-crashed",
            LaunchCrashEvent {
                id: stats_path_for_exit.clone(),
                path: stats_path_for_exit.clone(),
                profile: profile_for_exit.clone(),
                legacy_error: info.clone(),
                error: info,
                exit_code: exit.code,
            },
        );
    }));

    // Tee JVM stdout/stderr to TuffBox console log; Minecraft owns logs/latest.log.
    let running = tuffbox_core::process::spawn_and_track_with_cleanup(
        path.clone(),
        profile,
        cmd,
        &console_log,
        cleanup_paths,
        on_exit,
        show_console,
    )
    .map_err(|e| {
        let msg = e.to_string();
        let kind = tuffbox_core::launch_error::classify_build_error_kind(&msg);
        LaunchErrorInfo::new(kind, msg).with_log(&console_log)
    })?;

    // Give Java the scheduler budget while it is running. On Windows this
    // enters background CPU/I/O mode for TuffBox only and restores the exact
    // previous priority after this Java PID exits. Java itself is not lowered.
    match resource_mode::enter_for_game(running.pid) {
        Ok(()) => progress.log("# Launcher resource mode: background until Minecraft exits"),
        Err(e) => progress.log(&format!("# WARNING: launcher resource mode: {e}")),
    }

    // CPU affinity (performance-core pinning) — best effort, never blocks the game.
    let affinity_cfg = cpu_affinity::AffinityConfig {
        mode: launch_settings.cpu_affinity_mode.clone(),
        mask_raw: launch_settings.cpu_affinity_mask.clone(),
    };
    if let Some(mask) = cpu_affinity::resolve_target_mask(
        &affinity_cfg,
        cpu_affinity::detect_core_topology().ok().as_ref(),
    ) {
        match cpu_affinity::apply_mask_to_pid(running.pid, mask) {
            Ok(()) => progress.log(&format!(
                "# CPU affinity: pinned to 0x{mask:X} ({} logical cores)",
                mask.count_ones()
            )),
            Err(e) => progress.log(&format!("# WARNING: CPU affinity: {e}")),
        }
    }

    let _ = app.emit(
        "process-started",
        serde_json::json!({
            "id": running.id.clone(),
            "pid": running.pid,
            "profile": running.profile_id.clone(),
            "startedAt": running.started_at,
        }),
    );
    emit_launch_phase(
        &app,
        &running.id,
        &running.profile_id,
        LaunchPhase::Running,
        "Game is running.",
        Some(&console_log),
        Some(running.pid),
        Some(running.started_at),
        None,
        false,
        None,
    );

    if overlay_env.is_some() {
        let pid = running.pid;
        std::thread::spawn(move || {
            // Wait until opengl32 is likely loaded.
            std::thread::sleep(std::time::Duration::from_secs(8));
            match overlay_hook::inject_hook_dll(pid) {
                Ok(msg) => eprintln!("[overlay-hook] {msg}"),
                Err(e) => eprintln!("[overlay-hook] inject failed: {e}"),
            }
        });
    }

    Ok(running)
}

fn ensure_authlib_injector_agent(authority: &str) -> Result<String, String> {
    let dir = dirs::config_dir()
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("TuffBox")
        .join("authlib");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let jar = dir.join("authlib-injector.jar");
    if !jar.is_file() {
        // Pin a known release so launches stay reproducible offline after first fetch.
        let url = "https://github.com/yushijinhun/authlib-injector/releases/download/v1.2.5/authlib-injector-1.2.5.jar";
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(|e| e.to_string())?;
        let bytes = client
            .get(url)
            .send()
            .map_err(|e| format!("authlib-injector download failed: {e}"))?
            .bytes()
            .map_err(|e| e.to_string())?;
        std::fs::write(&jar, &bytes).map_err(|e| e.to_string())?;
    }
    Ok(format!(
        "-javaagent:{}={}",
        jar.to_string_lossy().replace('\\', "/"),
        authority.trim_end_matches('/')
    ))
}

#[tauri::command(rename_all = "camelCase")]
fn get_presence_settings() -> Result<presence::PresenceSettings, String> {
    Ok(presence::load_presence_settings())
}

#[tauri::command(rename_all = "camelCase")]
fn save_presence_settings(settings: presence::PresenceSettings) -> Result<(), String> {
    presence::save_presence_settings(&settings)
}

#[tauri::command(rename_all = "camelCase")]
fn set_discord_presence(details: String, state: String) -> Result<(), String> {
    presence::set_playing_activity(&details, &state)
}

#[tauri::command(rename_all = "camelCase")]
fn clear_discord_presence() -> Result<(), String> {
    presence::clear_activity()
}

/// Read the installed mod JAR names from a game directory (best-effort).
fn read_installed_mods(game_dir: &PathBuf) -> Vec<String> {
    std::fs::read_dir(game_dir.join("mods"))
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|name| name.ends_with(".jar"))
                .collect()
        })
        .unwrap_or_default()
}

/// Run the crash-analysis engine over the launch log and produce a
/// user-facing, categorized launch error the UI can surface with a Retry
/// action. The classification logic lives in `tuffbox_core` so it stays
/// unit-testable without linking the Tauri runtime.
fn classify_crash(ctx: &CrashExitCtx, exit_code: Option<i32>) -> LaunchErrorInfo {
    let installed_mods = read_installed_mods(&ctx.game_dir);
    // Prefer Minecraft's own latest.log; fall back to our console capture if
    // log4j never wrote anything (very early JVM death).
    let mut log_path = ctx.log_path.clone();
    let usable = log_path.is_file()
        && std::fs::metadata(&log_path)
            .map(|m| m.len() > 32)
            .unwrap_or(false);
    if !usable {
        if let Some(parent) = ctx.log_path.parent() {
            let console = parent.join("tuffbox-console.log");
            if console.is_file() {
                log_path = console;
            }
        }
    }
    tuffbox_core::crash_assistant::classify_launch_crash(
        &log_path,
        exit_code,
        &ctx.mc_version,
        &ctx.java_version,
        &ctx.loader_kind,
        &ctx.loader_version,
        &installed_mods,
    )
}

#[tauri::command]
fn import_curseforge_project(source: String, target_dir: String) -> Result<String, String> {
    let mut manifest = tuffbox_core::import_curseforge_pack(&source).map_err(|e| e.to_string())?;
    let _ =
        tuffbox_core::resolve_curseforge_pack_files(&mut manifest).map_err(|e| e.to_string())?;
    let target = PathBuf::from(&target_dir);
    std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    let overrides_folder =
        tuffbox_core::curseforge_overrides_folder(&source).unwrap_or_else(|_| "overrides".into());
    let _ = tuffbox_core::extract_curseforge_overrides(&source, &target, &overrides_folder);
    let _ = tuffbox_core::stash_curseforge_manifest(&source, &target);
    let manifest_path = target.join(format!("{}.tuffbox.json", manifest.project.id));
    let json = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    std::fs::write(&manifest_path, json).map_err(|e| e.to_string())?;
    let out = manifest_path.to_string_lossy().to_string();
    swarm_api::spawn_pack_cooccurrence(out.clone(), "pack_import");
    Ok(out)
}

#[tauri::command(rename_all = "camelCase")]
fn import_project(source: String, target_dir: String) -> Result<String, String> {
    use tuffbox_core::{
        import_curseforge_pack, import_instance_directory, import_modrinth_pack,
        import_prism_instance, is_curseforge_pack, resolve_curseforge_pack_files,
    };

    let path = PathBuf::from(&source);
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let (mut manifest, is_cf) = if path.is_dir() {
        let (m, game_dir) = import_instance_directory(&source).map_err(|e| e.to_string())?;
        let target_root = PathBuf::from(&target_dir).join(&m.project.id);
        std::fs::create_dir_all(&target_root).map_err(|e| e.to_string())?;
        // Inline copy of game content (same folders as install_modpack).
        for entry_name in [
            "mods",
            "config",
            "defaultconfigs",
            "kubejs",
            "scripts",
            "resourcepacks",
            "shaderpacks",
            "datapacks",
            "options.txt",
            "optionsof.txt",
            "servers.dat",
        ] {
            let src = game_dir.join(entry_name);
            if src.is_dir() {
                copy_dir_recursive(&src, &target_root.join(entry_name)).map_err(|e| e.to_string())?;
            } else if src.is_file() {
                std::fs::copy(&src, target_root.join(entry_name)).map_err(|e| e.to_string())?;
            }
        }
        let target = target_root.join(format!("{}.tuffbox.json", m.project.id));
        let json = serde_json::to_string_pretty(&m).map_err(|e| e.to_string())?;
        std::fs::write(&target, json).map_err(|e| e.to_string())?;
        let out = target.to_string_lossy().to_string();
        swarm_api::spawn_pack_cooccurrence(out.clone(), "pack_import");
        return Ok(out);
    } else {
        match ext.as_str() {
            "mrpack" => (
                import_modrinth_pack(&source).map_err(|e| e.to_string())?,
                false,
            ),
            "zip" if is_curseforge_pack(&source) => (
                import_curseforge_pack(&source).map_err(|e| e.to_string())?,
                true,
            ),
            "zip" => (
                import_prism_instance(&source).map_err(|e| e.to_string())?,
                false,
            ),
            _ => return Err(format!("unsupported import format: {ext}")),
        }
    };

    if is_cf {
        let _ = resolve_curseforge_pack_files(&mut manifest).map_err(|e| e.to_string())?;
    }

    let target_root = PathBuf::from(&target_dir).join(&manifest.project.id);
    std::fs::create_dir_all(&target_root).map_err(|e| e.to_string())?;
    if is_cf {
        let overrides_folder = tuffbox_core::curseforge_overrides_folder(&source)
            .unwrap_or_else(|_| "overrides".into());
        let _ =
            tuffbox_core::extract_curseforge_overrides(&source, &target_root, &overrides_folder);
        let _ = tuffbox_core::stash_curseforge_manifest(&source, &target_root);
    }

    let target = target_root.join(format!("{}.tuffbox.json", manifest.project.id));
    let json = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    std::fs::write(&target, json).map_err(|e| e.to_string())?;
    let out = target.to_string_lossy().to_string();
    swarm_api::spawn_pack_cooccurrence(out.clone(), "pack_import");
    Ok(out)
}

/// Search CurseForge modpacks (classId 4471), Prism FlamePage style.
#[tauri::command(rename_all = "camelCase")]
async fn search_curseforge_modpacks(
    query: String,
    game_version: Option<String>,
    offset: Option<u32>,
) -> Result<Vec<serde_json::Value>, String> {
    tokio::task::spawn_blocking(move || {
        let provider = tuffbox_core::CurseForgeProvider::new();
        if !provider.is_configured() {
            return Err("CurseForge API key is not configured".to_string());
        }
        let hits = provider
            .search_modpacks(&query, game_version.as_deref(), offset.unwrap_or(0), 30)
            .map_err(|e| e.to_string())?
            .hits;
        Ok(hits
            .into_iter()
            .map(|h| {
                serde_json::json!({
                    "id": h.id,
                    "slug": h.slug,
                    "name": h.name,
                    "summary": h.summary,
                    "downloadCount": h.download_count,
                    "iconUrl": h.icon_url,
                    "authors": h.authors,
                    "categories": h.categories,
                })
            })
            .collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// List files for a CurseForge modpack project.
#[tauri::command(rename_all = "camelCase")]
async fn get_curseforge_modpack_files(
    mod_id: u64,
    game_version: Option<String>,
) -> Result<Vec<serde_json::Value>, String> {
    tokio::task::spawn_blocking(move || {
        let provider = tuffbox_core::CurseForgeProvider::new();
        let mut files = provider
            .get_mod_files(mod_id, game_version.as_deref())
            .map_err(|e| e.to_string())?;
        // Newest first so Discover "Add" picks a current pack version.
        files.sort_by(|a, b| b.file_date.cmp(&a.file_date).then_with(|| b.id.cmp(&a.id)));
        Ok(files
            .into_iter()
            .map(|f| {
                serde_json::json!({
                    "id": f.id,
                    "modId": f.mod_id,
                    "displayName": f.display_name,
                    "fileName": f.file_name,
                    "downloadUrl": f.resolved_download_url(),
                    "releaseType": f.release_type,
                    "gameVersions": f.game_versions,
                    "fileDate": f.file_date,
                    "blocked": f.blocked && f.resolved_download_url().is_none(),
                })
            })
            .collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Download a CurseForge / Modrinth / local pack and create an instance with
/// resolved mods + download progress (Prism InstanceImportTask flow).
///
/// Apply the user's import-time dedup choice to the freshly created pack:
/// - `Some(true)`  → single-root retro-dedup: identical jars/zips already in
///   the shared store (e.g. from another imported Prism pack) are linked in,
///   new unique files are recorded — the pack immediately shares bytes.
/// - `Some(false)` → `.tuffbox-no-dedup` marker: the pack keeps independent
///   files (installs never consult or feed the store).
/// - `None`        → no decision (legacy callers): store behavior unchanged.
pub(crate) fn apply_import_dedup_choice(
    instance_dir: &Path,
    dedup: Option<bool>,
) -> serde_json::Value {
    match dedup {
        Some(true) => {
            let report = tuffbox_core::mod_store::retro_dedup(&[instance_dir]);
            serde_json::json!({
                "mode": "shared",
                "linked": report.linked,
                "recorded": report.recorded,
                "bytesReclaimed": report.bytes_reclaimed,
                "errors": report.errors.len(),
            })
        }
        Some(false) => {
            let marker = instance_dir.join(tuffbox_core::mod_store::NO_DEDUP_MARKER);
            let _ = std::fs::write(&marker, b"");
            serde_json::json!({ "mode": "independent" })
        }
        None => serde_json::json!({ "mode": "default" }),
    }
}

/// Also accepts launcher instance folders (Prism / MultiMC / CurseForge /
/// plain `mods/`) and mods-only zip archives.
#[tauri::command(rename_all = "camelCase")]
async fn install_modpack(
    app: tauri::AppHandle,
    source: String,
    target_dir: String,
    instance_name: Option<String>,
    dedup: Option<bool>,
) -> Result<serde_json::Value, String> {
    if !std::path::Path::new(&source).exists()
        && tuffbox_core::github_pack::parse_github_source(&source).is_ok()
    {
        return crate::github_pack_commands::github_pack_install(
            app,
            source,
            target_dir,
            instance_name,
            dedup,
        )
        .await;
    }
    tokio::task::spawn_blocking(move || {
        use tauri::Emitter;
        use tuffbox_core::{
            curseforge_overrides_folder, extract_curseforge_overrides, import_curseforge_pack,
            import_instance_directory, import_modrinth_pack, import_prism_instance,
            is_curseforge_pack, is_mods_only_zip, resolve_curseforge_pack_files,
            resolve_instance_game_dir, stash_curseforge_manifest, CurseForgeProvider,
        };

        let _ = app.emit(
            "modpack-install-progress",
            serde_json::json!({ "phase": "resolving", "message": "Preparing modpack…" }),
        );
        let import_dedup = dedup;
        let task_id = tuffbox_core::task_progress::start_task(
            format!("modpack-{}", tuffbox_core::time_util::compact_now()),
            "Install modpack",
        );
        tuffbox_core::task_progress::set_progress(&task_id, 0.05, Some("Preparing…".into()));

        let source_path = PathBuf::from(&source);

        // ── Instance / plain folder import ──────────────────────────
        if source_path.is_dir() {
            // Content-only drop (resourcepacks/ shaderpacks/ config/ …):
            // not an instance by itself — stage a copy with the expected
            // game-dir layout, then run the normal folder flow on it.
            // The guard cleans the stage up on every exit path below.
            let mut _stage_guard = StageDirCleanup(None);
            let source_path = match stage_content_only_dir(&source_path) {
                Some(staged) => {
                    _stage_guard.0 = Some(staged.clone());
                    staged
                }
                None => source_path,
            };
            let (mut manifest, game_dir) =
                import_instance_directory(&source_path).map_err(|e| e.to_string())?;
            if let Some(name) = instance_name.filter(|n| !n.trim().is_empty()) {
                manifest.project.name = name.clone();
                manifest.project.id = slugify_project_name(&name);
            }
            let instance_dir = PathBuf::from(&target_dir).join(&manifest.project.id);
            std::fs::create_dir_all(&instance_dir).map_err(|e| e.to_string())?;
            copy_instance_game_content(&game_dir, &instance_dir)?;
            let manifest_path = instance_dir.join(format!("{}.tuffbox.json", manifest.project.id));
            let json = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
            std::fs::write(&manifest_path, &json).map_err(|e| e.to_string())?;
            let _ = app.emit(
                "modpack-install-progress",
                serde_json::json!({
                    "phase": "done",
                    "message": format!("Imported {} mods from folder", manifest.mods.len()),
                }),
            );
            tuffbox_core::task_progress::succeed(
                &task_id,
                Some(format!("{} mods", manifest.mods.len())),
            );
            swarm_api::spawn_pack_cooccurrence(
                manifest_path.to_string_lossy().to_string(),
                "pack_import",
            );
            let dedup_info = apply_import_dedup_choice(&instance_dir, import_dedup);
            return Ok(serde_json::json!({
                "path": manifest_path.to_string_lossy(),
                "name": manifest.project.name,
                "modCount": manifest.mods.len(),
                "provider": "folder",
                "dedup": dedup_info,
            }));
        }

        // Remote CF file: source is "cf:<modId>:<fileId>" or a direct URL.
        let pack_path = if let Some(rest) = source.strip_prefix("cf:") {
            let parts: Vec<&str> = rest.split(':').collect();
            if parts.len() != 2 {
                return Err("expected cf:<modId>:<fileId>".into());
            }
            let mod_id: u64 = parts[0].parse().map_err(|_| "invalid mod id")?;
            let file_id: u64 = parts[1].parse().map_err(|_| "invalid file id")?;
            let provider = CurseForgeProvider::new();
            let file = provider.get_file(mod_id, file_id).map_err(|e| e.to_string())?;
            let urls = file.resolved_download_urls();
            if urls.is_empty() {
                return Err(format!(
                    "CurseForge returned no download URL for {} (file {}). Try importing the zip manually from CurseForge.",
                    file.file_name, file_id
                ));
            }
            let _ = app.emit(
                "modpack-install-progress",
                serde_json::json!({ "phase": "downloading-pack", "message": format!("Downloading {}", file.file_name) }),
            );
            let tmp = std::env::temp_dir().join(format!("tuffbox-pack-{}-{}.zip", mod_id, file_id));
            tuffbox_core::provider::curseforge::download_curseforge_url_candidates(
                &urls,
                &tmp,
                file.hashes.sha1.as_deref(),
            )
            .map_err(|e| format!("pack download failed: {e}"))?;
            tmp
        } else if source.starts_with("http://") || source.starts_with("https://") {
            let _ = app.emit(
                "modpack-install-progress",
                serde_json::json!({ "phase": "downloading-pack", "message": "Downloading pack…" }),
            );
            let tmp = std::env::temp_dir().join(format!(
                "tuffbox-pack-{}.zip",
                tuffbox_core::time_util::compact_now()
            ));
            // download_with_sha1 trusts a pre-existing dest when no checksum is
            // known; a stale/interrupted file would then fail deep inside the
            // importer with an opaque zip error. Verify the archive actually
            // opens and contains a pack marker, re-download once fresh if not.
            let mut last_err: Option<String> = None;
            for attempt in 0..2 {
                if tmp.exists() {
                    let _ = std::fs::remove_file(&tmp);
                }
                remove_part_file(&tmp);
                if attempt == 1 {
                    let _ = app.emit(
                        "modpack-install-progress",
                        serde_json::json!({ "phase": "downloading-pack", "message": "Retrying pack download…" }),
                    );
                }
                match tuffbox_core::download_with_sha1(&source, &tmp, None) {
                    Ok(()) => {}
                    Err(e) => {
                        last_err = Some(format!("pack download failed: {e}"));
                        continue;
                    }
                }
                if !tmp.is_file() {
                    last_err = Some("pack download produced no file".into());
                    continue;
                }
                match verify_pack_archive(&tmp) {
                    Ok(()) => {
                        last_err = None;
                        break;
                    }
                    Err(e) => {
                        last_err = Some(format!(
                            "downloaded pack is not a valid modpack archive ({e}); retrying"
                        ));
                    }
                }
            }
            if let Some(e) = last_err {
                let _ = std::fs::remove_file(&tmp);
                return Err(e);
            }
            tmp
        } else {
            PathBuf::from(&source)
        };

        // Dropped .rar / .7z packs: pre-scan for unsafe entries, extract to
        // a private temp dir and re-pack as zip so the sniffing below (and
        // every importer) works on a format it already understands.
        let pack_path = normalize_foreign_pack_archive(&pack_path)?;

        if !pack_path.is_file() {
            return Err(format!("pack not found: {}", pack_path.display()));
        }

        // Player-built VanillaTweaks texture packs from vanillatweaks.net are not
        // modpacks — steer the user to the Resourcepacks import path.
        if let Some(name) = pack_path.file_name().and_then(|n| n.to_str()) {
            if matches!(
                tuffbox_core::manifest::ContentType::from_filename(name),
                tuffbox_core::manifest::ContentType::Resourcepack
            ) && name.to_ascii_lowercase().starts_with("vanillatweaks_")
            {
                return Err(
                    "VanillaTweaks_*.zip is a custom resource pack from vanillatweaks.net, not a modpack. Open the instance → Resourcepacks → Import…."
                        .into(),
                );
            }
        }

        // Content-sniff the archive type instead of trusting the file
        // extension: remote downloads land in a temp file named `*.zip`
        // regardless of their real format, so a Modrinth .mrpack fetched from
        // Discover used to be misrouted to the Prism importer, which then
        // failed with "archive error: specified file not found in archive"
        // (it looks for instance.cfg, which .mrpack archives don't have).
        let is_cf = is_curseforge_pack(&pack_path);
        // WinRAR-style single-root wrappers ("MyPack/modrinth.index.json")
        // are re-rooted before sniffing so wrapped packs are not misrouted
        // to the Prism importer (or to a "not a modpack archive" error).
        let zip_root = zip_wrap_prefix_of(&pack_path).unwrap_or_default();
        let has_mrpack_index = zip_has_entry(&pack_path, &format!("{zip_root}modrinth.index.json"));
        let has_prism_cfg = zip_has_entry(&pack_path, &format!("{zip_root}instance.cfg"));
        let ext = pack_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        let effective_ext = if has_mrpack_index {
            "mrpack".to_string()
        } else if has_prism_cfg {
            "zip".to_string()
        } else {
            ext.clone()
        };
        let is_mods_zip = effective_ext == "zip" && !is_cf && is_mods_only_zip(&pack_path);
        // Content zips carry resourcepacks/shaderpacks/config instead of a
        // pack manifest — previously they fell through to the Prism importer
        // and failed with "instance.cfg not found".
        let is_content_zip =
            effective_ext == "zip" && !is_cf && !has_mrpack_index && !has_prism_cfg && !is_mods_zip
                && is_content_zip_archive(&pack_path);
        let is_prism_zip = effective_ext == "zip"
            && !is_cf
            && !has_mrpack_index
            && !is_mods_zip
            && !is_content_zip;
        let mut manifest = match effective_ext.as_str() {
            "mrpack" => import_modrinth_pack(&pack_path).map_err(|e| e.to_string())?,
            "zip" if is_cf => import_curseforge_pack(&pack_path).map_err(|e| e.to_string())?,
            "zip" if is_mods_zip => {
                // Temporary extract → treat as folder with mods/.
                let tmp_root = std::env::temp_dir().join(format!(
                    "tuffbox-mods-zip-{}",
                    tuffbox_core::time_util::compact_now()
                ));
                std::fs::create_dir_all(tmp_root.join("mods")).map_err(|e| e.to_string())?;
                extract_mods_only_zip(&pack_path, &tmp_root.join("mods"))?;
                let (m, _) = import_instance_directory(&tmp_root).map_err(|e| e.to_string())?;
                let _ = std::fs::remove_dir_all(&tmp_root);
                m
            }
            "zip" if is_content_zip => {
                // Temporary extract of the whitelisted content dirs →
                // manifest from the staged game dir (mods/resourcepacks/
                // shaderpacks all counted), real extraction happens after
                // the instance dir exists.
                let tmp_root = std::env::temp_dir().join(format!(
                    "tuffbox-content-zip-{}",
                    tuffbox_core::time_util::compact_now()
                ));
                std::fs::create_dir_all(tmp_root.join("mods")).map_err(|e| e.to_string())?;
                extract_content_zip(&pack_path, &tmp_root)?;
                let (m, _) = import_instance_directory(&tmp_root).map_err(|e| e.to_string())?;
                let _ = std::fs::remove_dir_all(&tmp_root);
                m
            }
            "zip" => import_prism_instance(&pack_path).map_err(|e| e.to_string())?,
            _ => return Err(format!("unsupported pack format: .{ext}")),
        };

        if let Some(name) = instance_name.filter(|n| !n.trim().is_empty()) {
            manifest.project.name = name.clone();
            manifest.project.id = slugify_project_name(&name);
        }

        if is_cf {
            let _ = app.emit(
                "modpack-install-progress",
                serde_json::json!({ "phase": "resolving-files", "message": "Resolving CurseForge files…" }),
            );
            let resolved =
                resolve_curseforge_pack_files(&mut manifest).map_err(|e| e.to_string())?;
            let _ = app.emit(
                "modpack-install-progress",
                serde_json::json!({
                    "phase": "resolving-files",
                    "message": format!("Resolved {resolved} download URLs")
                }),
            );
        }

        let instance_dir = PathBuf::from(&target_dir).join(&manifest.project.id);
        std::fs::create_dir_all(&instance_dir).map_err(|e| e.to_string())?;

        if is_cf {
            let folder =
                curseforge_overrides_folder(&pack_path).unwrap_or_else(|_| "overrides".into());
            let n = extract_curseforge_overrides(&pack_path, &instance_dir, &folder)
                .map_err(|e| format!("failed to extract CurseForge overrides: {e}"))?;
            let _ = stash_curseforge_manifest(&pack_path, &instance_dir);
            let _ = app.emit(
                "modpack-install-progress",
                serde_json::json!({
                    "phase": "overrides",
                    "message": format!("Extracted {n} override files")
                }),
            );
        }

        if is_prism_zip {
            extract_prism_zip_content(&pack_path, &instance_dir)?;
            // Re-scan local jars so remote-less Prism exports still list mods.
            let game = resolve_instance_game_dir(&instance_dir);
            if game.join("mods").is_dir() {
                let (scanned, _) = import_instance_directory(&instance_dir).map_err(|e| e.to_string())?;
                if !scanned.mods.is_empty() {
                    manifest.mods = scanned.mods;
                    if manifest.minecraft.version.is_empty() {
                        manifest.minecraft.version = scanned.minecraft.version;
                    }
                    if manifest.loader.version.is_empty() {
                        manifest.loader = scanned.loader;
                    }
                }
            }
        }

        if is_mods_zip {
            // Re-extract into final instance (temp was cleaned).
            std::fs::create_dir_all(instance_dir.join("mods")).map_err(|e| e.to_string())?;
            extract_mods_only_zip(&pack_path, &instance_dir.join("mods"))?;
        }

        if is_content_zip {
            // Content packs: pull resourcepacks/shaderpacks/config/etc. into
            // the final instance and rescan so loader/version fill in the
            // same way the Prism zip path does.
            extract_content_zip(&pack_path, &instance_dir)?;
            let game = resolve_instance_game_dir(&instance_dir);
            if game.join("mods").is_dir() {
                let (scanned, _) =
                    import_instance_directory(&instance_dir).map_err(|e| e.to_string())?;
                if !scanned.mods.is_empty() {
                    manifest.mods = scanned.mods;
                }
                if manifest.minecraft.version.is_empty() {
                    manifest.minecraft.version = scanned.minecraft.version;
                }
                if manifest.loader.version.is_empty() {
                    manifest.loader = scanned.loader;
                }
            }
        }

        // Modrinth .mrpack bundles config/resourcepack/shader files under
        // overrides/ — they must land in the instance or the pack installs
        // without its configs. Mirrors the CurseForge overrides step above.
        if effective_ext == "mrpack" {
            let n = extract_mrpack_overrides(&pack_path, &instance_dir)
                .map_err(|e| format!("failed to extract Modrinth overrides: {e}"))?;
            if n > 0 {
                let _ = app.emit(
                    "modpack-install-progress",
                    serde_json::json!({
                        "phase": "overrides",
                        "message": format!("Extracted {n} override files")
                    }),
                );
            }
        }

        let manifest_path = instance_dir.join(format!("{}.tuffbox.json", manifest.project.id));
        let json = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
        std::fs::write(&manifest_path, &json).map_err(|e| e.to_string())?;

        let needs_download = manifest.mods.iter().any(|m| {
            m.source
                .url
                .as_ref()
                .map(|u| !u.is_empty())
                .unwrap_or(false)
        });

        let report = if needs_download {
            let _ = app.emit(
                "modpack-install-progress",
                serde_json::json!({
                    "phase": "downloading-mods",
                    "message": format!("Downloading {} content files…", manifest.mods.len())
                }),
            );
            download_project_mods_tracked(&app, &manifest_path, &manifest, None, true)
        } else {
            tuffbox_core::ModSyncReport {
                downloaded: vec![],
                already_present: manifest.mods.iter().map(|m| m.id.clone()).collect(),
                skipped: vec![],
                failed: vec![],
            }
        };

        let _ = app.emit(
            "modpack-install-progress",
            serde_json::json!({
                "phase": "done",
                "message": "Modpack installed",
                "failed": report.failed.len(),
            }),
        );
        tuffbox_core::task_progress::succeed(
            &task_id,
            Some(format!("{} mods", manifest.mods.len())),
        );
        swarm_api::spawn_pack_cooccurrence(
            manifest_path.to_string_lossy().to_string(),
            "pack_import",
        );
        let dedup_info = apply_import_dedup_choice(&instance_dir, import_dedup);

        Ok(serde_json::json!({
            "path": manifest_path.to_string_lossy(),
            "name": manifest.project.name,
            "modCount": manifest.mods.len(),
            "download": report,
            "dedup": dedup_info,
            "provider": if is_cf {
                "curseforge"
            } else if effective_ext == "mrpack" {
                "modrinth"
            } else if is_mods_zip {
                "mods-zip"
            } else {
                "prism"
            },
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

fn copy_instance_game_content(game_dir: &Path, instance_dir: &Path) -> Result<(), String> {
    for entry_name in [
        "mods",
        "config",
        "defaultconfigs",
        "kubejs",
        "scripts",
        "resourcepacks",
        "shaderpacks",
        "datapacks",
        "options.txt",
        "optionsof.txt",
        "servers.dat",
    ] {
        let src = game_dir.join(entry_name);
        if src.is_dir() {
            copy_dir_recursive(&src, &instance_dir.join(entry_name)).map_err(|e| e.to_string())?;
        } else if src.is_file() {
            std::fs::copy(&src, instance_dir.join(entry_name)).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn extract_mods_only_zip(zip_path: &Path, mods_dir: &Path) -> Result<(), String> {
    let file = std::fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(mods_dir).map_err(|e| e.to_string())?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().replace('\\', "/");
        let lower = name.to_lowercase();
        if !lower.ends_with(".jar") {
            continue;
        }
        let file_name = Path::new(&name)
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| format!("bad jar path in zip: {name}"))?;
        let dest = mods_dir.join(file_name);
        let mut out = std::fs::File::create(&dest).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Extracts a Modrinth .mrpack `overrides/` tree into the instance dir.
/// Returns the number of files written. Files already provided by the index
/// (mods/ jars that will be downloaded separately) are still extracted if
/// present — the index download skips existing non-empty files anyway.
fn extract_mrpack_overrides(pack_path: &Path, instance_dir: &Path) -> Result<usize, String> {
    let file = std::fs::File::open(pack_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let prefix = "overrides/";
    let mut count = 0usize;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().replace('\\', "/");
        if !name.starts_with(prefix) || name.ends_with('/') {
            continue;
        }
        let rel = &name[prefix.len()..];
        if rel.is_empty() || rel.contains("..") {
            continue;
        }
        let dest = instance_dir.join(rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut out = std::fs::File::create(&dest).map_err(|e| format!("write {rel}: {e}"))?;
        std::io::copy(&mut entry, &mut out).map_err(|e| format!("write {rel}: {e}"))?;
        count += 1;
    }
    Ok(count)
}

/// Opens the archive and checks it contains at least one known pack marker.
/// Guards against truncated/HTML-error-page downloads that would otherwise
/// fail deep inside the importer with an opaque zip error.
/// True when the zip contains the exact entry `name` (root level). Cheap
/// header scan; used to content-sniff pack format instead of trusting the
/// file extension (temp downloads are always named `*.zip`).
fn zip_has_entry(path: &Path, name: &str) -> bool {
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let Ok(mut archive) = zip::ZipArchive::new(file) else {
        return false;
    };
    (0..archive.len()).any(|i| {
        archive
            .by_index(i)
            .map(|e| e.name().replace('\\', "/") == name)
            .unwrap_or(false)
    })
}

fn verify_pack_archive(path: &Path) -> Result<(), String> {
    let file = std::fs::File::open(path).map_err(|e| format!("cannot open pack: {e}"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("corrupt zip: {e}"))?;
    for i in 0..archive.len() {
        let entry = archive
            .by_index(i)
            .map_err(|e| format!("corrupt zip: {e}"))?;
        let name = entry.name().replace('\\', "/");
        let lower = name.to_lowercase();
        if lower == "modrinth.index.json"
            || lower.ends_with("/modrinth.index.json")
            || lower == "manifest.json"
            || lower.ends_with("/manifest.json")
            || lower == "instance.cfg"
            || lower.ends_with("/instance.cfg")
            || lower.ends_with(".jar")
        {
            return Ok(());
        }
    }
    Err("no modrinth.index.json / manifest.json / instance.cfg / jars found".into())
}

/// Removes the resumable-download sidecar (<name>.tuffbox.part) for a dest.
fn remove_part_file(dest: &Path) {
    let name = dest
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("download");
    let part = dest.with_file_name(format!("{name}.tuffbox.part"));
    if part.exists() {
        let _ = std::fs::remove_file(part);
    }
}

// ── Drag & drop import (Library tab) ────────────────────────────────────
//
// The webview runs with `dragDropEnabled: false`, so drops arrive as DOM
// File objects without real paths. The frontend stages the bytes here via
// chunked base64 (4 MiB chunks keep JSON IPC small and give progress), then
// everything downstream is path-based: `inspect_import_source` previews the
// archive/folder for the "create a build?" dialog and `install_modpack`
// performs the actual import (it already understands Modrinth .mrpack,
// CurseForge zips, Prism instances, mods-only zips and instance folders).
//
// Foreign containers (.rar / .7z) are pre-scanned for zip-slip entry names,
// extracted into a private temp dir, verified for containment, and
// re-packed as a zip so the existing sniffing pipeline works unchanged.

/// Standard modpack folders the drop dialog reports as present/missing.
const DROP_STANDARD_DIRS: [&str; 7] = [
    "mods",
    "config",
    "resourcepacks",
    "shaderpacks",
    "overrides",
    "kubejs",
    "defaultconfigs",
];

fn drop_stage_root() -> PathBuf {
    std::env::temp_dir().join("tuffbox-drop-imports")
}

/// Validate a dropped relative path (webview-supplied). Rejects absolute
/// paths, traversal and drive letters; normalizes backslashes.
fn sanitize_drop_rel(rel: &str) -> Result<String, String> {
    let norm = rel.replace('\\', "/");
    // Reject absolute paths and drive letters before stripping leading slashes.
    if norm.starts_with('/') || norm.starts_with("//") {
        return Err(format!("absolute path not allowed: {rel}"));
    }
    let bytes = norm.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' {
        return Err(format!("absolute path not allowed: {rel}"));
    }
    let trimmed = norm.trim_start_matches('/');
    if trimmed.is_empty() {
        return Err("empty dropped file name".into());
    }
    if tuffbox_core::importer::is_unsafe_zip_rel_path(trimmed) {
        return Err(format!("unsafe dropped path: {rel}"));
    }
    if trimmed.split('/').any(|seg| {
        seg.is_empty()
            || seg == "."
            || seg == ".."
            || seg.contains(':')
            || seg == "CON"
            || seg == "NUL"
    }) {
        return Err(format!("unsafe dropped path: {rel}"));
    }
    Ok(trimmed.to_string())
}

static DROP_STAGES: Lazy<std::sync::Mutex<std::collections::HashMap<String, PathBuf>>> =
    Lazy::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));
static DROP_STAGE_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn drop_stage_dir_for(token: &str) -> Result<PathBuf, String> {
    DROP_STAGES
        .lock()
        .map(|stages| stages.get(token).cloned())
        .map_err(|_| "drop stage lock poisoned".to_string())?
        .ok_or_else(|| "unknown drop stage (expired?)".to_string())
}

/// Remove stale staging dirs (crash leftovers older than 12h) — best effort.
fn prune_drop_stage_root() {
    let root = drop_stage_root();
    let Ok(entries) = std::fs::read_dir(&root) else {
        return;
    };
    let cutoff = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        .saturating_sub(12 * 3600);
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        let modified = meta
            .modified()
            .ok()
            .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(u64::MAX);
        if modified < cutoff {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

#[tauri::command(rename_all = "camelCase")]
async fn begin_drop_import(name: String) -> Result<serde_json::Value, String> {
    tokio::task::spawn_blocking(move || {
        prune_drop_stage_root();
        let safe: String = name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ' ' {
                    c
                } else {
                    '-'
                }
            })
            .collect();
        let safe = safe.trim().trim_matches('-');
        let safe = if safe.is_empty() { "import" } else { safe };
        let token = format!(
            "{}-{}",
            tuffbox_core::time_util::compact_now(),
            DROP_STAGE_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        );
        let dir = drop_stage_root().join(format!("{safe}-{token}"));
        std::fs::create_dir_all(&dir).map_err(|e| format!("stage dir: {e}"))?;
        DROP_STAGES
            .lock()
            .map_err(|_| "drop stage lock poisoned".to_string())?
            .insert(token.clone(), dir.clone());
        Ok(serde_json::json!({
            "token": token,
            "dir": dir.to_string_lossy(),
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Append one base64 chunk of a staged file at `offset`. Creates parent
/// dirs on first sight of the rel path. Returns bytes written.
#[tauri::command(rename_all = "camelCase")]
async fn write_drop_chunk(
    token: String,
    rel: String,
    offset: u64,
    data: String,
) -> Result<serde_json::Value, String> {
    tokio::task::spawn_blocking(move || {
        let dir = drop_stage_dir_for(&token)?;
        let rel = sanitize_drop_rel(&rel)?;
        let dest = dir.join(&rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("stage parent: {e}"))?;
        }
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data.as_bytes())
            .map_err(|e| format!("bad chunk payload: {e}"))?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .open(&dest)
            .map_err(|e| format!("stage open: {e}"))?;
        std::io::Seek::seek(&mut file, std::io::SeekFrom::Start(offset))
            .map_err(|e| format!("stage seek: {e}"))?;
        std::io::Write::write_all(&mut file, &bytes).map_err(|e| format!("stage write: {e}"))?;
        Ok(serde_json::json!({ "written": bytes.len() }))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
async fn finish_drop_import(token: String) -> Result<serde_json::Value, String> {
    tokio::task::spawn_blocking(move || {
        let dir = drop_stage_dir_for(&token)?;
        if !dir.is_dir() {
            return Err("drop stage vanished".into());
        }
        DROP_STAGES
            .lock()
            .map_err(|_| "drop stage lock poisoned".to_string())?
            .remove(&token);
        Ok(serde_json::json!({ "dir": dir.to_string_lossy() }))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
async fn cancel_drop_import(token: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let dir = DROP_STAGES
            .lock()
            .map_err(|_| "drop stage lock poisoned".to_string())?
            .remove(&token);
        if let Some(dir) = dir {
            let _ = std::fs::remove_dir_all(dir);
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Removes a staged drop dir when dropped out of scope (error paths).
struct StageDirCleanup(Option<PathBuf>);
impl Drop for StageDirCleanup {
    fn drop(&mut self) {
        if let Some(dir) = self.0.take() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

// ── Drop classification (inspect_import_source) ─────────────────────────

/// Where a dropped name resolves per modpack conventions.
fn content_dir_target(name: &str) -> Option<&'static str> {
    match name.to_ascii_lowercase().as_str() {
        "mods" => Some("mods"),
        "resourcepacks" => Some("resourcepacks"),
        "shaderpacks" | "shaders" => Some("shaderpacks"),
        _ => None,
    }
}

#[derive(Default)]
struct DropSummary {
    total_entries: u64,
    files: u64,
    mods: u64,
    resourcepacks: u64,
    shaderpacks: u64,
    has_mrpack_index: bool,
    has_cf_manifest: bool,
    has_instance_cfg: bool,
    has_mmc_pack: bool,
    has_packwiz: bool,
    dirs_seen: Vec<String>,
    unsafe_names: u64,
    truncated: bool,
}

/// Single root shared by every entry (WinRAR "PackName/…" wrappers).
/// Returns `Some("PackName/")` when every file lives under one root that is
/// itself not a content dir. `names` must be file paths, `/`-separated.
fn drop_wrap_prefix(names: &[String]) -> Option<String> {
    let mut root: Option<String> = None;
    for name in names {
        let Some(first) = name.split('/').next() else {
            continue;
        };
        if first.is_empty() {
            return None;
        }
        match &root {
            Some(r) if r == first => {}
            _ => {
                if root.is_some() {
                    return None; // second distinct root → not wrapped
                }
                root = Some(first.to_string());
            }
        }
    }
    let root = root?;
    if content_dir_target(&root).is_some() || root.eq_ignore_ascii_case("overrides") {
        return None;
    }
    Some(format!("{root}/"))
}

/// Feed one `/`-separated entry name (files only) into the summary.
/// `wrap` strips a detected single-root wrapper first.
fn drop_classify_name(name: &str, wrap: &str, sum: &mut DropSummary) {
    let rel = name
        .strip_prefix(wrap)
        .unwrap_or(name)
        .trim_start_matches('/');
    // Modrinth packs put content inside overrides/; strip it to expose
    // the actual content directories (mods/, resourcepacks/, etc.).
    let rel = rel
        .strip_prefix("overrides/")
        .unwrap_or(rel);
    if rel.is_empty() {
        return;
    }
    sum.total_entries += 1;
    let lower = rel.to_ascii_lowercase();
    match lower.as_str() {
        "modrinth.index.json" => sum.has_mrpack_index = true,
        "manifest.json" => sum.has_cf_manifest = true,
        "instance.cfg" => sum.has_instance_cfg = true,
        "mmc-pack.json" => sum.has_mmc_pack = true,
        "packwiz.toml" => sum.has_packwiz = true,
        _ => {}
    }
    let segments: Vec<&str> = rel.split('/').collect();
    let top = segments[0].to_ascii_lowercase();
    if !sum.dirs_seen.iter().any(|d| d == &top) {
        sum.dirs_seen.push(top.clone());
    }
    let is_file_at = |folder: &str, depth: usize| {
        segments.len() == depth + 1 && top == folder && !rel.ends_with('/')
    };
    let ext = segments
        .last()
        .and_then(|s| s.rsplit('.').next())
        .unwrap_or("")
        .to_ascii_lowercase();
    if is_file_at("mods", 1) && ext == "jar" {
        sum.mods += 1;
        sum.files += 1;
    } else if rel.split('/').count() == 1 && ext == "jar" {
        // jars loose at the archive root are mods (is_mods_only_zip rule)
        sum.mods += 1;
        sum.files += 1;
    } else if is_file_at("resourcepacks", 1) && (ext == "zip" || ext == "jar") {
        sum.resourcepacks += 1;
        sum.files += 1;
    } else if is_file_at("shaderpacks", 1) && (ext == "zip" || ext == "jar") {
        sum.shaderpacks += 1;
        sum.files += 1;
    } else if segments.len() == 1 {
        sum.files += 1;
    } else {
        sum.files += 1;
    }
}

fn drop_format_of(sum: &DropSummary) -> &'static str {
    if sum.has_mrpack_index {
        "modrinth"
    } else if sum.has_cf_manifest {
        "curseforge"
    } else if sum.has_packwiz {
        "packwiz"
    } else if sum.has_instance_cfg || sum.has_mmc_pack {
        "prism"
    } else if sum.mods > 0 || sum.resourcepacks > 0 || sum.shaderpacks > 0 {
        "content"
    } else if sum.dirs_seen.iter().any(|d| d == "config" || d == "saves") {
        "instance"
    } else if sum.files == 0 {
        "empty"
    } else {
        "unknown"
    }
}

/// present/missing for the dialog + all counts, shaped for the frontend.
fn drop_summary_json(kind: &str, name: &str, sum: &DropSummary) -> serde_json::Value {
    let dirs: Vec<String> = sum
        .dirs_seen
        .iter()
        .filter(|d| DROP_STANDARD_DIRS.contains(&d.as_str()))
        .cloned()
        .collect();
    let present: Vec<String> = DROP_STANDARD_DIRS
        .iter()
        .filter(|d| {
            let dir_name: &str = d;
            dirs.iter().any(|x| x.as_str() == dir_name)
                || (dir_name == "mods" && sum.mods > 0)
                || (dir_name == "resourcepacks" && sum.resourcepacks > 0)
                || (dir_name == "shaderpacks" && sum.shaderpacks > 0)
        })
        .map(|d| d.to_string())
        .collect();
    let missing: Vec<String> = DROP_STANDARD_DIRS
        .iter()
        .filter(|d| !present.iter().any(|x| x == *d))
        .map(|d| d.to_string())
        .collect();
    let mut warnings: Vec<String> = Vec::new();
    if sum.unsafe_names > 0 {
        warnings.push(format!(
            "{} entries with unsafe paths were skipped",
            sum.unsafe_names
        ));
    }
    if sum.truncated {
        warnings.push("listing truncated (archive too large to scan fully)".into());
    }
    if sum.total_entries == 0 {
        warnings.push("the archive contains no files".into());
    }
    serde_json::json!({
        "kind": kind,
        "format": drop_format_of(sum),
        "name": name,
        "totalEntries": sum.total_entries,
        "counts": {
            "mods": sum.mods,
            "resourcepacks": sum.resourcepacks,
            "shaderpacks": sum.shaderpacks,
            "files": sum.files,
        },
        "present": present,
        "missing": missing,
        "warnings": warnings,
    })
}

fn summarize_drop_dir(dir: &Path) -> Result<serde_json::Value, String> {
    let mut names: Vec<String> = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    let mut sum = DropSummary::default();
    while let Some(current) = stack.pop() {
        if names.len() > 50_000 {
            sum.truncated = true;
            break;
        }
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Some(rel) = path.strip_prefix(dir).ok().and_then(|p| p.to_str()) {
                names.push(rel.replace('\\', "/"));
            }
        }
    }
    let wrap = drop_wrap_prefix(&names).unwrap_or_default();
    for name in &names {
        drop_classify_name(name, &wrap, &mut sum);
    }
    let name = dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("dropped folder")
        .to_string();
    Ok(drop_summary_json("dir", &name, &sum))
}

fn summarize_drop_zip(zip_path: &Path) -> Result<serde_json::Value, String> {
    let file = std::fs::File::open(zip_path).map_err(|e| format!("open: {e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("corrupt zip: {e}"))?;
    let mut names: Vec<String> = Vec::new();
    let mut sum = DropSummary::default();
    for i in 0..archive.len() {
        let Ok(entry) = archive.by_index(i) else { continue };
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().replace('\\', "/");
        if tuffbox_core::importer::is_unsafe_zip_rel_path(&name) {
            sum.unsafe_names += 1;
            continue;
        }
        names.push(name);
    }
    let wrap = drop_wrap_prefix(&names).unwrap_or_default();
    for name in &names {
        drop_classify_name(name, &wrap, &mut sum);
    }
    let name = zip_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("archive")
        .to_string();
    let kind = if zip_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("mrpack"))
        .unwrap_or(false)
    {
        "mrpack"
    } else {
        "zip"
    };
    Ok(drop_summary_json(kind, &name, &sum))
}

/// Entry names of a .rar / .7z without extracting (listing only).
fn list_foreign_archive_names(path: &Path) -> Result<Vec<String>, String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "7z" {
        let mut reader = sevenz_rust::SevenZReader::open(path, sevenz_rust::Password::empty())
            .map_err(|e| {
                format!(
                    "cannot open 7z archive (encrypted archives are not supported): {e}"
                )
            })?;
        let mut names = Vec::new();
        reader
            .for_each_entries(|entry, _reader| {
                names.push(entry.name().replace('\\', "/"));
                Ok(true)
            })
            .map_err(|e| format!("scan 7z failed: {e}"))?;
        return Ok(names);
    }
    // RAR
    let archive = unrar::Archive::new(path)
        .open_for_listing()
        .map_err(|e| format!("cannot open rar archive: {e}"))?;
    let mut names = Vec::new();
    for entry in archive {
        let entry = entry.map_err(|e| format!("scan rar failed: {e}"))?;
        if entry.is_directory() {
            continue;
        }
        names.push(entry.filename.to_string_lossy().replace('\\', "/"));
    }
    Ok(names)
}

fn summarize_drop_foreign(path: &Path) -> Result<serde_json::Value, String> {
    let names = list_foreign_archive_names(path)?;
    let mut sum = DropSummary::default();
    let mut safe_names: Vec<String> = Vec::new();
    for name in &names {
        if tuffbox_core::importer::is_unsafe_zip_rel_path(name) {
            sum.unsafe_names += 1;
            continue;
        }
        safe_names.push(name.clone());
    }
    let wrap = drop_wrap_prefix(&safe_names).unwrap_or_default();
    for name in &safe_names {
        drop_classify_name(name, &wrap, &mut sum);
    }
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("archive")
        .to_string();
    let kind = if path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("rar"))
        .unwrap_or(false)
    {
        "rar"
    } else {
        "7z"
    };
    Ok(drop_summary_json(kind, &name, &sum))
}

/// Preview a dropped archive/folder for the "create a build?" dialog.
#[tauri::command(rename_all = "camelCase")]
async fn inspect_import_source(path: String) -> Result<serde_json::Value, String> {
    tokio::task::spawn_blocking(move || {
        let path = PathBuf::from(&path);
        if path.is_dir() {
            summarize_drop_dir(&path)
        } else if path.is_file() {
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            match ext.as_str() {
                "zip" | "mrpack" => summarize_drop_zip(&path),
                "rar" | "7z" => summarize_drop_foreign(&path),
                other => Err(format!("unsupported import format: .{other}")),
            }
        } else {
            Err(format!("path not found: {}", path.display()))
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

// ── .rar / .7z → zip normalization for the install pipeline ─────────────

/// Extract a pre-scanned .rar/.7z into `dest` (containment-verified).
fn extract_foreign_archive(src: &Path, dest: &Path) -> Result<(), String> {
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    std::fs::create_dir_all(dest).map_err(|e| format!("extract dir: {e}"))?;
    if ext == "7z" {
        sevenz_rust::decompress_file(src, dest)
            .map_err(|e| format!("7z extraction failed: {e}"))?;
    } else {
        let mut archive = unrar::Archive::new(src)
            .open_for_processing()
            .map_err(|e| format!("cannot open rar archive: {e}"))?;
        while let Some(header) = archive.read_header().map_err(|e| format!("rar read: {e}"))? {
            archive = if header.entry().is_directory() {
                header.skip().map_err(|e| format!("rar skip: {e}"))?
            } else {
                header
                    .extract_to(dest)
                    .map_err(|e| format!("rar extract: {e}"))?
            };
        }
    }
    // Defense in depth: every produced file must stay inside dest.
    let canon_dest = dest.canonicalize().map_err(|e| e.to_string())?;
    let mut stack = vec![dest.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in std::fs::read_dir(&current).map_err(|e| e.to_string())?.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let canon = path
                    .canonicalize()
                    .map_err(|e| format!("verify {}: {e}", path.display()))?;
                if !canon.starts_with(&canon_dest) {
                    return Err(format!(
                        "archive tried to escape the extraction folder: {}",
                        path.display()
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Repack an extracted directory tree into a flat zip (rel `/` names).
fn dir_to_zip(src_dir: &Path, dest_zip: &Path) -> Result<(), String> {
    let output = std::fs::File::create(dest_zip)
        .map_err(|e| format!("create {}: {e}", dest_zip.display()))?;
    let mut zip = zip::ZipWriter::new(output);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    fn add_dir(
        zip: &mut zip::ZipWriter<std::fs::File>,
        opts: zip::write::SimpleFileOptions,
        base: &Path,
        dir: &Path,
    ) -> Result<(), String> {
        for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let rel = path
                .strip_prefix(base)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            if path.is_dir() {
                zip.add_directory(&rel, opts)
                    .map_err(|e| format!("zip dir {rel}: {e}"))?;
                add_dir(zip, opts, base, &path)?;
            } else if path.is_file() {
                zip.start_file(&rel, opts)
                    .map_err(|e| format!("zip file {rel}: {e}"))?;
                let mut f = std::fs::File::open(&path).map_err(|e| e.to_string())?;
                std::io::copy(&mut f, zip).map_err(|e| format!("zip copy {rel}: {e}"))?;
            }
        }
        Ok(())
    }
    add_dir(&mut zip, opts, src_dir, src_dir)?;
    zip.finish()
        .map_err(|e| format!("finish zip: {e}"))?;
    Ok(())
}

/// .rar / .7z packs are extracted to a private temp dir and re-packed as a
/// zip so the existing content sniffing (Modrinth/CurseForge/Prism/mods)
/// works unchanged. zip/mrpack inputs pass through untouched.
fn normalize_foreign_pack_archive(pack_path: &Path) -> Result<PathBuf, String> {
    let ext = pack_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext != "rar" && ext != "7z" {
        return Ok(pack_path.to_path_buf());
    }
    let names = list_foreign_archive_names(pack_path)?;
    let unsafe_hit = names
        .iter()
        .find(|n| tuffbox_core::importer::is_unsafe_zip_rel_path(n))
        .cloned();
    if let Some(name) = unsafe_hit {
        return Err(format!(
            "archive contains an unsafe entry ({name}) and was rejected"
        ));
    }
    let tag = tuffbox_core::time_util::compact_now();
    let extract_dir = std::env::temp_dir().join(format!("tuffbox-archive-ext-{tag}"));
    let _guard = StageDirCleanup(Some(extract_dir.clone()));
    extract_foreign_archive(pack_path, &extract_dir)?;
    let zip_path = std::env::temp_dir().join(format!("tuffbox-archive-{tag}.zip"));
    dir_to_zip(&extract_dir, &zip_path)?;
    Ok(zip_path)
}

// ── Content zips (mods / resourcepacks / shaderpacks / config) ──────────

const DROP_CONTENT_DIRS: [&str; 9] = [
    "mods",
    "resourcepacks",
    "shaderpacks",
    "config",
    "defaultconfigs",
    "kubejs",
    "scripts",
    "datapacks",
    "overrides",
];

/// File rel names inside a zip (unsafe entries skipped), `/`-separated.
fn zip_file_rel_names(zip_path: &Path) -> Result<(Vec<String>, u64), String> {
    let file = std::fs::File::open(zip_path).map_err(|e| format!("open: {e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("corrupt zip: {e}"))?;
    let mut names = Vec::new();
    let mut skipped = 0u64;
    for i in 0..archive.len() {
        let Ok(entry) = archive.by_index(i) else { continue };
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().replace('\\', "/");
        if tuffbox_core::importer::is_unsafe_zip_rel_path(&name) {
            skipped += 1;
            continue;
        }
        names.push(name);
    }
    Ok((names, skipped))
}

/// Wrap prefix of a zip on disk (see `drop_wrap_prefix`), empty for non-zips.
fn zip_wrap_prefix_of(zip_path: &Path) -> Option<String> {
    let (names, _) = zip_file_rel_names(zip_path).ok()?;
    drop_wrap_prefix(&names)
}

/// Whether a manifest-less zip carries modpack content folders (or loose
/// mod jars) — after unwrapping a single-root wrapper.
fn is_content_zip_archive(zip_path: &Path) -> bool {
    let Ok((names, _)) = zip_file_rel_names(zip_path) else {
        return false;
    };
    if names.iter().any(|n| {
        matches!(
            n.as_str(),
            "modrinth.index.json" | "manifest.json" | "instance.cfg" | "mmc-pack.json"
        )
    }) {
        return false;
    }
    let wrap = drop_wrap_prefix(&names).unwrap_or_default();
    names.iter().any(|name| {
        let rel = name.strip_prefix(&wrap.as_str()).unwrap_or(name);
        let segments: Vec<&str> = rel.split('/').collect();
        if segments.len() == 1 {
            return rel.to_ascii_lowercase().ends_with(".jar");
        }
        DROP_CONTENT_DIRS.contains(&segments[0].to_ascii_lowercase().as_str())
    })
}

/// Extract whitelisted content folders (and root-level mod jars) from a
/// zip into `dest_root`. Unsafe entries are skipped (counted in Err-free
/// form — the inspect step already warns about them).
fn extract_content_zip(zip_path: &Path, dest_root: &Path) -> Result<(), String> {
    let file = std::fs::File::open(zip_path).map_err(|e| format!("open: {e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("corrupt zip: {e}"))?;
    // Pass 1: collect names to detect the wrapper root.
    let mut names = Vec::new();
    for i in 0..archive.len() {
        let Ok(entry) = archive.by_index(i) else { continue };
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().replace('\\', "/");
        if tuffbox_core::importer::is_unsafe_zip_rel_path(&name) {
            continue;
        }
        names.push(name);
    }
    let wrap = drop_wrap_prefix(&names).unwrap_or_default();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().replace('\\', "/");
        if tuffbox_core::importer::is_unsafe_zip_rel_path(&name) {
            continue;
        }
        let rel = name
            .strip_prefix(wrap.as_str())
            .unwrap_or(name.as_str())
            .trim_start_matches('/');
        // Modrinth packs put content inside overrides/; strip it to expose
        // the actual content directories (mods/, resourcepacks/, etc.).
        let rel = rel
            .strip_prefix("overrides/")
            .unwrap_or(rel);
        // When no global wrapper was detected, try stripping the first
        // segment if it's not a content dir (handles mixed wrapped/unwrapped
        // entries like "MyPack/mods/a.jar" alongside loose "loose.jar").
        let rel = if wrap.is_empty() {
            let segs: Vec<&str> = rel.split('/').collect();
            if segs.len() > 1
                && !DROP_CONTENT_DIRS.contains(&segs[0].to_ascii_lowercase().as_str())
                && segs[0].to_ascii_lowercase() != "overrides"
            {
                segs[1..].join("/")
            } else {
                rel.to_string()
            }
        } else {
            rel.to_string()
        };
        if rel.is_empty() {
            continue;
        }
        let segments: Vec<&str> = rel.split('/').collect();
        let target_rel = if segments.len() == 1
            && rel.to_ascii_lowercase().ends_with(".jar")
        {
            format!("mods/{rel}")
        } else if DROP_CONTENT_DIRS.contains(&segments[0].to_ascii_lowercase().as_str()) {
            rel.clone()
        } else {
            continue;
        };
        let dest = dest_root.join(&target_rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
        }
        let mut out = std::fs::File::create(&dest)
            .map_err(|e| format!("write {target_rel}: {e}"))?;
        std::io::copy(&mut entry, &mut out)
            .map_err(|e| format!("extract {target_rel}: {e}"))?;
    }
    Ok(())
}

/// A dropped folder that is itself a content folder (`mods`, `resourcepacks`,
/// `shaderpacks`/`shaders`, `config`) — stage a copy with the expected game
/// layout (empty `mods/` included) so the normal folder import accepts it.
/// Returns `None` when the folder already imports through the standard path.
fn stage_content_only_dir(path: &Path) -> Option<PathBuf> {
    let folder_name = path.file_name().and_then(|s| s.to_str())?;
    let mut copies: Vec<(PathBuf, &'static str)> = Vec::new();
    if let Some(target) = content_dir_target(folder_name) {
        // `mods` alone already passes the standard instance check.
        if target == "mods" {
            return None;
        }
        copies.push((path.to_path_buf(), target));
    } else {
        // Parent folder with only content dirs inside (no instance markers).
        if path.join("instance.cfg").is_file()
            || path.join("mmc-pack.json").is_file()
            || path.join("manifest.json").is_file()
            || path.join("modrinth.index.json").is_file()
            || path.join("minecraftinstance.json").is_file()
        {
            return None;
        }
        let mut saw_content = false;
        for entry in std::fs::read_dir(path).ok()?.flatten() {
            let child = entry.path();
            if !child.is_dir() {
                continue;
            }
            let child_name = child.file_name()?.to_str()?;
            if child_name.starts_with('.') {
                continue;
            }
            let Some(target) = content_dir_target(child_name) else {
                return None; // any non-content child → use the standard flow
            };
            if target != "config" {
                saw_content = true;
            }
            copies.push((child, target));
        }
        if !saw_content {
            return None;
        }
    }
    let slug: String = folder_name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let stage = std::env::temp_dir().join(format!(
        "tuffbox-content-dir-{}-{}",
        slug,
        tuffbox_core::time_util::compact_now()
    ));
    std::fs::create_dir_all(stage.join("mods")).ok()?;
    for (src, target) in &copies {
        if crate::helpers::copy_dir_recursive(src, &stage.join(target)).is_err() {
            let _ = std::fs::remove_dir_all(&stage);
            return None;
        }
    }
    Some(stage)
}

#[cfg(test)]
mod drop_import_tests {
    use super::*;

    fn write_zip(path: &std::path::Path, entries: &[(&str, &[u8])]) {
        let file = std::fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, data) in entries {
            zip.start_file(*name, options).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn sanitize_drop_rel_rejects_traversal_and_accepts_normal() {
        assert!(sanitize_drop_rel("../escaped.txt").is_err());
        assert!(sanitize_drop_rel("mods/../../escape.jar").is_err());
        assert!(sanitize_drop_rel("/absolute/path.zip").is_err());
        assert!(sanitize_drop_rel("C:/system32/x.dll").is_err());
        assert!(sanitize_drop_rel("a//b.zip").is_err());
        assert_eq!(
            sanitize_drop_rel("mods\\jei-1.0.jar").unwrap(),
            "mods/jei-1.0.jar"
        );
    }

    #[test]
    fn wrap_prefix_detects_single_root_and_ignores_content_roots() {
        let names = vec!["Pack/mods/a.jar".to_string(), "Pack/config/x.toml".to_string()];
        assert_eq!(drop_wrap_prefix(&names).as_deref(), Some("Pack/"));
        let mixed = vec!["Pack/a".to_string(), "Other/b".to_string()];
        assert_eq!(drop_wrap_prefix(&mixed), None);
        let content = vec!["mods/a.jar".to_string()];
        assert_eq!(drop_wrap_prefix(&content), None);
    }

    #[test]
    fn classify_counts_content_and_markers() {
        let mut sum = DropSummary::default();
        for name in [
            "Pack/modrinth.index.json",
            "Pack/overrides/mods/jei.jar",
            "Pack/resourcepacks/fancy.zip",
            "Pack/shaderpacks/soft.zip",
        ] {
            drop_classify_name(name, "Pack/", &mut sum);
        }
        assert_eq!(drop_format_of(&sum), "modrinth");
        assert_eq!(sum.mods, 1);
        assert_eq!(sum.resourcepacks, 1);
        assert_eq!(sum.shaderpacks, 1);
    }

    #[test]
    fn content_zip_detection_covers_resourcepacks_and_wrappers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pack.zip");
        write_zip(&path, &[("resourcepacks/fancy.zip", b"pk3")]);
        assert!(is_content_zip_archive(&path));

        let wrapped = dir.path().join("wrapped.zip");
        write_zip(&wrapped, &[("MyPack/mods/a.jar", b"jar")]);
        assert!(is_content_zip_archive(&wrapped));

        let cf = dir.path().join("cf.zip");
        write_zip(&cf, &[("manifest.json", b"{}")]);
        assert!(!is_content_zip_archive(&cf));

        let junk = dir.path().join("junk.zip");
        write_zip(&junk, &[("readme.txt", b"hi")]);
        assert!(!is_content_zip_archive(&junk));
    }

    #[test]
    fn extract_content_zip_unwraps_and_reroots_jars() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("in.zip");
        write_zip(
            &zip_path,
            &[
                ("MyPack/mods/a.jar", b"jar-a"),
                ("MyPack/resourcepacks/fancy.zip", b"pk3"),
                ("loose.jar", b"loose"),
                ("MyPack/ignore.txt", b"nope"),
            ],
        );
        let out = dir.path().join("out");
        std::fs::create_dir_all(&out).unwrap();
        extract_content_zip(&zip_path, &out).unwrap();
        assert!(out.join("mods/a.jar").is_file());
        assert!(out.join("resourcepacks/fancy.zip").is_file());
        assert!(out.join("mods/loose.jar").is_file());
        assert!(!out.join("ignore.txt").exists());
    }

    #[test]
    fn stage_content_only_dir_scaffolds_game_layout() {
        let dir = tempfile::tempdir().unwrap();
        let rp = dir.path().join("resourcepacks");
        std::fs::create_dir_all(&rp).unwrap();
        std::fs::write(rp.join("fancy.zip"), b"pk3").unwrap();

        // A bare content folder stages; `mods` alone goes through the
        // standard flow unchanged.
        let staged = stage_content_only_dir(&rp).expect("resourcepacks folder stages");
        assert!(staged.join("mods").is_dir());
        assert!(staged.join("resourcepacks/fancy.zip").is_file());
        let _ = std::fs::remove_dir_all(staged);

        let mods_dir = dir.path().join("mods");
        std::fs::create_dir_all(&mods_dir).unwrap();
        assert!(stage_content_only_dir(&mods_dir).is_none());
    }
}

fn extract_prism_zip_content(zip_path: &Path, instance_dir: &Path) -> Result<(), String> {
    let file = std::fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().replace('\\', "/");
        let lower = name.to_lowercase();
        // Skip launcher metadata; keep game content.
        if lower == "instance.cfg"
            || lower == "mmc-pack.json"
            || lower.ends_with("tuffbox.remote-mods.json")
            || name.ends_with('/')
        {
            continue;
        }
        // Strip optional minecraft/ or .minecraft/ prefix used by some exporters.
        let rel = name
            .strip_prefix("minecraft/")
            .or_else(|| name.strip_prefix(".minecraft/"))
            .unwrap_or(name.as_str());
        if rel.is_empty() || rel.contains("..") {
            continue;
        }
        let dest = instance_dir.join(rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut out = std::fs::File::create(&dest).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Re-download only the mods that failed in the last sync (user Retry).
#[tauri::command(rename_all = "camelCase")]
async fn retry_failed_mod_downloads(
    app: tauri::AppHandle,
    path: String,
    mod_ids: Vec<String>,
) -> Result<serde_json::Value, String> {
    tokio::task::spawn_blocking(move || {
        let manifest_path = PathBuf::from(&path);
        let mut manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
        // Re-resolve any CurseForge entries that still lack URLs.
        let needs_cf = manifest.mods.iter().any(|m| {
            mod_ids.contains(&m.id)
                && m.source.kind == SourceKind::Curseforge
                && m.source.url.as_ref().map(|u| u.is_empty()).unwrap_or(true)
        });
        if needs_cf {
            let _ = tuffbox_core::resolve_curseforge_pack_files(&mut manifest);
            let _ = save_manifest(&manifest_path, &manifest);
        }
        let only: std::collections::HashSet<String> = mod_ids.into_iter().collect();
        let report =
            download_project_mods_tracked(&app, &manifest_path, &manifest, Some(&only), true);
        Ok(serde_json::json!({ "download": report }))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
#[allow(deprecated)]
fn open_project_folder(
    _app: tauri::AppHandle,
    path: String,
    subdir: Option<String>,
) -> Result<(), String> {
    let project_dir = PathBuf::from(&path)
        .parent()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| "manifest has no parent directory".to_string())?;
    let dir = match subdir
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(sub) => {
            // Only allow known instance content folders (no path traversal).
            const ALLOWED: &[&str] = &[
                "resourcepacks",
                "shaderpacks",
                "saves",
                "mods",
                "config",
                "screenshots",
            ];
            if !ALLOWED.contains(&sub) || sub.contains('/') || sub.contains('\\') || sub.contains("..")
            {
                return Err(format!("unsupported instance subfolder: {sub}"));
            }
            let target = project_dir.join(sub);
            std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
            target
        }
        None => project_dir,
    };
    // Direct detached spawn instead of tauri-plugin-shell: its open() path
    // uses ShellExecuteExW + CoInitialize on the IPC thread and aborted the
    // whole app (0xC0000409) when the user pressed the Home Folder button.
    file_manager::open_in_file_manager(&dir, None)
}

static PENDING_LAUNCH_PROJECT: Lazy<Mutex<Option<String>>> =
    Lazy::new(|| Mutex::new(None));

fn parse_launch_cli_args() {
    let args: Vec<String> = std::env::args().collect();
    let mut pending: Option<String> = None;
    let mut i = 1;
    while i < args.len() {
        let a = args[i].as_str();
        if a == "--launch" || a == "--open" {
            if let Some(path) = args.get(i + 1) {
                pending = Some(path.clone());
                i += 2;
                continue;
            }
        } else if let Some(rest) = a.strip_prefix("--launch=") {
            pending = Some(rest.to_string());
        } else if let Some(rest) = a.strip_prefix("--open=") {
            pending = Some(rest.to_string());
        } else if a.ends_with(".tuffbox.json") || a.ends_with("tuffbox.json") {
            // Allow dropping a manifest onto the exe / associating the file type.
            pending = Some(a.to_string());
        }
        i += 1;
    }
    if let Some(path) = pending {
        if let Ok(resolved) = resolve_manifest_path(&path) {
            if let Ok(mut slot) = PENDING_LAUNCH_PROJECT.lock() {
                *slot = Some(resolved.to_string_lossy().to_string());
            }
        } else if let Ok(mut slot) = PENDING_LAUNCH_PROJECT.lock() {
            *slot = Some(path);
        }
    }
}

/// Pop one-shot `--launch` / `--open` path from process start (frontend auto-launches).
#[tauri::command(rename_all = "camelCase")]
fn take_pending_launch_project() -> Option<String> {
    PENDING_LAUNCH_PROJECT.lock().ok().and_then(|mut g| g.take())
}

fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

#[tauri::command(rename_all = "camelCase")]
fn create_project_desktop_shortcut(path: String) -> Result<String, String> {
    let manifest_path = resolve_manifest_path(&path)?;
    let manifest =
        ProjectManifest::load_from_path(&manifest_path).map_err(|e| e.to_string())?;
    let desktop = dirs::desktop_dir().ok_or_else(|| "desktop folder was not found".to_string())?;
    let safe_name: String = manifest
        .project
        .name
        .chars()
        .map(|ch| {
            if matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' | '\n' | '\r') {
                '_'
            } else {
                ch
            }
        })
        .collect::<String>()
        .trim()
        .to_string();
    let safe_name = if safe_name.is_empty() {
        "Instance".to_string()
    } else {
        safe_name
    };

    let exe = std::env::current_exe().map_err(|e| format!("current exe: {e}"))?;
    let exe_dir = exe
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    let manifest_str = manifest_path.to_string_lossy().to_string();
    let args = format!("--launch \"{manifest_str}\"");

    #[cfg(target_os = "windows")]
    {
        let shortcut = desktop.join(format!("TuffBox - {safe_name}.lnk"));
        let script = format!(
            "$ws = New-Object -ComObject WScript.Shell; \
             $s = $ws.CreateShortcut({lnk}); \
             $s.TargetPath = {exe}; \
             $s.Arguments = {args}; \
             $s.WorkingDirectory = {cwd}; \
             $s.WindowStyle = 1; \
             $s.Description = {desc}; \
             $s.Save();",
            lnk = ps_quote(&shortcut.to_string_lossy()),
            exe = ps_quote(&exe.to_string_lossy()),
            args = ps_quote(&args),
            cwd = ps_quote(&exe_dir.to_string_lossy()),
            desc = ps_quote(&format!("Launch {} with TuffBox", manifest.project.name)),
        );
        let output = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                &script,
            ])
            .output()
            .map_err(|e| format!("powershell failed: {e}"))?;
        if !output.status.success() || !shortcut.is_file() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let bat = desktop.join(format!("TuffBox - {safe_name}.bat"));
            let bat_body = format!(
                "@echo off\r\nstart \"\" \"{}\" --launch \"{}\"\r\n",
                exe.to_string_lossy().replace('"', ""),
                manifest_str.replace('"', ""),
            );
            std::fs::write(&bat, &bat_body).map_err(|e| e.to_string())?;
            if !output.status.success() && !stderr.trim().is_empty() {
                return Ok(format!(
                    "{} (wrote .bat; .lnk error: {})",
                    bat.to_string_lossy(),
                    stderr.trim()
                ));
            }
            return Ok(bat.to_string_lossy().to_string());
        }
        return Ok(shortcut.to_string_lossy().to_string());
    }

    #[cfg(target_os = "macos")]
    {
        let shortcut = desktop.join(format!("TuffBox - {safe_name}.command"));
        let contents = format!(
            "#!/bin/bash\nexec {} --launch {}\n",
            helpers::shell_escape(&exe.to_string_lossy()),
            helpers::shell_escape(&manifest_str),
        );
        std::fs::write(&shortcut, contents).map_err(|e| e.to_string())?;
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&shortcut)
                .map_err(|e| e.to_string())?
                .permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&shortcut, perms).map_err(|e| e.to_string())?;
        }
        return Ok(shortcut.to_string_lossy().to_string());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let shortcut = desktop.join(format!("TuffBox - {safe_name}.desktop"));
        let contents = format!(
            "[Desktop Entry]\nType=Application\nName=TuffBox - {safe_name}\nExec={exe} --launch {manifest}\nTerminal=false\nCategories=Game;\n",
            exe = helpers::shell_escape(&exe.to_string_lossy()),
            manifest = helpers::shell_escape(&manifest_str),
        );
        std::fs::write(&shortcut, contents).map_err(|e| e.to_string())?;
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&shortcut)
                .map_err(|e| e.to_string())?
                .permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&shortcut, perms).map_err(|e| e.to_string())?;
        }
        Ok(shortcut.to_string_lossy().to_string())
    }
}

#[tauri::command]
fn delete_project(path: String) -> Result<(), String> {
    std::fs::remove_file(&path).map_err(|e| e.to_string())
}

/// Packs `logs/`, `crash-reports/` and test-run history into a zip next to
/// the manifest and returns its path, so the UI's "Create logs.zip" action
/// (previously an `alert("not implemented yet")` stub) actually produces a
/// shareable archive.
#[tauri::command(rename_all = "camelCase")]
fn create_logs_zip(path: String) -> Result<String, String> {
    let project_dir = manifest_parent(&path)?;
    let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
    let timestamp = tuffbox_core::time_util::compact_now();
    let output = project_dir.join(format!("{}-logs-{timestamp}.zip", manifest.project.id));
    let result = tuffbox_core::export_logs_zip(&project_dir, &output).map_err(|e| e.to_string())?;
    Ok(result.path.to_string_lossy().to_string())
}

/// Duplicates a project (manifest + mods/config/overrides folders, minus
/// `.tuffbox/` internal state and snapshots) into a sibling directory,
/// implementing the previously-stubbed "Clone as..." action.
#[tauri::command(rename_all = "camelCase")]
fn clone_project(path: String, new_name: String) -> Result<String, String> {
    let source_dir = manifest_parent(&path)?;
    let mut manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;

    let new_slug = slugify_project_name(&new_name);
    let target_dir = source_dir
        .parent()
        .map(|p| p.join(&new_slug))
        .ok_or_else(|| "project has no parent directory".to_string())?;
    if target_dir.exists() {
        return Err(format!(
            "a folder named '{new_slug}' already exists next to this project"
        ));
    }
    std::fs::create_dir_all(&target_dir).map_err(|e| e.to_string())?;

    for entry_name in [
        "mods",
        "config",
        "defaultconfigs",
        "kubejs",
        "scripts",
        "overrides",
    ] {
        let src = source_dir.join(entry_name);
        if src.is_dir() {
            copy_dir_recursive(&src, &target_dir.join(entry_name)).map_err(|e| e.to_string())?;
        }
    }

    manifest.project.id = new_slug.clone();
    manifest.project.name = new_name;

    let target_manifest = target_dir.join(format!("{new_slug}.tuffbox.json"));
    save_manifest(&target_manifest, &manifest).map_err(|e| e.to_string())?;

    Ok(target_manifest.to_string_lossy().to_string())
}

/// Re-syncs a project's content folders against the manifest: re-downloads
/// any missing/hash-mismatched mod/resourcepack/shaderpack/datapack files.
/// This is the honest version of the previously-stubbed "Repair Profile"
/// action — it doesn't pretend to fix arbitrary problems, but it does fix
/// the most common real one (missing or corrupted content files).
#[tauri::command(rename_all = "camelCase")]
async fn repair_project(path: String) -> Result<serde_json::Value, String> {
    let report = tokio::task::spawn_blocking({
        let path = path.clone();
        move || {
            let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
            let instance_dir = tuffbox_core::instance_dir_for_manifest(&PathBuf::from(&path))
                .ok_or_else(|| "manifest has no parent directory".to_string())?;
            let task_id = format!("repair-{}", manifest.project.id);
            if !tuffbox_core::task_progress::try_start_task(
                task_id.clone(),
                format!("Repair {}", manifest.project.name),
            ) {
                return Err(format!(
                    "Repair of {} is already running",
                    manifest.project.name
                ));
            }
            tuffbox_core::task_progress::set_progress(&task_id, 0.1, Some("Checking mod files…".into()));
            let report = tuffbox_core::ensure_project_mods_downloaded_cancellable(
                &manifest,
                &instance_dir,
                &tuffbox_core::ProgressCallback::new(),
                Some(&task_id),
            );
            if report
                .skipped
                .iter()
                .any(|s| s.starts_with("__cancelled__"))
            {
                tuffbox_core::task_progress::mark_cancelled(&task_id, Some("cancelled by user".into()));
                return Err("Repair cancelled".to_string());
            }
            let detail = if !report.downloaded.is_empty() {
                format!("{} file(s) re-downloaded", report.downloaded.len())
            } else {
                "all files present".into()
            };
            tuffbox_core::task_progress::succeed(&task_id, Some(detail));
            Ok(report)
        }
    })
    .await
    .map_err(|e| e.to_string())??;

    let duplicates = detect_duplicate_mod_jars(path.clone()).await.unwrap_or_default();
    let wrong_loader = detect_wrong_loader_mods(path).await.unwrap_or_default();

    Ok(serde_json::json!({
        "downloaded": report.downloaded,
        "failed": report.failed,
        "alreadyPresent": report.already_present,
        "skipped": report.skipped,
        "duplicates": duplicates,
        "wrongLoader": wrong_loader,
    }))
}

#[tauri::command(rename_all = "camelCase")]
async fn get_minecraft_versions() -> Result<Vec<tuffbox_core::versions::MinecraftVersion>, String> {
    // Never fails: cached / popular-pins fallback inside fetch_minecraft_versions.
    Ok(tokio::task::spawn_blocking(|| {
        tuffbox_core::versions::fetch_minecraft_versions()
    })
    .await
    .map_err(|e| e.to_string())?)
}

#[tauri::command(rename_all = "camelCase")]
async fn get_loader_versions(
    loader: String,
    minecraft_version: String,
) -> Result<Vec<tuffbox_core::versions::LoaderVersion>, String> {
    // Never fails: cache + empty-list fallback inside fetch_loader_versions.
    Ok(tokio::task::spawn_blocking(move || {
        tuffbox_core::versions::fetch_loader_versions(&loader, &minecraft_version)
    })
    .await
    .map_err(|e| e.to_string())?)
}

/// Category-aware heap recommendation for one instance (Project Settings
/// preview; launch logs the same estimate but keeps the memory setting).
#[tauri::command(rename_all = "camelCase")]
fn recommend_heap_cmd(path: String) -> Result<tuffbox_core::jvm_tuning::HeapRecommendation, String> {
    let manifest_path = resolve_manifest_path(&path)?;
    let manifest = ProjectManifest::load_from_path(&manifest_path).map_err(|e| e.to_string())?;
    let loader_slug = tuffbox_core::graph::loader_kind_slug(&manifest.loader.kind);
    let mod_categories: Vec<Vec<String>> = manifest
        .mods
        .iter()
        .filter(|m| {
            matches!(
                m.content_type,
                tuffbox_core::manifest::ContentType::Mod
            )
        })
        .map(|m| m.source.categories.clone())
        .collect();
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    let total_ram_mb = sys.total_memory() / 1024 / 1024;
    Ok(tuffbox_core::jvm_tuning::recommend_heap_mb(
        total_ram_mb,
        loader_slug,
        &mod_categories,
    ))
}

/// Loader/version-aware JVM flag recommendation for one instance. Uses the
/// required Java for the game version (launch re-resolves with the real
/// runtime); never emits heap-size options.
#[tauri::command(rename_all = "camelCase")]
fn recommend_jvm_cmd(path: String) -> Result<tuffbox_core::jvm_tuning::JvmRecommendation, String> {
    let manifest_path = resolve_manifest_path(&path)?;
    let manifest = ProjectManifest::load_from_path(&manifest_path).map_err(|e| e.to_string())?;
    let loader_slug = tuffbox_core::graph::loader_kind_slug(&manifest.loader.kind);
    let mod_categories: Vec<Vec<String>> = manifest
        .mods
        .iter()
        .filter(|m| {
            matches!(
                m.content_type,
                tuffbox_core::manifest::ContentType::Mod
            )
        })
        .map(|m| m.source.categories.clone())
        .collect();
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    let total_ram_mb = sys.total_memory() / 1024 / 1024;
    let heap_rec = tuffbox_core::jvm_tuning::recommend_heap_mb(
        total_ram_mb,
        loader_slug,
        &mod_categories,
    );
    let heavy_pack = heap_rec.category_mb >= 2048 || heap_rec.mod_count >= 100;
    let java_major = tuffbox_core::jre::required_java_major(&manifest.minecraft.version);
    Ok(tuffbox_core::jvm_tuning::recommend_jvm_args(
        loader_slug,
        &manifest.minecraft.version,
        java_major,
        heavy_pack,
    ))
}

#[tauri::command(rename_all = "camelCase")]
async fn find_java_runtimes() -> Result<Vec<tuffbox_core::jre::JavaRuntime>, String> {
    tokio::task::spawn_blocking(|| tuffbox_core::jre::find_all_runtimes_full())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Download the latest GraalVM Community JDK into the managed runtime folder
/// when no Java is installed (or force-refresh via ensure).
#[tauri::command(rename_all = "camelCase")]
async fn ensure_java_runtime() -> Result<tuffbox_core::jre::JavaRuntime, String> {
    tokio::task::spawn_blocking(|| tuffbox_core::jre::ensure_java())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_java_version(path: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let runtime = tuffbox_core::jre::check_java_at_path(&PathBuf::from(path))
            .map_err(|e| e.to_string())?;
        Ok(runtime.version)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_default_java_version() -> Result<String, String> {
    tokio::task::spawn_blocking(|| {
        tuffbox_core::jre::find_all_runtimes()
            .map_err(|e| e.to_string())?
            .into_iter()
            .next()
            .ok_or_else(|| "no java runtime found".to_string())
            .map(|runtime| runtime.version)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn append_release_artifact(
    manifest_path: &str,
    kind: &str,
    result: &tuffbox_core::ExportResult,
) -> anyhow::Result<()> {
    let project_dir = PathBuf::from(manifest_path)
        .parent()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| anyhow::anyhow!("manifest has no parent directory"))?;
    let dir = project_dir.join(".tuffbox");
    std::fs::create_dir_all(&dir)?;
    let artifacts_path = dir.join("artifacts.json");
    let mut artifacts: Vec<ReleaseArtifactRecord> = if artifacts_path.is_file() {
        serde_json::from_str(&std::fs::read_to_string(&artifacts_path)?).unwrap_or_default()
    } else {
        Vec::new()
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    artifacts.push(ReleaseArtifactRecord {
        id: format!("artifact-{kind}-{now}"),
        kind: kind.to_string(),
        path: result.path.to_string_lossy().to_string(),
        created_at: now.to_string(),
        file_count: result.file_count,
        override_count: result.override_count,
    });
    if artifacts.len() > 100 {
        let keep_from = artifacts.len().saturating_sub(100);
        artifacts = artifacts.split_off(keep_from);
    }
    std::fs::write(artifacts_path, serde_json::to_string_pretty(&artifacts)?)?;
    Ok(())
}

fn append_test_run_record(
    manifest_path: &str,
    profile: &str,
    log_path: &Path,
) -> anyhow::Result<()> {
    let project_dir = PathBuf::from(manifest_path)
        .parent()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| anyhow::anyhow!("manifest has no parent directory"))?;
    let dir = project_dir.join(".tuffbox");
    std::fs::create_dir_all(&dir)?;
    let runs_path = dir.join("test-runs.json");
    let mut runs: Vec<TestRunRecord> = if runs_path.is_file() {
        serde_json::from_str(&std::fs::read_to_string(&runs_path)?).unwrap_or_default()
    } else {
        Vec::new()
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    runs.push(TestRunRecord {
        id: format!("run-{profile}-{now}"),
        profile: profile.to_string(),
        started_at: now.to_string(),
        status: "started".to_string(),
        log_path: log_path.to_string_lossy().to_string(),
        duration_seconds: None,
        verdict_reason: None,
        captured_paths: Vec::new(),
        peak_proc_mb: None,
        peak_host_mb: None,
        recommended_ram_gb: None,
    });
    if runs.len() > 100 {
        let keep_from = runs.len().saturating_sub(100);
        runs = runs.split_off(keep_from);
    }
    std::fs::write(runs_path, serde_json::to_string_pretty(&runs)?)?;
    Ok(())
}

/// Lists all log files in the instance's logs/ folder with sizes and
/// modification times, similar to NitroLaunch's get_instance_logs.
#[tauri::command(rename_all = "camelCase")]
fn list_instance_logs(path: String) -> Result<Vec<serde_json::Value>, String> {
    let project_dir = manifest_parent(&path)?;
    let logs_dir = project_dir.join("logs");
    if !logs_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut entries: Vec<_> = std::fs::read_dir(&logs_dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .filter_map(|entry| {
            let p = entry.path();
            if !p.is_file() {
                return None;
            }
            let meta = p.metadata().ok()?;
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs());
            Some(serde_json::json!({
                "name": entry.file_name().to_string_lossy(),
                "size": meta.len(),
                "modified": modified,
            }))
        })
        .collect();
    entries.sort_by_key(|e| -(e["modified"].as_u64().unwrap_or(0) as i64));
    Ok(entries)
}

/// Reads a specific log file from the instance's logs/ folder.
#[tauri::command(rename_all = "camelCase")]
fn read_instance_log(path: String, log_name: String) -> Result<String, String> {
    let project_dir = manifest_parent(&path)?;
    let log_path = project_dir.join("logs").join(&log_name);
    if !log_path.exists() {
        return Ok(String::new());
    }
    let resolved = std::fs::canonicalize(&log_path).map_err(|e| e.to_string())?;
    if !resolved.starts_with(&project_dir.join("logs")) {
        return Err("path traversal detected".to_string());
    }
    tuffbox_core::process::read_log_tail(&log_path, 5000).map_err(|e| e.to_string())
}

/// Returns the total size of the instance on disk (mods, configs,
/// resourcepacks, etc.), useful for UI display like NitroLaunch.
#[tauri::command(rename_all = "camelCase")]
async fn get_instance_size(path: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || home_bootstrap::instance_size_label(&path))
        .await
        .map_err(|e| e.to_string())?
}

/// Resolve a manifest normally, while still supporting a staged server log
/// before `prepare_server_instance` has copied `tuffbox.project.json` into the
/// selected folder. The latter is important because the Live modal opens at
/// the beginning of a server launch, not only after staging succeeds.
fn launch_log_parent(path: &str) -> Result<PathBuf, String> {
    manifest_parent(path).or_else(|_| {
        PathBuf::from(path)
            .parent()
            .map(|parent| parent.to_path_buf())
            .ok_or_else(|| "manifest has no parent directory".to_string())
    })
}

#[tauri::command(rename_all = "camelCase")]
fn get_launch_log(path: String) -> Result<String, String> {
    let project_dir = launch_log_parent(&path)?;
    let log_path = resolve_live_launch_log(&project_dir.join("logs"));
    tuffbox_core::process::read_log_tail(&log_path, 2500).map_err(|error| error.to_string())
}

/// Exposes the exact file currently selected by `get_launch_log` without
/// duplicating its latest.log/console fallback in the frontend. This is used
/// by LaunchLogModal to label the Live tab truthfully while Minecraft takes
/// ownership of latest.log during startup.
#[tauri::command(rename_all = "camelCase")]
fn resolve_live_launch_log_path(path: String) -> Result<String, String> {
    let project_dir = launch_log_parent(&path)?;
    Ok(resolve_live_launch_log(&project_dir.join("logs"))
        .to_string_lossy()
        .into_owned())
}

/// Same source the Live tab tails: prefer Minecraft `latest.log` once it has
/// real content, else TuffBox console capture.
fn resolve_live_launch_log(logs_dir: &Path) -> PathBuf {
    let console = logs_dir.join("tuffbox-console.log");
    let latest = logs_dir.join("latest.log");
    let console_len = std::fs::metadata(&console).map(|m| m.len()).unwrap_or(0);
    let latest_len = std::fs::metadata(&latest).map(|m| m.len()).unwrap_or(0);
    if latest_len > 256 {
        latest
    } else if console_len > 0 {
        console
    } else if latest.exists() {
        latest
    } else {
        console
    }
}

/// Upload a crash / instance log to mclo.gs and return the public share URL.
/// - `logName` set → that file under logs/ or crash-reports/
/// - `logName` = `__live__` → same resolution as the Live log tab (latest.log preferred)
/// - `logName` omitted → newest crash-report, then latest.log (post-crash share)
#[tauri::command(rename_all = "camelCase")]
fn share_log_mclogs(
    path: String,
    log_name: Option<String>,
) -> Result<serde_json::Value, String> {
    let project_dir = manifest_parent(&path)?;
    let logs_dir = project_dir.join("logs");
    let crashes_dir = project_dir.join("crash-reports");

    let log_path = match log_name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        Some("__live__") => {
            let p = resolve_live_launch_log(&logs_dir);
            if !p.exists() {
                return Err("no live log found (latest.log / console empty)".into());
            }
            p
        }
        Some(name) => {
            // Basename-only: reject path separators / traversal before join.
            if name.contains("..")
                || name.contains('/')
                || name.contains('\\')
                || name.contains('\0')
                || Path::new(name).file_name().and_then(|f| f.to_str()) != Some(name)
            {
                return Err("invalid log name".into());
            }
            let candidate = if name.starts_with("crash-") || name.ends_with(".txt") {
                crashes_dir.join(name)
            } else {
                logs_dir.join(name)
            };
            if !candidate.exists() {
                return Err(format!("log not found: {name}"));
            }
            let canonical_project =
                std::fs::canonicalize(&project_dir).map_err(|e| e.to_string())?;
            let resolved = std::fs::canonicalize(&candidate).map_err(|e| e.to_string())?;
            if !resolved.starts_with(&canonical_project) {
                return Err("log path escapes project directory".into());
            }
            let under_logs = resolved.starts_with(
                std::fs::canonicalize(&logs_dir).unwrap_or_else(|_| logs_dir.clone()),
            );
            let under_crashes = resolved.starts_with(
                std::fs::canonicalize(&crashes_dir).unwrap_or_else(|_| crashes_dir.clone()),
            );
            if !under_logs && !under_crashes {
                return Err("log path must be under logs/ or crash-reports/".into());
            }
            resolved
        }
        None => pick_shareable_crash_log(&logs_dir, &crashes_dir)
            .ok_or_else(|| "no crash report or latest.log found to share".to_string())?,
    };

    // Read more than the UI tail so the shared paste has useful context.
    let content = tuffbox_core::process::read_log_tail(&log_path, 20_000)
        .map_err(|e| e.to_string())?;
    if content.trim().is_empty() {
        return Err("log file is empty".into());
    }

    let manifest = ProjectManifest::load_from_path(&path).ok();
    let mut metadata = Vec::new();
    if let Some(m) = &manifest {
        metadata.push(tuffbox_core::mclo_gs::MetadataEntry {
            key: "minecraft".into(),
            value: serde_json::json!(m.minecraft.version),
            label: Some("Minecraft".into()),
            visible: Some(true),
        });
        metadata.push(tuffbox_core::mclo_gs::MetadataEntry {
            key: "loader".into(),
            value: serde_json::json!(format!("{} {}", m.loader.kind.as_str(), m.loader.version)),
            label: Some("Loader".into()),
            visible: Some(true),
        });
        metadata.push(tuffbox_core::mclo_gs::MetadataEntry {
            key: "project".into(),
            value: serde_json::json!(m.project.name),
            label: Some("Project".into()),
            visible: Some(true),
        });
    }
    metadata.push(tuffbox_core::mclo_gs::MetadataEntry {
        key: "file".into(),
        value: serde_json::json!(log_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("log")),
        label: Some("File".into()),
        visible: Some(true),
    });

    let shared = tuffbox_core::mclo_gs::upload_log(&content, "TuffBox IDE", metadata)
        .map_err(|e| e.to_string())?;

    Ok(serde_json::json!({
        "id": shared.id,
        "url": shared.url,
        "rawUrl": shared.raw_url,
        "lines": shared.lines,
        "size": shared.size,
        // Intentionally omit token from the renderer — keep deletion capability
        // out of the webview attack surface.
        "fileName": log_path.file_name().and_then(|n| n.to_str()),
    }))
}

fn pick_shareable_crash_log(logs_dir: &Path, crashes_dir: &Path) -> Option<PathBuf> {
    // Newest crash-report first (best match for "I just crashed").
    if let Ok(rd) = std::fs::read_dir(crashes_dir) {
        let mut reports: Vec<_> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.is_file()
                    && p.extension()
                        .and_then(|e| e.to_str())
                        .is_some_and(|e| e.eq_ignore_ascii_case("txt"))
            })
            .collect();
        reports.sort_by_key(|p| {
            std::cmp::Reverse(
                p.metadata()
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
            )
        });
        if let Some(first) = reports.into_iter().next() {
            return Some(first);
        }
    }
    let latest = logs_dir.join("latest.log");
    if latest.exists() {
        return Some(latest);
    }
    let console = logs_dir.join("tuffbox-console.log");
    if console.exists() {
        return Some(console);
    }
    None
}

/// Analyze an arbitrary log/console text against the installed mods of a
/// project and return the suspected mods together with the exact line numbers
/// where they were referenced, so the UI can highlight those lines.
#[tauri::command(rename_all = "camelCase")]
async fn analyze_log_text(
    path: String,
    text: String,
) -> Result<serde_json::Value, String> {
    // Blocking-pool conversion: sync commands run on the main thread; the
    // suspect scan over the pasted log must not freeze IPC delivery.
    tokio::task::spawn_blocking(move || analyze_log_text_impl(path, text))
        .await
        .map_err(|e| e.to_string())?
}

fn analyze_log_text_impl(
    path: String,
    text: String,
) -> Result<serde_json::Value, String> {
    use tuffbox_core::crash::analyze_text_for_suspects;
    let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
    let (signals, suspected_mods) = analyze_text_for_suspects(&text, "log", &manifest);
    let highlights: Vec<serde_json::Value> = suspected_mods
        .iter()
        .flat_map(|s| {
            s.evidence.iter().map(move |ev| {
                serde_json::json!({
                    "lineNumber": ev.line_number,
                    "modId": s.id,
                    "modName": s.name,
                    "confidence": s.confidence,
                    "kind": format!("{:?}", ev.kind),
                    "text": ev.text,
                })
            })
        })
        .collect();
    Ok(serde_json::json!({
        "signals": signals.len(),
        "suspectedMods": suspected_mods,
        "highlights": highlights,
    }))
}

#[tauri::command(rename_all = "camelCase")]
fn update_project_settings(
    path: String,
    minecraft_version: String,
    loader: String,
    loader_version: String,
    java_path: Option<String>,
    memory_mb: u32,
    jvm_args: Vec<String>,
    player_name: Option<String>,
) -> Result<(), String> {
    use tuffbox_core::manifest::{JavaSpec, LoaderKind, LoaderSpec, MinecraftSpec};

    let mut manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;

    let loader_kind = match loader.as_str() {
        "fabric" => LoaderKind::Fabric,
        "forge" => LoaderKind::Forge,
        "neoforge" => LoaderKind::Neoforge,
        "quilt" => LoaderKind::Quilt,
        _ => LoaderKind::Vanilla,
    };

    manifest.minecraft = MinecraftSpec {
        version: minecraft_version,
    };
    manifest.loader = LoaderSpec {
        kind: loader_kind,
        version: loader_version,
    };
    manifest.java = Some(JavaSpec {
        major: manifest.java.as_ref().and_then(|j| j.major),
        distribution: manifest.java.as_ref().and_then(|j| j.distribution.clone()),
        path: java_path,
    });

    if let Some(profile) = manifest.profiles.iter_mut().find(|p| p.id == "client") {
        profile.memory_mb = Some(memory_mb);
        profile.jvm_args = jvm_args;
        profile.player_name = player_name.filter(|name| !name.trim().is_empty());
    }

    let json = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())
}

/// ── Pinning & session state persisted to .tuffbox/data.json ─────────

#[tauri::command(rename_all = "camelCase")]
fn pin_project(path: String, pin: bool) -> Result<(), String> {
    let project_dir = manifest_parent(&path)?;
    let mut state = load_launcher_data(&project_dir);
    let key = path.clone();
    if pin {
        state.pinned.insert(key);
    } else {
        state.pinned.remove(&key);
    }
    save_launcher_data(&project_dir, &state)
}

#[tauri::command(rename_all = "camelCase")]
fn is_project_pinned(path: String) -> Result<bool, String> {
    let project_dir = manifest_parent(&path)?;
    let state = load_launcher_data(&project_dir);
    Ok(state.pinned.contains(&path))
}

#[tauri::command(rename_all = "camelCase")]
fn set_last_opened_project(path: String) -> Result<(), String> {
    let resolved = resolve_manifest_path(&path)?;
    let path = resolved.to_string_lossy().to_string();
    let project_dir = resolved
        .parent()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| "manifest has no parent directory".to_string())?;
    let mut state = load_launcher_data(&project_dir);
    state.last_opened = Some(path);
    save_launcher_data(&project_dir, &state)
}

#[tauri::command(rename_all = "camelCase")]
fn get_last_opened_project() -> Result<Option<String>, String> {
    // Scan parent dirs of known projects — simple approach
    let home = dirs::home_dir().unwrap_or_default();
    let tuffbox_dir = home.join("TuffBox");
    let data_path = tuffbox_dir.join(".tuffbox").join("launcher-data.json");
    if data_path.exists() {
        if let Ok(raw) = std::fs::read_to_string(&data_path) {
            if let Ok(state) = serde_json::from_str::<LauncherDataState>(&raw) {
                return Ok(state.last_opened);
            }
        }
    }
    Ok(None)
}

#[tauri::command(rename_all = "camelCase")]
fn load_recent_projects() -> Result<Vec<crate::types::RecentProjectEntry>, String> {
    Ok(helpers::load_recent_projects())
}

#[tauri::command(rename_all = "camelCase")]
fn save_recent_projects(
    projects: Vec<crate::types::RecentProjectEntry>,
) -> Result<(), String> {
    helpers::save_recent_projects(&projects)
}

#[tauri::command(rename_all = "camelCase")]
fn get_home_dir() -> Result<String, String> {
    dirs::home_dir()
        .map(|p| p.to_string_lossy().to_string())
        .ok_or_else(|| "home directory not found".to_string())
}

#[tauri::command(rename_all = "camelCase")]
fn create_instance(
    name: String,
    minecraft_version: String,
    loader: String,
    loader_version: String,
    location: String,
    memory_mb: Option<u32>,
    jvm_args: Option<Vec<String>>,
) -> Result<String, String> {
    use tuffbox_core::manifest::{
        JavaSpec, LoaderKind, LoaderSpec, MinecraftSpec, ProfileSpec, ProjectManifest,
        ProjectMetadata, Side,
    };

    let loader_kind = match loader.as_str() {
        "fabric" => LoaderKind::Fabric,
        "forge" => LoaderKind::Forge,
        "neoforge" => LoaderKind::Neoforge,
        "quilt" => LoaderKind::Quilt,
        _ => LoaderKind::Vanilla,
    };

    let id = name
        .to_lowercase()
        .replace(|c: char| !c.is_alphanumeric() && c != '-' && c != '_', "-")
        .replace("--", "-")
        .trim_matches('-')
        .to_string();

    let dir = PathBuf::from(&location);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let mem = memory_mb.unwrap_or(4096).clamp(1024, 65536);
    let args = jvm_args.unwrap_or_default();

    let manifest = ProjectManifest {
        schema_version: "0.1.0".to_string(),
        project: ProjectMetadata {
            id: id.clone(),
            name: name.clone(),
            version: "1.0.0".to_string(),
            description: None,
            authors: Vec::new(),
        },
        minecraft: MinecraftSpec {
            version: minecraft_version,
        },
        loader: LoaderSpec {
            kind: loader_kind,
            version: loader_version,
        },
        brief: None,
        listing: None,
        java: Some(JavaSpec {
            major: Some(17),
            distribution: None,
            path: None,
        }),
        profiles: vec![ProfileSpec {
            id: "client".to_string(),
            name: "Client".to_string(),
            side: Side::Client,
            include_optional_mods: false,
            include_shaders: true,
            memory_mb: Some(mem),
            jvm_args: args,
            include_mods: Vec::new(),
            player_name: None,
        }],
        mods: Vec::new(),
        overrides: None,
    };

    let path = dir.join(format!("{id}.tuffbox.json"));
    let json = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

fn collect_tracked_project_files(
    project_dir: &Path,
    dir: &Path,
    files: &mut Vec<PathBuf>,
) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_symlink() {
            continue;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            collect_tracked_project_files(project_dir, &path, files)?;
            continue;
        }
        if !path.is_file() {
            continue;
        }
        let relative = match path.strip_prefix(project_dir) {
            Ok(rel) => rel.to_path_buf(),
            Err(_) => continue,
        };
        files.push(relative);
    }
    Ok(())
}

fn collect_config_files(
    project_dir: &Path,
    dir: &Path,
    files: &mut Vec<ConfigFileSummary>,
) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_symlink() {
            continue;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            collect_config_files(project_dir, &path, files)?;
            continue;
        }
        if !path.is_file() || !is_editable_config_path(&path) {
            continue;
        }
        let metadata = std::fs::metadata(&path)?;
        if metadata.len() > 2 * 1024 * 1024 {
            continue;
        }
        let relative = path
            .strip_prefix(project_dir)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs());
        files.push(ConfigFileSummary {
            name,
            extension: path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase(),
            path: relative,
            size: metadata.len(),
            modified,
        });
    }
    Ok(())
}

fn mod_change_entries(
    snapshot: &tuffbox_core::Snapshot,
    before: &ProjectManifest,
    after: &ProjectManifest,
) -> Vec<ProjectChangeEntry> {
    let mut entries = Vec::new();
    let before_mods: std::collections::HashMap<_, _> =
        before.mods.iter().map(|m| (m.id.as_str(), m)).collect();
    let after_mods: std::collections::HashMap<_, _> =
        after.mods.iter().map(|m| (m.id.as_str(), m)).collect();

    let actor = if snapshot.plan_source.is_some() {
        pack_events::actor_for_plan_source(snapshot.plan_source.as_deref()).to_string()
    } else {
        pack_events::actor_for_operation(&snapshot.name).to_string()
    };
    let (episode_id, fix_method) = history_episode_fields(
        &snapshot.crash_fingerprint_key,
        &snapshot.plan_source,
        None,
    );

    for (id, module) in &after_mods {
        if !before_mods.contains_key(*id) {
            entries.push(ProjectChangeEntry {
                id: format!("{}:mod-added:{id}", snapshot.id),
                snapshot_id: snapshot.id.clone(),
                operation: snapshot.name.clone(),
                reason: snapshot.reason.clone(),
                created_at: snapshot.created_at.clone(),
                path: "project.tuffbox.json".to_string(),
                category: "Mods".to_string(),
                kind: "mod_added".to_string(),
                preview: format!(
                    "Added {} {} ({:?})",
                    module.name, module.version, module.side
                ),
                diff: format!("+ {} {} ({:?})", module.name, module.version, module.side),
                can_open: false,
                tags: snapshot.tags.clone(),
                crash_fingerprint_key: snapshot.crash_fingerprint_key.clone(),
                plan_source: snapshot.plan_source.clone(),
                actor: actor.clone(),
                op: "mod_added".into(),
                episode_id: episode_id.clone(),
                fix_method: fix_method.clone(),
                log_path: None,
            });
        }
    }

    for (id, module) in &before_mods {
        if !after_mods.contains_key(*id) {
            entries.push(ProjectChangeEntry {
                id: format!("{}:mod-removed:{id}", snapshot.id),
                snapshot_id: snapshot.id.clone(),
                operation: snapshot.name.clone(),
                reason: snapshot.reason.clone(),
                created_at: snapshot.created_at.clone(),
                path: "project.tuffbox.json".to_string(),
                category: "Mods".to_string(),
                kind: "mod_removed".to_string(),
                preview: format!(
                    "Removed {} {} ({:?})",
                    module.name, module.version, module.side
                ),
                diff: format!("- {} {} ({:?})", module.name, module.version, module.side),
                can_open: false,
                tags: snapshot.tags.clone(),
                crash_fingerprint_key: snapshot.crash_fingerprint_key.clone(),
                plan_source: snapshot.plan_source.clone(),
                actor: actor.clone(),
                op: "mod_removed".into(),
                episode_id: episode_id.clone(),
                fix_method: fix_method.clone(),
                log_path: None,
            });
        }
    }

    for (id, before_module) in &before_mods {
        let Some(after_module) = after_mods.get(*id) else {
            continue;
        };
        if before_module.version != after_module.version
            || before_module.file_name != after_module.file_name
            || before_module.side != after_module.side
        {
            entries.push(ProjectChangeEntry {
                id: format!("{}:mod-updated:{id}", snapshot.id),
                snapshot_id: snapshot.id.clone(),
                operation: snapshot.name.clone(),
                reason: snapshot.reason.clone(),
                created_at: snapshot.created_at.clone(),
                path: "project.tuffbox.json".to_string(),
                category: "Mods".to_string(),
                kind: "mod_updated".to_string(),
                preview: format!(
                    "Updated {}: {} → {}",
                    after_module.name, before_module.version, after_module.version
                ),
                diff: format!(
                    "- {} {} ({:?})\n+ {} {} ({:?})",
                    before_module.name,
                    before_module.version,
                    before_module.side,
                    after_module.name,
                    after_module.version,
                    after_module.side
                ),
                can_open: false,
                tags: snapshot.tags.clone(),
                crash_fingerprint_key: snapshot.crash_fingerprint_key.clone(),
                plan_source: snapshot.plan_source.clone(),
                actor: actor.clone(),
                op: "mod_updated".into(),
                episode_id: episode_id.clone(),
                fix_method: fix_method.clone(),
                log_path: None,
            });
        }
    }

    entries
}

fn change_category(path: &str) -> &'static str {
    pack_events::category_for_path(path)
}

fn default_history_settings() -> HistorySettings {
    let mut tracked = std::collections::HashMap::new();
    tracked.insert("Mods".to_string(), true);
    tracked.insert("Configs".to_string(), true);
    tracked.insert("Shaders".to_string(), true);
    tracked.insert("Resource Packs".to_string(), true);
    tracked.insert("World/Data".to_string(), false);
    tracked.insert("Other".to_string(), true);
    HistorySettings {
        tracked,
        focused_scan: false,
    }
}

fn diff_preview(diff: &str) -> String {
    let lines = diff
        .lines()
        .filter(|line| line.starts_with("+ ") || line.starts_with("- "))
        .take(8)
        .collect::<Vec<_>>();
    if lines.is_empty() {
        diff.lines().take(6).collect::<Vec<_>>().join("\n")
    } else {
        lines.join("\n")
    }
}

fn remove_mod_file_from_disk(manifest_path: &Path, removed_mod: &ModSpec) {
    if let Some(file_name) = &removed_mod.file_name {
        if let Some(instance_dir) = tuffbox_core::instance_dir_for_manifest(manifest_path) {
            let content_dir =
                tuffbox_core::content_dir_for(&instance_dir, removed_mod.content_type);
            let live = content_dir.join(file_name);
            let disabled = content_dir.join(format!("{file_name}.disabled"));
            // Hash the content before removal so the shared store object can
            // be released when this was the last pack using these bytes.
            let sha1 = removed_mod
                .hashes
                .as_ref()
                .and_then(|h| h.sha1.clone())
                .or_else(|| tuffbox_core::sha1_file(&live).ok());
            // Dedup-store hardlinks are read-only on Windows — clear before
            // remove or the file silently survives (stale jar on next sync).
            tuffbox_core::fs_util::clear_readonly(&live);
            tuffbox_core::fs_util::clear_readonly(&disabled);
            let _ = std::fs::remove_file(&live);
            let _ = std::fs::remove_file(&disabled);
            if let Some(sha1) = sha1 {
                release_store_object_detached(sha1);
            }
        }
    }
}

fn mod_file_path(manifest_path: &Path, module: &ModSpec) -> Option<PathBuf> {
    let file_name = module.file_name.as_ref()?;
    let instance_dir = tuffbox_core::instance_dir_for_manifest(manifest_path)?;
    Some(tuffbox_core::content_dir_for(&instance_dir, module.content_type).join(file_name))
}

fn existing_mod_file_path(manifest_path: &Path, module: &ModSpec) -> Option<PathBuf> {
    let normal = mod_file_path(manifest_path, module)?;
    if normal.is_file() {
        return Some(normal);
    }
    let file_name = normal.file_name()?.to_string_lossy();
    let disabled = normal.with_file_name(format!("{file_name}.disabled"));
    disabled.is_file().then_some(disabled)
}

/// Prefer a human display name over Modrinth/CF provider ids.
fn mod_display_name(module: &ModSpec) -> String {
    let name = module.name.trim();
    if !name.is_empty() && !looks_like_provider_id(name) {
        return name.to_string();
    }
    let id = module.id.trim();
    if !id.is_empty() && !looks_like_provider_id(id) {
        return id.to_string();
    }
    if let Some(file) = module.file_name.as_deref().filter(|s| !s.is_empty()) {
        return file.trim_end_matches(".jar").trim_end_matches(".disabled").to_string();
    }
    if !name.is_empty() {
        return name.to_string();
    }
    if !id.is_empty() {
        return id.to_string();
    }
    "Unknown mod".into()
}

/// Modrinth project ids are typically 8-char base62; CF numeric ids are all digits.
fn looks_like_provider_id(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() {
        return false;
    }
    if s.chars().all(|c| c.is_ascii_digit()) && s.len() >= 4 {
        return true;
    }
    s.len() == 8 && s.chars().all(|c| c.is_ascii_alphanumeric())
}

fn mod_matches_ref(module: &ModSpec, id: &str) -> bool {
    module.id == id
        || module.source.project_id.as_deref() == Some(id)
        || module.file_name.as_deref() == Some(id)
}

fn mod_history_line(verb: &str, module: &ModSpec) -> String {
    let name = mod_display_name(module);
    if module.version.trim().is_empty() {
        format!("{verb} {name}")
    } else {
        format!("{verb} {name} {}", module.version)
    }
}

/// Human-readable install summary: primary mods first, then dependencies.
fn mod_install_history_lines(related: &[&ModSpec], requested_ids: &[String]) -> Vec<String> {
    if related.is_empty() {
        return vec!["Install mod(s)".into()];
    }
    let (primaries, deps): (Vec<&ModSpec>, Vec<&ModSpec>) = related.iter().copied().partition(|m| {
        requested_ids.iter().any(|id| mod_matches_ref(m, id))
    });
    let primaries = if primaries.is_empty() {
        related.to_vec()
    } else {
        primaries
    };
    let mut lines: Vec<String> = primaries
        .iter()
        .map(|m| mod_history_line("Install", m))
        .collect();
    if !deps.is_empty() && primaries.len() < related.len() {
        if deps.len() <= 4 {
            for m in &deps {
                lines.push(mod_history_line("Install dependency", m));
            }
        } else {
            let names: Vec<String> = deps.iter().take(3).map(|m| mod_display_name(m)).collect();
            lines.push(format!(
                "Install {} dependencies ({}, …)",
                deps.len(),
                names.join(", ")
            ));
        }
    }
    lines
}

/// Rewrite legacy History text that still embeds raw Modrinth/CF ids.
fn humanize_history_summary(text: &str, mods: &[ModSpec]) -> String {
    if text.is_empty() || mods.is_empty() {
        return text.to_string();
    }
    let mut out = text.to_string();
    for module in mods {
        let name = mod_display_name(module);
        if let Some(pid) = module.source.project_id.as_deref() {
            if looks_like_provider_id(pid) && out.contains(pid) {
                out = out.replace(pid, &name);
            }
        }
        if module.id != name && looks_like_provider_id(&module.id) && out.contains(&module.id) {
            out = out.replace(&module.id, &name);
        }
    }
    out.replace("Install CurseForge ", "Install ")
}

fn humanize_history_op(op: &str) -> String {
    match op {
        "mod_change" => "Mod change".into(),
        "mod_added" => "Mod added".into(),
        "mod_removed" => "Mod removed".into(),
        "external_add" => "External add".into(),
        "external_edit" => "External edit".into(),
        "external_remove" => "External remove".into(),
        "file_edit" => "File edit".into(),
        "crash_detected" => "Crash detected".into(),
        "crash_fix" => "Crash fix".into(),
        "crash_resolved" => "Crash resolved".into(),
        "snapshot" => "Snapshot".into(),
        "rollback" => "Rollback".into(),
        other => other.replace('_', " "),
    }
}

/// After a launcher mod install/remove, write concrete History events and keep
/// the disk baseline in sync so the next scan does not flag launcher jars as
/// external adds/removes.
fn finalize_mod_history(
    manifest_path: &Path,
    snapshot: &mut tuffbox_core::Snapshot,
    operation: &str,
    action_lines: &[String],
    related_mods: &[&ModSpec],
    removed_rel_paths: &[String],
) {
    let Some(project_dir) = manifest_path.parent() else {
        return;
    };
    if !action_lines.is_empty() {
        snapshot.actions_summary = action_lines.to_vec();
        let store = SnapshotStore::new(project_dir);
        let _ = store.update_meta(snapshot);
    }

    let mut paths: Vec<String> = Vec::new();
    for module in related_mods {
        if let Some(abs) = existing_mod_file_path(manifest_path, module) {
            if let Ok(rel) = abs.strip_prefix(project_dir) {
                paths.push(rel.to_string_lossy().replace('\\', "/"));
            }
        } else if let Some(name) = module.file_name.as_deref() {
            paths.push(format!("mods/{name}"));
        }
    }
    for rel in removed_rel_paths {
        paths.push(rel.replace('\\', "/"));
    }
    paths.sort();
    paths.dedup();

    let mod_ids: Vec<String> = related_mods.iter().map(|m| m.id.clone()).collect();
    let _ = pack_events::record_mod_change_event(
        project_dir,
        operation,
        Some(&snapshot.id),
        action_lines,
        &paths,
        &mod_ids,
    );

    // If a crash episode is open, accumulate Content/UI mod ops into the share trail.
    let mod_refs: Vec<(String, Option<String>)> = related_mods
        .iter()
        .map(|m| (m.id.clone(), Some(m.version.clone()).filter(|v| !v.is_empty())))
        .collect();
    let _ = swarm_api::note_player_mod_actions_on_open_marker(
        manifest_path,
        operation,
        &mod_refs,
        Some(snapshot.id.as_str()),
    );
}

/// Removes jars superseded by an update: the previous filename and any file
/// whose sha1 still matches the pre-update artifact. Filename-only cleanup
/// misses Modrinth renames (`mod-1.0.0.jar` → `mod-1.0.1.jar`) when the
/// manifest path was already out of sync with disk.
fn remove_superseded_mod_files(
    manifest_path: &Path,
    old_mod: &ModSpec,
    new_mod: &ModSpec,
    tracked_bases: &std::collections::HashSet<String>,
) {
    let Some(instance_dir) = tuffbox_core::instance_dir_for_manifest(manifest_path) else {
        return;
    };
    let content_dir = tuffbox_core::content_dir_for(&instance_dir, new_mod.content_type);
    let old = superseded_cleanup::SupersededOld {
        id: &old_mod.id,
        file_name: old_mod.file_name.as_deref(),
        sha1: old_mod.hashes.as_ref().and_then(|h| h.sha1.as_deref()),
    };
    superseded_cleanup::remove_superseded_in_dir(
        &content_dir,
        &old,
        new_mod.file_name.as_deref(),
        tracked_bases,
    );
}

fn refresh_modrinth_file_metadata(
    manifest: &ProjectManifest,
    module: &mut ModSpec,
) -> Result<(), String> {
    if module.source.kind != SourceKind::Modrinth && module.source.project_id.is_none() {
        return Ok(());
    }
    let Some(version_id) = module.source.file_id.clone() else {
        return Ok(());
    };
    let provider = tuffbox_core::ModrinthProvider::new();
    let version = provider
        .get_version(&version_id)
        .map_err(|e| format!("failed to refresh {} from Modrinth: {e}", module.name))?;
    let loader_slug = tuffbox_core::graph::loader_kind_slug(&manifest.loader.kind);
    // Loader must match so we don't install a Forge jar into a Fabric instance.
    // Minecraft mismatch is allowed for intentional cross-version switches from
    // the version picker (user confirms incompatible installs in the UI).
    if !version
        .loaders
        .iter()
        .any(|loader| loader == loader_slug || (loader_slug == "quilt" && loader == "fabric"))
    {
        return Err(format!(
            "{} update has no build for loader {loader_slug} (supports [{}])",
            module.name,
            version.loaders.join(", ")
        ));
    }
    let file =
        ProviderFileInfo::select_file_for_loader(&version, loader_slug).ok_or_else(|| {
            format!(
                "{} update has no downloadable file for loader {loader_slug}",
                module.name
            )
        })?;
    module.source.url = Some(file.url.clone());
    module.file_name = Some(file.filename.clone());
    module.version = version.version_number.clone();
    module.hashes = Some(tuffbox_core::FileHashes {
        sha1: file.hashes.sha1.clone(),
        sha512: file.hashes.sha512.clone(),
    });
    Ok(())
}

/// Downloads and verifies a single replacement before publishing its
/// manifest entry. The previous file is kept until both operations succeed,
/// so a network/hash/manifest error cannot leave metadata pointing at bytes
/// that were never installed.
fn commit_single_mod_update(
    app: &tauri::AppHandle,
    manifest_path: &Path,
    updated_manifest: &mut ProjectManifest,
    old_mod: &ModSpec,
    emit_lifecycle: bool,
) -> Result<tuffbox_core::ModSyncReport, String> {
    let project_id = old_mod.source.project_id.as_deref();
    let mut new_mod = updated_manifest
        .mods
        .iter()
        .find(|module| {
            module.id == old_mod.id
                || (project_id.is_some() && module.source.project_id.as_deref() == project_id)
        })
        .ok_or_else(|| format!("updated mod {} disappeared from manifest", old_mod.id))?
        .clone();
    refresh_modrinth_file_metadata(updated_manifest, &mut new_mod)?;
    if let Some(idx) = updated_manifest
        .mods
        .iter()
        .position(|module| module.id == new_mod.id)
    {
        updated_manifest.mods[idx] = new_mod.clone();
    }
    let old_path = existing_mod_file_path(manifest_path, old_mod);
    let new_path = mod_file_path(manifest_path, &new_mod)
        .ok_or_else(|| format!("updated mod {} has no destination file", new_mod.id))?;

    // Prefer the on-disk hash for cleanup — manifest metadata may already
    // disagree with the jar after a partial prior update.
    let mut old_for_cleanup = old_mod.clone();
    if let Some(path) = old_path.as_ref() {
        if let Ok(hash) = tuffbox_core::sha1_file(path) {
            old_for_cleanup.hashes = Some(tuffbox_core::FileHashes {
                sha1: Some(hash),
                sha512: old_mod
                    .hashes
                    .as_ref()
                    .and_then(|h| h.sha512.clone()),
            });
        }
        if let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) {
            let base = name.strip_suffix(".disabled").unwrap_or(&name).to_string();
            old_for_cleanup.file_name = Some(base);
        }
    }

    let backup = if let Some(path) = old_path.as_ref() {
        let parent = path
            .parent()
            .ok_or_else(|| format!("invalid mod path {}", path.display()))?;
        let staged = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        std::fs::copy(path, staged.path())
            .map_err(|e| format!("failed to preserve {} before update: {e}", path.display()))?;
        Some(staged)
    } else {
        None
    };

    let restore = || {
        if old_path.as_ref() != Some(&new_path) {
            let _ = std::fs::remove_file(&new_path);
        }
        if let (Some(path), Some(staged)) = (old_path.as_ref(), backup.as_ref()) {
            let _ = std::fs::copy(staged.path(), path);
        }
    };

    let mut download_manifest = updated_manifest.clone();
    download_manifest.mods = vec![new_mod.clone()];
    let only_mod = std::collections::HashSet::from([new_mod.id.clone()]);
    let report = download_project_mods_tracked(
        app,
        manifest_path,
        &download_manifest,
        Some(&only_mod),
        emit_lifecycle,
    );
    if let Some(failure) = report
        .failed
        .iter()
        .find(|failure| failure.mod_id == new_mod.id)
    {
        restore();
        return Err(format!(
            "failed to update {}: {}",
            new_mod.name, failure.error
        ));
    }

    if let Err(error) = save_manifest(manifest_path, updated_manifest) {
        restore();
        return Err(format!(
            "downloaded {}, but could not save manifest: {error}",
            new_mod.name
        ));
    }

    // Base file names of all other tracked mods — never removed by cleanup.
    let tracked_bases: std::collections::HashSet<String> = updated_manifest
        .mods
        .iter()
        .filter(|m| m.id != new_mod.id)
        .filter_map(|m| m.file_name.as_deref().map(strip_disabled_suffix))
        .collect();
    remove_superseded_mod_files(manifest_path, &old_for_cleanup, &new_mod, &tracked_bases);
    Ok(report)
}

/// Strip a trailing `.disabled` from a jar name, if present.
fn strip_disabled_suffix(name: &str) -> String {
    name.strip_suffix(".disabled").unwrap_or(name).to_string()
}

fn apply_change_action(
    manifest_path: &Path,
    manifest: &mut ProjectManifest,
    action: tuffbox_core::ChangeAction,
    applied: &mut Vec<String>,
) -> Result<(), String> {
    match action {
        tuffbox_core::ChangeAction::InstallMod { project_id, .. } => {
            // Soft-fail: missing Modrinth builds (e.g. celestial on 1.21) must not
            // abort the rest of a resolve plan or wipe the graph UI.
            match add_mod_from_modrinth(manifest, &project_id, Some("auto".to_string())) {
                Ok(()) => applied.push(format!("installed {project_id}")),
                Err(e) => applied.push(format!("skipped {project_id}: {e}")),
            }
        }
        tuffbox_core::ChangeAction::RemoveMod { node_id } => {
            let mod_id = node_id
                .0
                .strip_prefix("mod:")
                .unwrap_or(&node_id.0)
                .to_string();
            let removed_mod = manifest.mods.iter().find(|m| m.id == mod_id).cloned();
            let before = manifest.mods.len();
            manifest.mods.retain(|m| m.id != mod_id);
            if manifest.mods.len() != before {
                if let Some(removed_mod) = removed_mod {
                    remove_mod_file_from_disk(manifest_path, &removed_mod);
                }
                applied.push(format!("removed {mod_id}"));
            }
        }
        tuffbox_core::ChangeAction::DisableMod { node_id } => {
            let mod_id = node_id
                .0
                .strip_prefix("mod:")
                .unwrap_or(&node_id.0)
                .to_string();
            if let Some(module) = manifest.mods.iter_mut().find(|m| m.id == mod_id) {
                if let Some(file_name) = module.file_name.clone() {
                    if let Some(instance_dir) =
                        tuffbox_core::instance_dir_for_manifest(manifest_path)
                    {
                        let content_dir =
                            tuffbox_core::content_dir_for(&instance_dir, module.content_type);
                        let active = content_dir.join(&file_name);
                        let disabled = content_dir.join(format!("{file_name}.disabled"));
                        if active.is_file() {
                            let _ = std::fs::rename(&active, &disabled);
                        }
                    }
                }
                if !module
                    .status
                    .iter()
                    .any(|s| s.eq_ignore_ascii_case("disabled"))
                {
                    module.status.push("disabled".to_string());
                }
                applied.push(format!("disabled {mod_id}"));
            }
        }
        tuffbox_core::ChangeAction::UpdateMod {
            node_id,
            target_version,
        } => {
            let mod_id = node_id
                .0
                .strip_prefix("mod:")
                .unwrap_or(&node_id.0)
                .to_string();
            let target_version = target_version.trim();
            let version_id = if target_version.is_empty() || target_version == "latest-compatible" {
                None
            } else {
                Some(target_version)
            };
            update_mod_from_modrinth(manifest_path, manifest, &mod_id, version_id)
                .map_err(|e| e.to_string())?;
            applied.push(format!("updated {mod_id}"));
        }
        tuffbox_core::ChangeAction::EditConfig { path, patch } => {
            let envelope: serde_json::Value = serde_json::from_str(&patch).unwrap_or_else(|_| {
                serde_json::json!({
                    "patchType": "replace_file",
                    "patch": patch,
                })
            });
            let patch_type = envelope
                .get("patchType")
                .and_then(|v| v.as_str())
                .unwrap_or("replace_file");
            let patch_value = envelope
                .get("patch")
                .cloned()
                .unwrap_or(serde_json::Value::String(patch.clone()));
            let project_dir = manifest_path
                .parent()
                .ok_or_else(|| "manifest has no parent".to_string())?;
            let target = safe_project_file(project_dir, &path)?;
            let current = if target.is_file() {
                std::fs::read_to_string(&target).map_err(|e| e.to_string())?
            } else {
                String::new()
            };
            let new_content = tuffbox_core::action_plan::apply_config_patch(
                &current,
                &path,
                patch_type,
                &patch_value,
            )?;
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            // Atomic write + .bak-<ts> backup (Reality Launcher-style).
            tuffbox_core::fs_util::atomic_write_with_backup(&target, new_content.as_bytes())
                .map_err(|e| e.to_string())?;
            applied.push(format!("edited config {path}"));
        }
    }
    Ok(())
}

#[derive(serde::Serialize, Clone)]
struct DepPlanEntry {
    target: String,
    name: Option<String>,
    depth: usize,
}

/// Recursively resolves the full `Requires` dependency tree that
/// `install_modrinth_with_dependencies_rounds` would auto-install for the given
/// seeds, WITHOUT downloading or mutating the manifest. Used to let the user
/// review and deselect dependencies before installing.
fn plan_modrinth_dependencies(
    manifest: &ProjectManifest,
    seed_ids: &[String],
) -> Vec<DepPlanEntry> {
    let mut working = manifest.clone();
    let failed: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut entries: Vec<DepPlanEntry> = Vec::new();

    for seed in seed_ids {
        if working
            .mods
            .iter()
            .any(|m| m.id == *seed || m.source.project_id.as_deref() == Some(seed.as_str()))
        {
            continue;
        }
        let _ = add_mod_from_modrinth(&mut working, seed, Some("auto".to_string()));
    }

    for depth in 0..50 {
        let missing: Vec<String> = working
            .mods
            .iter()
            .flat_map(|m| m.dependencies.iter())
            .filter(|dep| dep.kind == tuffbox_core::DependencyKind::Requires)
            .map(|dep| dep.target.clone())
            .filter(|t| {
                !manifest_has_dependency_target(&working, t)
                    && !seen.contains(t)
                    && !failed.contains(t)
            })
            .collect();
        if missing.is_empty() {
            break;
        }
        for t in missing {
            seen.insert(t.clone());
            let _ = add_mod_from_modrinth(&mut working, &t, Some("auto".to_string()));
            let name = working
                .mods
                .iter()
                .find(|m| m.id == t || m.source.project_id.as_deref() == Some(t.as_str()))
                .map(|m| m.name.clone());
            entries.push(DepPlanEntry {
                target: t.clone(),
                name,
                depth: depth + 1,
            });
        }
    }
    entries
}

#[tauri::command(rename_all = "camelCase")]
async fn resolve_install_dependencies(
    path: String,
    mod_id: String,
) -> Result<Vec<DepPlanEntry>, String> {
    tokio::task::spawn_blocking(move || {
        let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
        Ok(plan_modrinth_dependencies(&manifest, &[mod_id]))
    })
    .await
    .map_err(|e| e.to_string())?
}

fn install_modrinth_with_dependencies(
    manifest: &mut ProjectManifest,
    mod_ids: &[String],
    side: &str,
    explicit_deps: Option<&[String]>,
) -> Result<Vec<String>, String> {
    install_modrinth_with_dependencies_rounds(manifest, mod_ids, side, 50, explicit_deps)
}

pub(crate) fn install_modrinth_with_dependencies_rounds(
    manifest: &mut ProjectManifest,
    mod_ids: &[String],
    side: &str,
    max_rounds: usize,
    explicit_deps: Option<&[String]>,
) -> Result<Vec<String>, String> {
    let mut installed = Vec::new();
    let mut primary_errors: Vec<String> = Vec::new();
    for mod_id in mod_ids {
        if manifest
            .mods
            .iter()
            .any(|m| m.id == *mod_id || m.source.project_id.as_deref() == Some(mod_id.as_str()))
        {
            continue;
        }
        match add_mod_from_modrinth(manifest, mod_id, Some(side.to_string())) {
            Ok(()) => installed.push(mod_id.clone()),
            Err(e) => primary_errors.push(format!("{mod_id}: {e}")),
        }
    }
    if installed.is_empty() && !mod_ids.is_empty() && !primary_errors.is_empty() {
        return Err(primary_errors.join("; "));
    }

    if let Some(deps) = explicit_deps {
        // User-reviewed subset: install exactly the listed targets, no recursion.
        let mut failed = std::collections::HashSet::new();
        for dependency_id in deps {
            if manifest_has_dependency_target(manifest, dependency_id)
                || failed.contains(dependency_id)
            {
                continue;
            }
            match add_mod_from_modrinth(manifest, dependency_id, Some("auto".to_string())) {
                Ok(()) => installed.push(dependency_id.clone()),
                Err(_) => {
                    failed.insert(dependency_id.clone());
                }
            }
        }
        return Ok(installed);
    }

    let mut failed = std::collections::HashSet::new();
    for _ in 0..max_rounds {
        let missing = manifest
            .mods
            .iter()
            .flat_map(|module| module.dependencies.iter())
            .filter(|dep| dep.kind == tuffbox_core::DependencyKind::Requires)
            .map(|dep| dep.target.clone())
            .filter(|target| {
                !manifest_has_dependency_target(manifest, target) && !failed.contains(target)
            })
            .collect::<Vec<_>>();

        if missing.is_empty() {
            break;
        }

        for dependency_id in missing {
            match add_mod_from_modrinth(manifest, &dependency_id, Some("auto".to_string())) {
                Ok(()) => installed.push(dependency_id),
                Err(_) => {
                    failed.insert(dependency_id);
                }
            }
        }
    }

    Ok(installed)
}

fn add_mod_from_modrinth(
    manifest: &mut ProjectManifest,
    mod_id: &str,
    side: Option<String>,
) -> anyhow::Result<()> {
    let provider = tuffbox_core::ModrinthProvider::new();
    let project = provider.get_project(mod_id)?;

    if manifest.mods.iter().any(|m| {
        m.id == project.slug || m.source.project_id.as_deref() == Some(project.id.as_str())
    }) {
        anyhow::bail!("mod {} is already in the project", project.slug);
    }

    let query = ProviderSearchQuery {
        query: None,
        minecraft_version: Some(manifest.minecraft.version.clone()),
        loader: Some(tuffbox_core::graph::loader_kind_slug(&manifest.loader.kind).to_string()),
        ..Default::default()
    };
    let loader = tuffbox_core::graph::loader_kind_slug(&manifest.loader.kind);
    let mc = manifest.minecraft.version.as_str();
    let versions = provider.get_versions(mod_id, &query)?;
    let version = versions.into_iter().next().ok_or_else(|| {
        anyhow::anyhow!("{mod_id}: no Modrinth build for Minecraft {mc} / {loader}")
    })?;

    let file = ProviderFileInfo::select_file_for_loader(
        &version,
        &tuffbox_core::graph::loader_kind_slug(&manifest.loader.kind),
    )
    .cloned()
    .ok_or_else(|| anyhow::anyhow!("no primary file for version {}", version.id))?;

    let dependencies = provider.resolve_dependencies(&version.id)?;
    let mod_side = parse_side(side.as_deref(), Some(&project));
    let mod_spec = build_mod_spec(&project, &version, file, dependencies, mod_side);
    manifest.mods.push(mod_spec);
    Ok(())
}

/// Adds the **exact pinned version** of a Modrinth mod (unlike
/// [`add_mod_from_modrinth`], which always resolves the latest compatible
/// build). Used by the Optimize FO flow where every mod version comes from
/// the FO `modrinth.index.json`.
fn add_mod_from_modrinth_pinned(
    manifest: &mut ProjectManifest,
    project_id: &str,
    version_id: &str,
    side: Option<String>,
) -> anyhow::Result<()> {
    let provider = tuffbox_core::ModrinthProvider::new();
    let project = provider.get_project(project_id)?;

    if manifest.mods.iter().any(|m| {
        m.id == project.slug || m.source.project_id.as_deref() == Some(project.id.as_str())
    }) {
        anyhow::bail!("mod {} is already in the project", project.slug);
    }

    let version = provider.get_version(version_id)?;
    if version.project_id != project.id {
        anyhow::bail!(
            "version {version_id} does not belong to project {}",
            project.slug
        );
    }

    let file = ProviderFileInfo::select_file_for_loader(
        &version,
        &tuffbox_core::graph::loader_kind_slug(&manifest.loader.kind),
    )
    .cloned()
    .ok_or_else(|| anyhow::anyhow!("no primary file for version {}", version.id))?;

    let dependencies = provider.resolve_dependencies(&version.id)?;
    let mod_side = parse_side(side.as_deref(), Some(&project));
    let mod_spec = build_mod_spec(&project, &version, file, dependencies, mod_side);
    manifest.mods.push(mod_spec);
    Ok(())
}

pub(crate) fn add_mod_from_curseforge(
    manifest: &mut ProjectManifest,
    mod_id: &str,
    side: Option<String>,
) -> anyhow::Result<()> {
    let project_id: u64 = mod_id
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid CurseForge project id: {mod_id}"))?;
    let provider = tuffbox_core::CurseForgeProvider::new();
    if !provider.is_configured() {
        anyhow::bail!("CurseForge API key is not configured");
    }
    let hit = provider.get_mod(project_id)?;
    let project_id_str = project_id.to_string();
    let slug = if hit.slug.is_empty() {
        format!("cf-{project_id}")
    } else {
        hit.slug.clone()
    };
    if manifest.mods.iter().any(|m| {
        m.id == slug
            || m.source.project_id.as_deref() == Some(mod_id)
            || m.source.project_id.as_deref() == Some(project_id_str.as_str())
    }) {
        anyhow::bail!("mod {slug} is already in the project");
    }

    let loader = tuffbox_core::graph::loader_kind_slug(&manifest.loader.kind).to_string();
    let mc = manifest.minecraft.version.clone();
    let mut files = provider.get_mod_files(project_id, Some(&mc))?;
    if files.is_empty() {
        files = provider.get_mod_files(project_id, None)?;
    }
    let chosen = tuffbox_core::CurseForgeProvider::pick_best_file(&files, &mc, &loader)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("no compatible CurseForge file for {slug}"))?;
    let mut file = provider.get_file(project_id, chosen.id).unwrap_or(chosen);
    if file
        .download_url
        .as_ref()
        .map(|u| u.is_empty())
        .unwrap_or(true)
    {
        let mut map = std::collections::HashMap::from([(file.id, file.clone())]);
        let _ = provider.apply_modrinth_fallback(&mut map);
        if let Some(resolved) = map.remove(&file.id) {
            file = resolved;
        }
    }
    let download_url = file.resolved_download_url().ok_or_else(|| {
        anyhow::anyhow!(
            "CurseForge withheld the download URL for {slug}. Install the file manually or mirror it via Modrinth."
        )
    })?;

    let dependencies = tuffbox_core::provider::curseforge::cf_deps_to_specs(&file.dependencies);
    let content_type = match hit.class_id.unwrap_or(6) {
        12 => tuffbox_core::manifest::ContentType::Resourcepack,
        6552 => tuffbox_core::manifest::ContentType::Shaderpack,
        6945 => tuffbox_core::manifest::ContentType::Datapack,
        _ => tuffbox_core::manifest::ContentType::Mod,
    };
    let mod_side = parse_side(side.as_deref(), None);
    manifest.mods.push(ModSpec {
        id: slug,
        name: hit.name,
        source: ModSource {
            kind: SourceKind::Curseforge,
            project_id: Some(project_id_str),
            file_id: Some(file.id.to_string()),
            url: Some(download_url),
            path: None,
            icon_url: hit.icon_url,
            categories: hit
                .categories
                .iter()
                .map(|c| tuffbox_core::normalize_mod_category(c))
                .collect(),
        },
        version: file.display_name.clone(),
        file_name: Some(file.file_name),
        hashes: Some(tuffbox_core::FileHashes {
            sha1: file.hashes.sha1,
            sha512: file.hashes.sha512,
        }),
        side: mod_side,
        dependencies,
        status: vec!["ok".to_string()],
        content_type,
        authors: hit.authors.clone(),
    option: None,
    });
    Ok(())
}

pub(crate) fn install_curseforge_with_dependencies_rounds(
    manifest: &mut ProjectManifest,
    mod_ids: &[String],
    side: &str,
    max_rounds: usize,
) -> Result<Vec<String>, String> {
    let mut installed = Vec::new();
    let mut primary_errors: Vec<String> = Vec::new();
    for mod_id in mod_ids {
        let already = manifest.mods.iter().any(|m| {
            m.id == *mod_id
                || m.source.project_id.as_deref() == Some(mod_id.as_str())
        });
        if already {
            continue;
        }
        match add_mod_from_curseforge(manifest, mod_id, Some(side.to_string())) {
            Ok(()) => installed.push(mod_id.clone()),
            Err(e) => primary_errors.push(format!("{mod_id}: {e}")),
        }
    }
    if installed.is_empty() && !mod_ids.is_empty() && !primary_errors.is_empty() {
        return Err(primary_errors.join("; "));
    }

    let mut failed = std::collections::HashSet::new();
    for _ in 0..max_rounds {
        let missing = manifest
            .mods
            .iter()
            .filter(|m| m.source.kind == SourceKind::Curseforge)
            .flat_map(|module| module.dependencies.iter())
            .filter(|dep| dep.kind == tuffbox_core::DependencyKind::Requires)
            .map(|dep| dep.target.clone())
            .filter(|target| {
                !manifest_has_dependency_target(manifest, target) && !failed.contains(target)
            })
            .collect::<Vec<_>>();

        if missing.is_empty() {
            break;
        }

        for dependency_id in missing {
            match add_mod_from_curseforge(manifest, &dependency_id, Some("auto".to_string())) {
                Ok(()) => installed.push(dependency_id),
                Err(_) => {
                    failed.insert(dependency_id);
                }
            }
        }
    }

    Ok(installed)
}

fn update_mod_from_modrinth(
    _manifest_path: &Path,
    manifest: &mut ProjectManifest,
    mod_id: &str,
    version_id: Option<&str>,
) -> anyhow::Result<()> {
    let provider = tuffbox_core::ModrinthProvider::new();
    let index = manifest
        .mods
        .iter()
        .position(|m| m.id == mod_id || m.source.project_id.as_deref() == Some(mod_id))
        .ok_or_else(|| anyhow::anyhow!("mod {mod_id} not found in project"))?;

    let old_mod = manifest.mods[index].clone();
    let project_id = old_mod
        .source
        .project_id
        .clone()
        .unwrap_or_else(|| mod_id.to_string());
    let project = provider.get_project(&project_id)?;

    let version = if let Some(vid) = version_id.filter(|v| !v.trim().is_empty()) {
        // Prefer the exact version from the update check / change plan.
        match provider.get_version(vid) {
            Ok(v) => v,
            Err(_) => {
                // `target_version` may be a version_number rather than an id.
                let query = ProviderSearchQuery {
                    query: None,
                    minecraft_version: Some(manifest.minecraft.version.clone()),
                    loader: Some(
                        tuffbox_core::graph::loader_kind_slug(&manifest.loader.kind).to_string(),
                    ),
                    ..Default::default()
                };
                provider
                    .get_versions(&project_id, &query)?
                    .into_iter()
                    .find(|v| v.version_number == vid || v.id == vid)
                    .ok_or_else(|| anyhow::anyhow!("version {vid} not found for {project_id}"))?
            }
        }
    } else {
        let query = ProviderSearchQuery {
            query: None,
            minecraft_version: Some(manifest.minecraft.version.clone()),
            loader: Some(tuffbox_core::graph::loader_kind_slug(&manifest.loader.kind).to_string()),
            ..Default::default()
        };
        provider
            .get_versions(&project_id, &query)?
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("no compatible version found for {project_id}"))?
    };

    let file = ProviderFileInfo::select_file_for_loader(
        &version,
        &tuffbox_core::graph::loader_kind_slug(&manifest.loader.kind),
    )
    .cloned()
    .ok_or_else(|| anyhow::anyhow!("no primary file for version {}", version.id))?;

    let dependencies = provider
        .resolve_dependencies(&version.id)
        .unwrap_or_else(|_| old_mod.dependencies.clone());
    let mut new_spec = build_mod_spec(
        &project,
        &version,
        file,
        dependencies,
        infer_project_side(Some(&project)),
    );
    // A manifest id is referenced by UI state, dependency edges, progress
    // scopes and history. Keep it stable even if Modrinth changed the slug.
    new_spec.id = old_mod.id;

    manifest.mods[index] = new_spec;
    Ok(())
}

/// Builds a minimal ProjectInfo from an existing ModSpec — used as a
/// fallback when Modrinth project lookup fails during batch updates.
fn project_info_from_mod(module: &ModSpec) -> tuffbox_core::ProjectInfo {
    let project_type = match module.content_type {
        tuffbox_core::manifest::ContentType::Resourcepack => "resourcepack",
        tuffbox_core::manifest::ContentType::Shaderpack => "shader",
        tuffbox_core::manifest::ContentType::Datapack => "datapack",
        tuffbox_core::manifest::ContentType::Mod => "mod",
    };
    tuffbox_core::ProjectInfo {
        id: module
            .source
            .project_id
            .clone()
            .unwrap_or_else(|| module.id.clone()),
        slug: module.id.clone(),
        name: module.name.clone(),
        description: String::new(),
        project_type: project_type.to_string(),
        icon_url: module.source.icon_url.clone(),
        author: None,
        downloads: None,
        follows: None,
        date_modified: None,
        date_created: None,
        categories: Vec::new(),
        license: None,
        client_side: None,
        server_side: None,
        issues_url: None,
        source_url: None,
    }
}

fn build_mod_spec(
    project: &tuffbox_core::ProjectInfo,
    version: &tuffbox_core::VersionInfo,
    file: ProviderFileInfo,
    dependencies: Vec<tuffbox_core::ModDependencySpec>,
    side: Side,
) -> ModSpec {
    ModSpec {
        id: project.slug.clone(),
        name: project.name.clone(),
        source: ModSource {
            kind: SourceKind::Modrinth,
            project_id: Some(project.id.clone()),
            file_id: Some(version.id.clone()),
            url: Some(file.url),
            path: None,
            icon_url: project.icon_url.clone(),
            categories: project.categories.clone(),
        },
        version: version.version_number.clone(),
        file_name: Some(file.filename),
        hashes: Some(tuffbox_core::FileHashes {
            sha1: file.hashes.sha1,
            sha512: file.hashes.sha512,
        }),
        side,
        dependencies,
        status: vec!["ok".to_string()],
        // Route the file into the right instance folder (mods/,
        // resourcepacks/, shaderpacks/, datapacks/) based on what Modrinth
        // actually says this project is, instead of always treating it as
        // a mod jar.
        content_type: tuffbox_core::manifest::ContentType::from_modrinth_project_type(
            &project.project_type,
        ),
        authors: project
            .author
            .as_ref()
            .map(|a| vec![a.clone()])
            .unwrap_or_default(),
        option: None,
    }
}

fn parse_side(side: Option<&str>, project: Option<&tuffbox_core::ProjectInfo>) -> Side {
    match side {
        Some("client") => Side::Client,
        Some("server") => Side::Server,
        Some("both") => Side::Both,
        Some("auto") | None => infer_project_side(project),
        _ => infer_project_side(project),
    }
}

fn infer_project_side(project: Option<&tuffbox_core::ProjectInfo>) -> Side {
    let Some(project) = project else {
        return Side::Unknown;
    };
    Side::from_modrinth(project.client_side.as_deref(), project.server_side.as_deref())
}

/// Downloads every manifest-declared entry that isn't already present with
/// a matching hash into its content-type-appropriate folder (`mods/`,
/// `resourcepacks/`, `shaderpacks/`, `datapacks/`).
///
/// This is called right after any manifest mutation that adds/updates
/// content so the files backing those entries actually exist before the
/// next test launch, instead of only existing as metadata in the manifest.
/// Failures are best-effort: an entry that fails to download still shows up
/// in diagnostics/graph as missing rather than silently blocking the whole
/// manifest write.

/// Side-by-side manifest diff between two snapshots.
/// Returns structured changes: added/removed mods, MC/loader version changes,
/// plus a unified diff of the full manifest JSON.
#[tauri::command(rename_all = "camelCase")]
fn diff_manifest_snapshots(
    project_dir: String,
    from_id: String,
    to_id: String,
) -> Result<serde_json::Value, String> {
    let store = SnapshotStore::new(&project_dir);
    let from_snapshot = store
        .get(&from_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("snapshot {from_id} not found"))?;
    let to_snapshot = store
        .get(&to_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("snapshot {to_id} not found"))?;

    let from_text = std::fs::read_to_string(&from_snapshot.manifest_path).unwrap_or_default();
    let to_text = std::fs::read_to_string(&to_snapshot.manifest_path).unwrap_or_default();
    let from_json: serde_json::Value = serde_json::from_str(&from_text).unwrap_or_default();
    let to_json: serde_json::Value = serde_json::from_str(&to_text).unwrap_or_default();

    let from_mods: std::collections::HashSet<String> = from_json
        .get("mods")
        .and_then(|m| m.as_array())
        .into_iter()
        .flatten()
        .filter_map(|m| m.get("id").and_then(|v| v.as_str()).map(String::from))
        .collect();
    let to_mods: std::collections::HashSet<String> = to_json
        .get("mods")
        .and_then(|m| m.as_array())
        .into_iter()
        .flatten()
        .filter_map(|m| m.get("id").and_then(|v| v.as_str()).map(String::from))
        .collect();
    let added_mods: Vec<_> = to_mods.difference(&from_mods).collect();
    let removed_mods: Vec<_> = from_mods.difference(&to_mods).collect();
    let from_ver = from_json
        .get("minecraft")
        .and_then(|m| m.get("version"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let to_ver = to_json
        .get("minecraft")
        .and_then(|m| m.get("version"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let from_loader = from_json
        .get("loader")
        .and_then(|l| l.get("version"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let to_loader = to_json
        .get("loader")
        .and_then(|l| l.get("version"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    Ok(serde_json::json!({
        "diffText": unified_text_diff(&from_text, &to_text),
        "addedMods": added_mods,
        "removedMods": removed_mods,
        "mcVersionChanged": from_ver != to_ver,
        "fromMcVersion": from_ver,
        "toMcVersion": to_ver,
        "loaderVersionChanged": from_loader != to_loader,
        "fromLoaderVersion": from_loader,
        "toLoaderVersion": to_loader,
    }))
}

#[tauri::command(rename_all = "camelCase")]
fn cosmetics_get_local_profile(player_key: String) -> Result<cosmetics_local::CosmeticsProfile, String> {
    cosmetics_local::load_profile(&player_key)
}

#[tauri::command(rename_all = "camelCase")]
fn cosmetics_save_profile(
    profile: cosmetics_local::CosmeticsProfile,
) -> Result<cosmetics_local::CosmeticsProfile, String> {
    cosmetics_local::save_profile(profile)
}

#[tauri::command(rename_all = "camelCase")]
fn cosmetics_upload_skin(
    player_key: String,
    username: String,
    path: String,
    model: String,
) -> Result<cosmetics_local::CosmeticsProfile, String> {
    cosmetics_local::upload_skin_file(&player_key, &username, &path, &model)
}

#[tauri::command(rename_all = "camelCase")]
fn cosmetics_upload_cape(
    player_key: String,
    username: String,
    path: String,
    animated: bool,
    frame_ms: u32,
    frames: u32,
) -> Result<cosmetics_local::CosmeticsProfile, String> {
    cosmetics_local::upload_cape_file(&player_key, &username, &path, animated, frame_ms, frames)
}

#[tauri::command(rename_all = "camelCase")]
fn cosmetics_set_wings(
    player_key: String,
    username: String,
    wings: Option<String>,
) -> Result<cosmetics_local::CosmeticsProfile, String> {
    cosmetics_local::set_wings(&player_key, &username, wings)
}

#[tauri::command(rename_all = "camelCase")]
fn cosmetics_set_visual_extras(
    player_key: String,
    username: String,
    hat: Option<String>,
    trail: bool,
    jump_circles: bool,
    hit_particles: bool,
    hit_bubbles: bool,
    target_esp: bool,
    kill_effect: bool,
) -> Result<cosmetics_local::CosmeticsProfile, String> {
    cosmetics_local::set_visual_extras(
        &player_key,
        &username,
        hat,
        trail,
        jump_circles,
        hit_particles,
        hit_bubbles,
        target_esp,
        kill_effect,
    )
}

#[tauri::command(rename_all = "camelCase")]
fn cosmetics_wings_catalog() -> Vec<serde_json::Value> {
    cosmetics_local::wings_catalog()
}

#[tauri::command(rename_all = "camelCase")]
fn cosmetics_hat_catalog() -> Vec<serde_json::Value> {
    cosmetics_local::hat_catalog()
}

/// ── Running instance tracking ──────────────────────────────────────


#[tauri::command(rename_all = "camelCase")]
fn list_running_instances(app: tauri::AppHandle) -> Result<Vec<serde_json::Value>, String> {
    let (alive, exited) = tuffbox_core::process::list_running_detailed();
    // Rehydrated PIDs (launcher restart) have no Child wait thread — notify UI
    // when prune / orphan watchers discover they died.
    for g in exited {
        let _ = app.emit(
            "process-exited",
            serde_json::json!({
                "id": g.id,
                "code": serde_json::Value::Null,
            }),
        );
    }
    Ok(alive
        .into_iter()
        .map(|g| {
            serde_json::json!({
                "id": g.id,
                "pid": g.pid,
                "profile": g.profile_id,
                "startedAt": g.started_at,
            })
        })
        .collect())
}

#[tauri::command(rename_all = "camelCase")]
fn kill_running_instance(app: tauri::AppHandle, instance_id: String) -> Result<String, String> {
    let running: Vec<_> = tuffbox_core::process::list_running()
        .into_iter()
        .filter(|process| process.id == instance_id)
        .collect();
    if running.is_empty() {
        return Err(format!("no running instance {instance_id}"));
    }
    for process in &running {
        emit_launch_phase(
            &app,
            &process.id,
            &process.profile_id,
            LaunchPhase::Stopping,
            "Stopping game…",
            Some(&process.log_path),
            Some(process.pid),
            Some(process.started_at),
            None,
            true,
            None,
        );
    }
    let count = match tuffbox_core::process::kill_instance(&instance_id) {
        Ok(count) => count,
        Err(error) => {
            // Restore the lifecycle if the OS rejected the signal. Without
            // this, a failed Stop could leave every Play surface stuck on
            // "Stopping…" until the next app restart.
            for process in &running {
                emit_launch_phase(
                    &app,
                    &process.id,
                    &process.profile_id,
                    LaunchPhase::Running,
                    "Game is still running.",
                    Some(&process.log_path),
                    Some(process.pid),
                    Some(process.started_at),
                    None,
                    false,
                    None,
                );
            }
            return Err(error.to_string());
        }
    };
    Ok(format!("Stopping {count} process(es) for {instance_id}"))
}

/// Cached sysinfo sampler so successive polls get real CPU deltas (no sleep).
fn live_sys() -> std::sync::MutexGuard<'static, sysinfo::System> {
    static SYS: once_cell::sync::Lazy<std::sync::Mutex<sysinfo::System>> =
        once_cell::sync::Lazy::new(|| std::sync::Mutex::new(sysinfo::System::new()));
    SYS.lock().unwrap_or_else(|e| e.into_inner())
}

#[tauri::command(rename_all = "camelCase")]
fn get_live_debug_stats(instance_id: Option<String>) -> Result<LiveDebugStats, String> {
    use sysinfo::{Pid, ProcessesToUpdate};

    let mut sys = live_sys();
    sys.refresh_memory();
    sys.refresh_cpu_all();

    let host_cpu_percent = sys.global_cpu_usage();
    let host_memory_used_mb = sys.used_memory() / (1024 * 1024);
    let host_memory_total_mb = sys.total_memory() / (1024 * 1024);

    let Some(id) = instance_id.filter(|s| !s.is_empty()) else {
        return Ok(LiveDebugStats {
            host_cpu_percent,
            host_memory_used_mb,
            host_memory_total_mb,
            instance: None,
        });
    };

    let want = id.replace('\\', "/").trim_end_matches('/').to_ascii_lowercase();
    let tracked = tuffbox_core::process::list_running().into_iter().find(|g| {
        g.id.replace('\\', "/")
            .trim_end_matches('/')
            .to_ascii_lowercase()
            == want
    });

    let Some(proc) = tracked else {
        return Ok(LiveDebugStats {
            host_cpu_percent,
            host_memory_used_mb,
            host_memory_total_mb,
            instance: None,
        });
    };

    let pid = Pid::from_u32(proc.pid);
    sys.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);

    let instance = sys.process(pid).map(|p| InstanceLiveStats {
        pid: proc.pid,
        profile: proc.profile_id.clone(),
        started_at: proc.started_at,
        cpu_percent: p.cpu_usage(),
        memory_mb: p.memory() / (1024 * 1024),
        virtual_memory_mb: p.virtual_memory() / (1024 * 1024),
    });

    Ok(LiveDebugStats {
        host_cpu_percent,
        host_memory_used_mb,
        host_memory_total_mb,
        instance,
    })
}

/// Returns true when the mod's jar is missing or its on-disk SHA1 does not
/// match the manifest — i.e. a download is required.
fn mod_needs_download(instance_dir: &Path, module: &ModSpec) -> bool {
    if module.source.kind == SourceKind::Local {
        return false;
    }
    let Some(file_name) = &module.file_name else {
        return false;
    };
    if module.source.url.is_none() {
        return false;
    }
    let target = tuffbox_core::content_dir_for(instance_dir, module.content_type).join(file_name);
    if !target.is_file() {
        return true;
    }
    match module.hashes.as_ref().and_then(|h| h.sha1.as_deref()) {
        Some(expected) => tuffbox_core::sha1_file(&target)
            .map(|actual| !actual.eq_ignore_ascii_case(expected))
            .unwrap_or(true),
        None => false,
    }
}

/// ────────────────────────────────────────────────────────────────────
fn download_project_mods(
    manifest_path: &Path,
    manifest: &ProjectManifest,
) -> tuffbox_core::ModSyncReport {
    let instance_dir =
        tuffbox_core::instance_dir_for_manifest(manifest_path).unwrap_or_else(|| {
            manifest_path
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_default()
        });
    tuffbox_core::ensure_project_mods_downloaded(manifest, &instance_dir)
}

fn emit_mod_update_progress(
    app: &tauri::AppHandle,
    phase: &str,
    message: &str,
    current: usize,
    total: usize,
    percent: u32,
    mod_id: Option<&str>,
) {
    use tauri::Emitter;
    let _ = app.emit(
        "mod-update-progress",
        ModUpdateProgressPayload {
            phase: phase.to_string(),
            message: message.to_string(),
            current,
            total,
            percent: percent.min(100),
            mod_id: mod_id.map(str::to_string),
        },
    );
}

fn emit_mod_download_status(
    app: &tauri::AppHandle,
    id: &str,
    name: &str,
    status: &str,
    percent: u32,
) {
    use tauri::Emitter;
    let _ = app.emit(
        "mod-download-progress",
        ModDownloadProgressPayload {
            id: id.to_string(),
            name: name.to_string(),
            downloaded: 0,
            total: 0,
            percent: percent.min(100),
            status: status.to_string(),
        },
    );
}

/// Downloads missing mod files while streaming per-mod byte progress to the
/// frontend via `mod-download-progress` / `mod-download-batch` events and the
/// `DOWNLOAD_PROGRESS` snapshot map.
pub(crate) fn download_project_mods_tracked(
    app: &tauri::AppHandle,
    manifest_path: &Path,
    manifest: &ProjectManifest,
    only_mod_ids: Option<&std::collections::HashSet<String>>,
    emit_lifecycle: bool,
) -> tuffbox_core::ModSyncReport {
    use tauri::Emitter;

    let instance_dir =
        tuffbox_core::instance_dir_for_manifest(manifest_path).unwrap_or_else(|| {
            manifest_path
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_default()
        });

    if let Ok(mut map) = DOWNLOAD_PROGRESS.lock() {
        map.clear();
    }

    let name_map: std::collections::HashMap<String, String> = manifest
        .mods
        .iter()
        .map(|m| (m.id.clone(), m.name.clone()))
        .collect();

    // Only surface mods that actually need a network fetch — already-present
    // jars would otherwise flood the progress UI on every update/install.
    let queue: Vec<ModDownloadProgressPayload> = manifest
        .mods
        .iter()
        .filter(|m| {
            only_mod_ids.map(|ids| ids.contains(&m.id)).unwrap_or(true)
                && mod_needs_download(&instance_dir, m)
        })
        .map(|m| ModDownloadProgressPayload {
            id: m.id.clone(),
            name: m.name.clone(),
            downloaded: 0,
            total: 0,
            percent: 0,
            status: "queued".to_string(),
        })
        .collect();

    let scope_mod_ids: Option<Vec<String>> = only_mod_ids.map(|ids| ids.iter().cloned().collect());

    // Nothing to fetch — still emit a quick start/done so any UI overlay can settle.
    if queue.is_empty() {
        let report = if let Some(ids) = only_mod_ids {
            tuffbox_core::ensure_project_mods_downloaded_with_progress_filtered(
                manifest,
                &instance_dir,
                &tuffbox_core::ProgressCallback::new(),
                Some(ids),
            )
        } else {
            tuffbox_core::ensure_project_mods_downloaded(manifest, &instance_dir)
        };
        if emit_lifecycle {
            let _ = app.emit(
                "mod-download-batch",
                serde_json::json!({
                    "phase": "start",
                    "items": Vec::<ModDownloadProgressPayload>::new(),
                    "scopeModIds": scope_mod_ids,
                }),
            );
            let _ = app.emit(
                "mod-download-batch",
                serde_json::json!({
                    "phase": "done",
                    "downloaded": report.downloaded,
                    "failed": report.failed,
                    "alreadyPresent": report.already_present,
                    "skipped": report.skipped,
                    "scopeModIds": scope_mod_ids,
                    "batchComplete": true,
                }),
            );
        }
        return report;
    }

    if emit_lifecycle {
        let _ = app.emit(
            "mod-download-batch",
            serde_json::json!({
                "phase": "start",
                "items": queue,
                "scopeModIds": scope_mod_ids,
            }),
        );
    }

    let app_for_cb = app.clone();
    let names_for_cb = name_map.clone();
    // Throttle: only emit when percent changes by >= 1 to avoid flooding the UI.
    let last_emitted: std::sync::Mutex<std::collections::HashMap<String, u32>> =
        std::sync::Mutex::new(std::collections::HashMap::new());

    let progress = tuffbox_core::ProgressCallback::with(move |id, done, total| {
        if let Ok(mut map) = DOWNLOAD_PROGRESS.lock() {
            map.insert(id.to_string(), (done, total));
        }
        let percent = if total > 0 {
            ((done as f64 / total as f64) * 100.0).round() as u32
        } else {
            0
        };
        let status = if total > 0 && done >= total {
            "done"
        } else {
            "downloading"
        };

        let should_emit = {
            let mut last = last_emitted.lock().unwrap_or_else(|e| e.into_inner());
            let prev = last.get(id).copied().unwrap_or(u32::MAX);
            if status == "done" || prev == u32::MAX || percent.abs_diff(prev) >= 1 {
                last.insert(id.to_string(), percent);
                true
            } else {
                false
            }
        };

        if should_emit {
            let name = names_for_cb
                .get(id)
                .cloned()
                .unwrap_or_else(|| id.to_string());
            let _ = app_for_cb.emit(
                "mod-download-progress",
                ModDownloadProgressPayload {
                    id: id.to_string(),
                    name,
                    downloaded: done,
                    total,
                    percent,
                    status: status.to_string(),
                },
            );
        }
    });

    let report = tuffbox_core::ensure_project_mods_downloaded_with_progress_filtered(
        manifest,
        &instance_dir,
        &progress,
        only_mod_ids,
    );

    // Mark completed / failed items explicitly so the UI can settle bars.
    for id in &report.downloaded {
        let name = name_map.get(id).cloned().unwrap_or_else(|| id.clone());
        let _ = app.emit(
            "mod-download-progress",
            ModDownloadProgressPayload {
                id: id.clone(),
                name,
                downloaded: 1,
                total: 1,
                percent: 100,
                status: "done".to_string(),
            },
        );
    }
    for id in &report.already_present {
        let name = name_map.get(id).cloned().unwrap_or_else(|| id.clone());
        let _ = app.emit(
            "mod-download-progress",
            ModDownloadProgressPayload {
                id: id.clone(),
                name,
                downloaded: 1,
                total: 1,
                percent: 100,
                status: "skipped".to_string(),
            },
        );
    }
    for fail in &report.failed {
        let name = name_map
            .get(&fail.mod_id)
            .cloned()
            .unwrap_or_else(|| fail.mod_id.clone());
        let _ = app.emit(
            "mod-download-progress",
            ModDownloadProgressPayload {
                id: fail.mod_id.clone(),
                name,
                downloaded: 0,
                total: 0,
                percent: 0,
                status: "failed".to_string(),
            },
        );
    }

    if emit_lifecycle {
        let _ = app.emit(
            "mod-download-batch",
            serde_json::json!({
                "phase": "done",
                "downloaded": report.downloaded,
                "failed": report.failed,
                "alreadyPresent": report.already_present,
                "skipped": report.skipped,
                "scopeModIds": scope_mod_ids,
                "batchComplete": true,
            }),
        );
    }

    if let Ok(mut map) = DOWNLOAD_PROGRESS.lock() {
        map.clear();
    }

    report
}

#[tauri::command(rename_all = "camelCase")]
async fn load_quest_chapter(file_path: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let content = std::fs::read_to_string(&file_path)
            .map_err(|e| format!("Failed to read file: {}", e))?;
        // Content-based format detection: SNBT (<26.1.2.1) or JSON5 (26.1.2.1+).
        let json_value = snbt_parser::parse_quest_file_to_json(&content)?;
        Ok(json_value.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
async fn save_quest_chapter_raw(file_path: String, json_payload: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let _guard = QUEST_IO_LOCK
            .lock()
            .map_err(|_| "quest I/O lock poisoned".to_string())?;
        let value: serde_json::Value = serde_json::from_str(&json_payload)
            .map_err(|e| format!("Invalid JSON payload: {}", e))?;
        // Preserve the on-disk format: existing SNBT chapters stay SNBT,
        // JSON5 chapters (26.1.2.1+) stay JSON5. New files follow the
        // extension (.json5 → JSON5, otherwise SNBT).
        let format = if std::path::Path::new(&file_path).is_file() {
            let existing = std::fs::read_to_string(&file_path).unwrap_or_default();
            snbt_parser::detect_format(&existing)
        } else if file_path.ends_with(".json5") {
            snbt_parser::QuestFileFormat::Json5
        } else {
            snbt_parser::QuestFileFormat::Snbt
        };
        let content = snbt_parser::json_to_quest_file(&value, format)?;
        tuffbox_core::fs_util::atomic_write_with_backup(std::path::Path::new(&file_path), content.as_bytes())
            .map_err(|e| format!("Failed to write quest file: {}", e))?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Serialize chapter JSON to SNBT without writing (same path as save).
#[tauri::command(rename_all = "camelCase")]
async fn preview_quest_chapter_snbt(json_payload: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let value: serde_json::Value = serde_json::from_str(&json_payload)
            .map_err(|e| format!("Invalid JSON payload: {}", e))?;
        Ok(snbt_parser::json_to_snbt(&value))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Read raw chapter SNBT (or any text) from an absolute path.
#[tauri::command(rename_all = "camelCase")]
async fn read_quest_chapter_text(file_path: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        std::fs::read_to_string(&file_path).map_err(|e| format!("Failed to read file: {}", e))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default();

    // TauRPC pilot (typed IPC for TuffSwarm): runs ALONGSIDE the legacy
    // generate_handler! — create_ipc_handler is additive, existing commands
    // keep working. Debug builds also export src/bindings.ts.
    // NOTE: the taurpc macro expands code that touches the tokio reactor
    // (channel/event plumbing), so building the router eagerly on the main
    // thread panics: "there is no reactor running". Enter Tauri's async
    // runtime context first (it lazily initializes the runtime) — the
    // handler itself only *registers* the router; actual invocations run
    // later inside Tauri's runtime.
    {
        // taurpc's macro-expanded router construction touches the tokio
        // reactor, so it must run inside a runtime context. block_on lazily
        // initializes Tauri's global runtime and enters its context.
        let router = tauri::async_runtime::block_on(async { taurpc_api::swarm_api_router() });
        builder = builder.invoke_handler(router.into_handler());
    }

    // Unified structured logging (tauri-plugin-tracing): Rust tracing spans
    // AND JS console output land in one rotating file under the app data dir
    // (logs/tuffbox.YYYY-MM-DD.log, 7 kept). Webview target also forwards
    // Rust logs to the devtools console in dev builds. Game process output
    // stays in per-instance tuffbox-console.log — this is launcher-only.
    {
        let log_dir = dirs::data_local_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("TuffBox")
            .join("logs");
        let _ = std::fs::create_dir_all(&log_dir);
        builder = builder.plugin(
            tauri_plugin_tracing::Builder::new()
                .with_max_level(tauri_plugin_tracing::LevelFilter::INFO)
                .targets([
                    tauri_plugin_tracing::Target::Webview,
                    tauri_plugin_tracing::Target::Folder {
                        path: log_dir,
                        file_name: Some("tuffbox".into()),
                    },
                ])
                .with_rotation(tauri_plugin_tracing::Rotation::Daily)
                .with_rotation_strategy(tauri_plugin_tracing::RotationStrategy::KeepSome(7))
                .build::<tauri::Wry>(),
        );
    }

    // Must be the very first plugin: a second launch hands its argv (the
    // clicked `tuffbox://install?…` link) to the instance already running.
    // Its callback only refocuses the window; URL replay happens inside the
    // deep-link plugin via its `deep-link` feature.
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
        use tauri::Manager;
        if let Some(win) = app.get_webview_window("main") {
            let _ = win.unminimize();
            let _ = win.set_focus();
        }
    }));
    // Registered before `setup`, which reaches for `app.deep_link()`.
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_deep_link::init());
    // Native open/save/message dialogs — required by every
    // `@tauri-apps/plugin-dialog` call from the frontend (skin browse on the
    // Me tab, instance folder pickers, server folder staging, …).
    let builder = builder.plugin(tauri_plugin_dialog::init());

    // Tauri MCP bridge — dev builds only. Lets the OMP `tauri` MCP server drive
    // this running app (webview screenshots, DOM snapshot, element picker, IPC
    // capture, console logs) over the bridge injected into the webview.
    // `withGlobalTauri: true` (required by the bridge) comes from the
    // tauri.dev.conf.json overlay wired into the `tauri:dev` script, so release
    // builds keep the global object off. Permission: `mcp-bridge:default`.
    #[cfg(debug_assertions)]
    let builder = builder.plugin(tauri_plugin_mcp_bridge::init());

    builder
        .setup(|app| {
            parse_launch_cli_args();
            // Task #66: stash the handle for non-command helpers (class-finder
            // cache) that run on the blocking pool without a State param.
            let _ = GLOBAL_APP_HANDLE.set(app.handle().clone());
            // `tuffbox://install?repo=owner/repo` share links.
            #[cfg(desktop)]
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                // Needed in dev and on Linux; a no-op once the installer has
                // registered the scheme for real.
                let _ = app.deep_link().register_all();

                let handle = app.handle().clone();
                app.deep_link().on_open_url(move |event| {
                    for url in event.urls() {
                        deep_link::handle_install_url(&handle, url.as_str());
                    }
                });
                // Cold start: the app was *opened by* the link.
                if let Ok(Some(urls)) = app.deep_link().get_current() {
                    for url in urls {
                        deep_link::handle_install_url(app.handle(), url.as_str());
                    }
                }
            }
            let _ = launcher_settings::load_launcher_settings();
            use tauri::Manager;
            if let Ok(resources) = app.path().resource_dir() {
                std::env::set_var("TUFFBOX_JEI_BRIDGE_DIR", resources.join("jei-bridge"));
                std::env::set_var(
                    "TUFFBOX_MCA_SELECTOR_DIR",
                    resources.join("mca-selector"),
                );
            }
            // Size to ~95%×94% of the monitor's *logical* area
            // (monitor.size() is physical pixels — convert via scale_factor).
            fn fit_to_screen(win: &tauri::WebviewWindow) {
                let Ok(Some(monitor)) = win.current_monitor() else {
                    let _ = win.unminimize();
                    let _ = win.show();
                    let _ = win.set_focus();
                    return;
                };
                let scale = monitor.scale_factor().max(0.5);
                let phys = monitor.size();
                let mw = (phys.width as f64 / scale).max(1.0);
                let mh = (phys.height as f64 / scale).max(1.0);
                let w = (mw * 0.95).clamp(800.0, mw);
                let h = (mh * 0.94).clamp(600.0, mh);
                let _ = win.set_size(tauri::LogicalSize::new(w, h));
                let _ = win.center();
                let _ = win.unminimize();
                let _ = win.show();
                let _ = win.set_focus();
            }
            if let Some(win) = app.get_webview_window("main") {
                fit_to_screen(&win);
            }
            launcher_presence::start_presence_loop();
            let swarm = integrations::swarm_settings();
            if swarm.enabled && swarm.p2p_enabled {
                swarm_node::maybe_start_volunteer_poller();
                swarm_node::maybe_start_creation_poller();
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_project_schema_status,
            migrate_project_schema,
            validate_project,
            resolve_project_path,
            get_project_brief,
            update_project_brief,
            rename_project,
            listing_api::get_project_listing,
            listing_api::update_project_listing,
            listing_api::set_project_listing_icon,
            listing_api::clear_project_listing_icon,
            listing_api::add_listing_gallery_image,
            listing_api::remove_listing_gallery_image,
            listing_api::reorder_listing_gallery,
            listing_api::read_listing_asset,
            listing_api::ensure_listing_folder,
            listing_api::update_project_brief_and_listing,
            listing_api::add_listing_gallery_bytes,
            list_profiles,
            list_mods,
            sync_mods_folder,
            import_local_content_files,
            search_content,
            search_modrinth_mods,
            search_modpack_index,
            list_modpack_index_categories,
            search_modpack_index_mods,
            search_curseforge_mods,
            search_unified_mods,
            preview_modrinth_install,
            resolve_install_dependencies,
            preview_curseforge_install,
            get_modrinth_project_icon,
            get_modrinth_project,
            list_modrinth_categories,
            list_curseforge_categories,
            get_catalog_project,
            get_catalog_versions,
            get_modrinth_pack_download,
            get_mod_user_state,
            set_mod_user_state,
            create_mod_list,
            delete_mod_list,
            rename_mod_list,
            add_to_mod_list,
            remove_from_mod_list,
            install_content_batch,
            add_modrinth_mod,
            add_modrinth_mod_with_dependencies,
            add_modrinth_mods_with_dependencies,
            install_steam_bridge,
            create_mode_api::create_mode_chat,
            create_mode_api::create_mode_quick_brief,
            create_mode_api::assemble_pack_draft,
            create_mode_api::rank_pack_draft,
            create_mode_api::curate_pack_loop,
            create_mode_api::cancel_curate_pack_loop,
            create_mode_api::preview_pack_draft,
            create_mode_api::install_pack_draft,
            create_mode_api::list_create_chats,
            create_mode_api::save_create_chat,
            create_mode_api::load_create_chat,
            create_mode_api::delete_create_chat,
            create_mode_api::new_create_chat,
            create_mode_api::resolve_modpack_index_search,
            add_curseforge_mod,
            add_curseforge_mods_with_dependencies,
            remove_project_mod,
            disable_project_mod,
            enable_project_mod,
            start_mod_group_test,
            get_mod_group_test,
            report_mod_group_test_outcome,
            cancel_mod_group_test,
            update_project_mod,
            check_mod_updates,
            update_all_mods,
            get_mod_versions,
            change_mod_version,
            detect_wrong_loader_mods,
            disable_wrong_loader_jar,
            remove_loose_jar,
            detect_duplicate_mod_jars,
            keep_one_duplicate_mod_jar,
            list_config_files,
            read_config_file,
            write_config_file,
            format_toml,
            search_in_configs,
            tune_config_api::tune_config_advise,
            tune_config_api::tune_config_preview_diffs,
            tune_config_api::list_tune_chat_sessions,
            tune_config_api::save_tune_chat_session,
            tune_config_api::load_tune_chat_session,
            tune_config_api::delete_tune_chat_session,
            tune_config_api::new_tune_chat_session,
            tune_config_api::tune_chat_turn,
            web_research::minecraft_wiki_rag_search,
            get_manifest_schema,
            record_launch,
            record_crash,
            get_launch_stats,
            get_graph,
            refresh_graph,
            get_diagnostics,
            get_pack_health,
            get_diagnostic_counts,
            get_health_report,
            run_project_validation,
            check_mod_compatibility,
            compare_modpacks,
            create_project_backup,
            list_backups,
            delete_backup,
            build_ai_crash_context,
            analyze_crash_with_ai,
            apply_action_plan,
            record_crash_ai_feedback,
            save_authored_crash_case,
            draft_authored_crash_case,
            list_authored_crash_cases,
            get_authored_case_export,
            open_authored_kb_folder,
            swarm_api::get_pending_action_plan,
            swarm_api::clear_pending_network_plan,
            swarm_api::write_pending_network_plan,
            swarm_api::get_share_prompt_after_launch,
            swarm_api::dismiss_share_prompt,
            swarm_api::confirm_crash_resolution_after_launch,
            swarm_api::confirm_crash_resolution_from_diagnose,
            swarm_api::get_crash_fix_banner,
            swarm_api::report_soft_verify_failure,
            swarm_api::rollback_last_crash_fix,
            swarm_api::distill_resolved_crash_plan,
            swarm_api::publish_experience_capsule,
            swarm_api::list_community_crash_capsules,
            swarm_api::vote_community_crash_capsule,
            swarm_api::accept_creation_result,
            swarm_api::get_local_kudos_balance,
            swarm_api::propose_community_capsule_plan,
            swarm_api::record_project_cooccurrence,
            swarm_api::report_mod_cooccurrence,
            swarm_api::get_local_cooccurrence,
            swarm_api::get_creation_trends,
            swarm_api::suggest_mods_from_trends,
            swarm_api::suggest_partners_for_mod,
            integrations::complete_swarm_onboarding,
            integrations::get_swarm_settings,
            integrations::set_swarm_enabled,
            integrations::set_swarm_share_prompts,
            integrations::set_swarm_hub_url,
            integrations::set_swarm_supabase_url,
            integrations::set_swarm_p2p,
            integrations::set_swarm_volunteer_diagnose,
            integrations::set_swarm_creation_worker,
            integrations::set_swarm_p2p_relay_server,
            integrations::set_swarm_advertised_vram_mb,
            swarm_node::get_p2p_node_status,
            swarm_node::ensure_p2p_node,
            swarm_node::restart_p2p_node,
            swarm_node::submit_creation_job,
            swarm_node::creation_job_defaults,
            swarm_node::apply_creation_artifacts,
            task_progress_api::list_background_tasks,
            task_progress_api::dismiss_background_task,
            task_progress_api::start_background_task,
            task_progress_api::try_start_background_task,
            task_progress_api::cancel_background_task,
            recommend_mods,
            get_mod_info,
            restore_backup,
            save_problematic_mods_config,
            get_problematic_mods_config,
            launch_server,
            generate_server_properties,
            scan_mod_recipes,
            get_vanilla_client_jar_status,
            download_vanilla_client_jar,
            get_item_icon,
            get_item_icons_batch,
            list_item_catalog,
            get_recipe_runtime_status,
            get_recipe_runtime_snapshot,
            write_kubejs_recipe_removes,
            write_kubejs_craft_recipe,
            write_kubejs_tag_edits,
            list_item_tags,
            get_item_tag_entries,
            generate_kubejs_recipe_script,
            load_quest_book,
            load_quest_chapter,
            save_quest_chapter_raw,
            preview_quest_chapter_snbt,
            read_quest_chapter_text,
            save_quest_chapter,
            validate_quest_book,
            save_quest_reward_table,
            save_quest_book_data,
            save_quest_chapter_groups,
            save_quest_locale,
            parse_and_merge_quest_plan,
            validate_quest_plan,
            quest_plan_system_prompt,
            generate_quest_plan_from_prompt,
            quest_chat_api::list_quest_chat_sessions,
            quest_chat_api::save_quest_chat_session,
            quest_chat_api::load_quest_chat_session,
            quest_chat_api::delete_quest_chat_session,
            quest_chat_api::new_quest_chat_session,
            quest_chat_api::quest_chat_turn,
            quest_chat_api::cancel_quest_chat_turn,
            quest_chat_api::generate_quest_line,
            quest_chat_api::filter_and_merge_quest_plan,
            list_quest_item_catalog,
            list_quest_progress_teams,
            load_quest_progress,
            simulate_quest_progress,
            quest_kubejs_list_scripts,
            quest_kubejs_audit,
            quest_kubejs_read_script,
            quest_kubejs_ensure_managed,
            quest_kubejs_render_template,
            quest_kubejs_append_handler,
            worlds::list_worlds,
            worlds::backup_world,
            worlds::restore_world_backup,
            worlds::delete_world,
            worlds::list_world_backups,
            worlds::delete_world_backup,
            worlds::read_world_icon,
            worlds::list_screenshots,
            worlds::delete_screenshot,
            mca_selector::open_mca_selector,
            list_content_packs,
            set_content_pack_enabled,
            list_mc_servers,
            add_mc_server,
            remove_mc_server,
            ping_mc_server,
            save_as_template,
            list_templates,
            get_download_progress,
            get_keyboard_shortcuts,
            lint_config,
            cleanup_project,
            get_app_version,
            integrations::check_for_app_update,
            integrations::get_integration_status,
            integrations::save_integration_settings,
            integrations::set_integration_secret,
            integrations::clear_integration_secret,
            integrations::test_integration,
            integrations::list_ollama_models,
            integrations::detect_ollama,
            integrations::scan_ollama_disk,
            integrations::pull_ollama_model,
            integrations::pause_ollama_model_pull,
            integrations::get_ollama_pull_status,
            integrations::import_ollama_gguf,
            integrations::ensure_ollama_model,
            integrations::get_ollama_storage,
            integrations::delete_ollama_model,
            integrations::get_publish_config,
            integrations::save_publish_config,
            integrations::publish_release,
            read_world_info,
            read_world_map,
            list_world_dimensions,
            delete_world_chunks,
            copy_world_chunks,
            paste_world_chunks,
            purge_world_regions,
            export_world_chunks,
            import_world_chunks,
            select_world_by_query,
            render_world_map_png,
            warm_world_map_cache,
            clear_world_map_cache,
            swap_world_chunks,
            change_world_chunks,
            read_chunk_editor,
            write_chunk_editor,
            filter_world_chunks_advanced,
            generate_github_release,
            localize,
            list_localizations,
            export_graph_dot,
            export_project_report,
            batch_export_all,
            export_preview,
            audit_performance,
            list_curated_optimize_packs,
            preview_curated_optimize_pack,
            install_curated_optimize_pack,
            build_optimize_plan,
            apply_optimize_custom_plan,
            preview_fo_optimize_pack,
            install_fo_optimize_pack,
            get_mod_presets,
            save_mod_presets_cmd,
            resolve_preset_mod,
            options_sync_status,
            options_sync_enable,
            options_sync_disable,
            options_sync_push,
            options_sync_groups,
            store_stats,
            store_retro_dedup,
            store_gc,
            dedup_project_status,
            dedup_project_set_enabled,
            youtube_my_feed_lookup,
            youtube_my_feed_fetch,
            scan_ore_generation,
            detect_duplicate_items,
            generate_unify_config,
            run_crash_assistant_full,
            find_class_in_mods,
            find_dependents_on_class,
            get_resolve_change_plan,
            apply_resolve_action,
            apply_resolve_change_plan,
            resolve_missing_dependencies,
            install_graph_dep,
            download_missing_files,
            get_crash_diagnosis,
            import_external_crash,
            export_diagnose_support_pack,
            create_crash_fix_plan,
            apply_crash_fix_plan,
            apply_fix_action,
            apply_fix_actions,
            get_history_settings,
            update_history_settings,
            list_project_change_history,
            get_history_entry_diff,
            scan_project_changes,
            list_recent_pack_events,
            explain_pack_change,
            explain_history_episode,
            read_project_history_file,
            create_tracked_history_snapshot,
            rollback_history_file,
            get_project_dir,
            list_snapshots,
            create_snapshot,
            diff_snapshots,
            rollback_snapshot,
            delete_snapshot,
            get_snapshot_detail,
            diff_manifest_snapshots,
            get_snapshot_file_diff,
            pack_diff_api::compare_pack_states,
            diff_snapshot_vs_current,
            get_snapshot_file_vs_current_diff,
            prune_auto_snapshots,
            validate_modrinth_export,
            validate_curseforge_export,
            generate_release_changelog,
            update_project_version,
            create_release_snapshot,
            export_modrinth_pack,
            export_server_pack,
            export_prism_instance,
            export_curseforge_pack,
            export_packwiz_pack,
            reveal_export_path,
            list_release_artifacts,
            create_release_draft,
            generate_lockfile,
            github_auth::github_pack_start_device_code,
            github_auth::github_pack_poll_device_code,
            github_auth::github_pack_auth_status,
            github_pack_commands::github_pack_parse_source,
            github_pack_commands::github_pack_inspect_source,
            github_pack_commands::github_pack_stage_preview,
            github_pack_commands::github_pack_install,
            github_pack_commands::github_pack_check_update,
            github_pack_commands::github_pack_preview_update,
            github_pack_commands::github_pack_apply_update,
            github_pack_commands::github_pack_publish,
            capture_test_run_logs,
            list_test_runs,
            finalize_test_run,
            launch_profile,
            launch_with_quick_play,
            import_project,
            import_curseforge_project,
            search_curseforge_modpacks,
            get_curseforge_modpack_files,
            install_modpack,
            inspect_import_source,
            begin_drop_import,
            write_drop_chunk,
            finish_drop_import,
            cancel_drop_import,
            retry_failed_mod_downloads,
            has_crashed,
            open_project_folder,
            create_project_desktop_shortcut,
            take_pending_launch_project,
            deep_link::take_pending_install_repo,
            delete_project,
            create_logs_zip,
            clone_project,
            repair_project,
            get_home_dir,
            list_running_instances,
            kill_running_instance,
            get_live_debug_stats,
            cosmetics_get_local_profile,
            cosmetics_save_profile,
            cosmetics_upload_skin,
            cosmetics_upload_cape,
            cosmetics_set_wings,
            cosmetics_set_visual_extras,
            cosmetics_wings_catalog,
            cosmetics_hat_catalog,
            get_minecraft_versions,
            get_loader_versions,
            create_instance,
            find_java_runtimes,
            ensure_java_runtime,
            get_java_version,
            get_default_java_version,
            get_launch_log,
            resolve_live_launch_log_path,
            share_log_mclogs,
            analyze_log_text,
            list_instance_logs,
            read_instance_log,
            get_instance_size,
            home_bootstrap::get_home_bootstrap,
            home_bootstrap::get_home_project_briefs,
            home_bootstrap::get_account_skin_paths,
            home_bootstrap::invalidate_home_project_cache,
            pin_project,
            is_project_pinned,
            set_last_opened_project,
            get_last_opened_project,
            load_recent_projects,
            save_recent_projects,
            update_project_settings,
            auth::mc_start_device_code,
            auth::mc_poll_device_code,
            auth::mc_get_microsoft_login_url,
            auth::mc_login_with_auth_url,
            auth::mc_start_microsoft_webview_auth,
            auth::mc_get_auth_status,
            auth::mc_logout,
            auth::mc_refresh_profile,
            auth::mc_refresh_token,
            auth::mc_get_skin_path,
            auth::mc_fetch_skin_url,
            auth::mc_offline_login,
            auth::mc_fetch_skin_for_username,
            auth::mc_set_skin_source,
            auth::mc_list_accounts,
            auth::mc_switch_account,
            auth::mc_remove_account,
            auth::mc_apply_skin,
            auth::mc_upload_skin,
            auth::mc_upload_skin_file,
            auth::mc_apply_cape,
            auth::mc_list_capes,
            auth::mc_set_cape_provider,
            auth::mc_list_yggdrasil_presets,
            auth::mc_yggdrasil_login,
            auth::mc_check_entitlement,
            auth::mc_get_skin_base64,
            get_presence_settings,
            save_presence_settings,
            launcher_settings::get_launcher_settings,
            launcher_settings::get_auto_tune,
            launcher_settings::count_instance_mods_cmd,
            recommend_heap_cmd,
            recommend_jvm_cmd,
            gpu::detect_gpus,
            window_glass::set_window_glass,
            launcher_settings::save_launcher_settings_cmd,
            launcher_settings::get_runtime_path_info,
            launcher_settings::get_instances_path_info,
            launcher_settings::validate_runtime_path_cmd,
            launcher_settings::validate_instances_path_cmd,
            set_discord_presence,
            clear_discord_presence,
            launcher_presence::launcher_presence_start,
            launcher_presence::launcher_presence_stop,
            launcher_presence::get_launcher_online,
            launcher_presence::get_launcher_recent_sessions,
        ])
        .manage(ClassFinderCache::default())
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app_handle, event| {
            if let tauri::RunEvent::Exit = event {
                launcher_presence::goodbye_on_exit();
            }
        });
}

#[cfg(test)]
mod mods_cleanup_tests {
    use super::*;
    use std::fs;
    use tuffbox_core::manifest::{
        ContentType, FileHashes, ModSource, ModSpec, Side, SourceKind,
    };

    /// Minimal Modrinth mod spec for cleanup tests.
    fn spec(id: &str, file_name: &str, sha1: Option<&str>) -> ModSpec {
        ModSpec {
            id: id.to_string(),
            name: id.to_string(),
            source: ModSource {
                kind: SourceKind::Modrinth,
                project_id: Some("proj-123".to_string()),
                file_id: Some("ver-1".to_string()),
                url: Some("https://example.com/file.jar".to_string()),
                path: None,
                icon_url: None,
                categories: Vec::new(),
            },
            version: "1.0.0".to_string(),
            file_name: Some(file_name.to_string()),
            hashes: sha1.map(|h| FileHashes {
                sha1: Some(h.to_string()),
                sha512: None,
            }),
            side: Side::Both,
            dependencies: Vec::new(),
            status: Vec::new(),
            content_type: ContentType::Mod,
            authors: Vec::new(),
            option: None,
        }
    }

    fn make_project() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let mods_dir = dir.path().join("mods");
        fs::create_dir_all(&mods_dir).unwrap();
        let manifest_path = dir.path().join("proj.tuffbox.json");
        // Manifest doesn't need to exist for cleanup to run — only the parent
        // dir (instance root) and the content folder matter.
        (dir, manifest_path)
    }

    fn tracked_from(names: &[&str]) -> std::collections::HashSet<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    fn files_in(dir: &std::path::Path) -> Vec<String> {
        let mut out = Vec::new();
        if let Ok(rd) = fs::read_dir(dir) {
            for e in rd.flatten() {
                if let Some(n) = e.file_name().to_str() {
                    out.push(n.to_string());
                }
            }
        }
        out.sort();
        out
    }

    /// Rename: the old filename (same mod slug prefix) is removed; the new file
    /// and unrelated jars are kept.
    #[test]
    fn removes_renamed_old_jar_keeps_new_and_unrelated() {
        let (dir, manifest_path) = make_project();
        let mods = dir.path().join("mods");
        fs::write(mods.join("sodium-fabric-0.5.0.jar"), b"old bytes").unwrap();
        fs::write(mods.join("sodium-fabric-0.5.8.jar"), b"new bytes").unwrap();
        // A different mod whose filename does NOT share the `sodium` slug.
        fs::write(mods.join("iris-fabric-1.6.0.jar"), b"unrelated").unwrap();

        let old = spec("sodium", "sodium-fabric-0.5.0.jar", None);
        let new = spec("sodium", "sodium-fabric-0.5.8.jar", Some("newsha1"));
        let tracked = tracked_from(&["iris-fabric-1.6.0.jar", "sodium-fabric-0.5.8.jar"]);
        remove_superseded_mod_files(&manifest_path, &old, &new, &tracked);

        let files = files_in(&mods);
        assert!(files.contains(&"sodium-fabric-0.5.8.jar".to_string()), "new jar kept: {files:?}");
        assert!(files.contains(&"iris-fabric-1.6.0.jar".to_string()), "unrelated kept: {files:?}");
        assert!(!files.contains(&"sodium-fabric-0.5.0.jar".to_string()), "old jar removed: {files:?}");
    }

    /// `.disabled` leftover for the same mod slug is cleaned up too.
    #[test]
    fn removes_disabled_leftover() {
        let (dir, manifest_path) = make_project();
        let mods = dir.path().join("mods");
        fs::write(mods.join("sodium-fabric-0.5.0.jar.disabled"), b"old bytes").unwrap();
        fs::write(mods.join("sodium-fabric-0.5.8.jar"), b"new bytes").unwrap();

        let old = spec("sodium", "sodium-fabric-0.5.0.jar", None);
        let new = spec("sodium", "sodium-fabric-0.5.8.jar", Some("newsha1"));
        let tracked = tracked_from(&["sodium-fabric-0.5.8.jar"]);
        remove_superseded_mod_files(&manifest_path, &old, &new, &tracked);

        let files = files_in(&mods);
        assert!(files.contains(&"sodium-fabric-0.5.8.jar".to_string()));
        assert!(!files.contains(&"sodium-fabric-0.5.0.jar.disabled".to_string()));
    }

    /// A jar whose bytes hash to the old sha1 is removed even when its
    /// filename drifted (manifest metadata out of sync with disk).
    #[test]
    fn removes_same_hash_even_with_different_name() {
        let (dir, manifest_path) = make_project();
        let mods = dir.path().join("mods");
        // Write content, then compute the real sha1 for that file.
        let content = b"identical-bytes-123";
        fs::write(mods.join("sodium-custom-name.jar"), content).unwrap();
        let hash = tuffbox_core::mc_install::sha1_file(&mods.join("sodium-custom-name.jar")).unwrap();
        fs::write(mods.join("sodium-fabric-0.5.8.jar"), b"new bytes").unwrap();

        // old spec claims a filename that no longer exists on disk, but its
        // sha1 matches the real leftover file.
        let old = spec("sodium", "sodium-fabric-0.5.0.jar", Some(&hash));
        let new = spec("sodium", "sodium-fabric-0.5.8.jar", Some("newsha1"));
        let tracked = tracked_from(&["sodium-fabric-0.5.8.jar"]);
        remove_superseded_mod_files(&manifest_path, &old, &new, &tracked);

        let files = files_in(&mods);
        assert!(files.contains(&"sodium-fabric-0.5.8.jar".to_string()));
        assert!(!files.contains(&"sodium-custom-name.jar".to_string()), "same-hash leftover removed: {files:?}");
    }

    /// The new file must never be removed even though it shares the mod slug
    /// prefix (guards against the cleanup deleting the fresh download).
    #[test]
    fn never_removes_the_new_file() {
        let (dir, manifest_path) = make_project();
        let mods = dir.path().join("mods");
        fs::write(mods.join("sodium-fabric-0.5.8.jar"), b"new bytes").unwrap();

        let old = spec("sodium", "sodium-fabric-0.5.0.jar", None);
        let new = spec("sodium", "sodium-fabric-0.5.8.jar", None);
        let tracked = tracked_from(&["sodium-fabric-0.5.8.jar"]);
        remove_superseded_mod_files(&manifest_path, &old, &new, &tracked);

        let files = files_in(&mods);
        assert!(files.contains(&"sodium-fabric-0.5.8.jar".to_string()));
    }

    /// A sibling mod (e.g. sodium-extra) whose filename shares the updated
    /// mod's slug prefix must NOT be removed by the cleanup heuristic.
    #[test]
    fn does_not_remove_sibling_mod_sharing_slug_prefix() {
        let (dir, manifest_path) = make_project();
        let mods = dir.path().join("mods");
        fs::write(mods.join("sodium-extra-0.4.0.jar"), b"sibling bytes").unwrap();
        fs::write(mods.join("sodium-fabric-0.5.8.jar"), b"new bytes").unwrap();

        let old = spec("sodium", "sodium-fabric-0.5.0.jar", None);
        let new = spec("sodium", "sodium-fabric-0.5.8.jar", Some("newsha1"));
        // sodium-extra is a separate tracked mod — its jar must survive.
        let tracked = tracked_from(&["sodium-extra-0.4.0.jar", "sodium-fabric-0.5.8.jar"]);
        remove_superseded_mod_files(&manifest_path, &old, &new, &tracked);

        let files = files_in(&mods);
        assert!(files.contains(&"sodium-fabric-0.5.8.jar".to_string()));
        assert!(files.contains(&"sodium-extra-0.4.0.jar".to_string()), "sibling kept: {files:?}");
    }
}

#[cfg(test)]
mod crash_diagnosis_cache_tests {
    use super::*;

    const MINIMAL_MANIFEST: &str = r#"{
      "schemaVersion": "0.1.0",
      "project": { "id": "diag-cache-test", "name": "Diag Cache Test", "version": "0.1.0" },
      "minecraft": { "version": "1.20.1" },
      "loader": { "type": "fabric", "version": "0.15.0" },
      "mods": []
    }"#;

    fn fixture_project() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let manifest = dir.path().join("project.tuffbox.json");
        std::fs::write(&manifest, MINIMAL_MANIFEST).unwrap();
        std::fs::create_dir_all(dir.path().join("logs")).unwrap();
        std::fs::create_dir_all(dir.path().join("mods")).unwrap();
        std::fs::write(
            dir.path().join("logs").join("latest.log"),
            "INFO: game started\n",
        )
        .unwrap();
        let path = dir.path().to_string_lossy().to_string();
        (dir, path)
    }

    /// The diagnosis cache lookup existed but the computed result was never
    /// stored, so the "Loaded from cache" fast path was dead code and every
    /// Diagnose open re-ran the full log/jar/graph analysis. Guard the put:
    /// after one impl run the mtime key must hold a fresh entry, and a second
    /// run must serve byte-identical data from it.
    #[test]
    fn get_crash_diagnosis_stores_result_in_cache() {
        let (_dir, path) = fixture_project();
        let first = get_crash_diagnosis_impl(None, path.clone(), None).unwrap();
        let key = crash_diagnosis_cache_key(&path, None);
        let cached: tuffbox_core::crash::CrashDiagnosis = tuffbox_core::api_cache::get(&key)
            .expect("diagnosis must be cached after the first run");
        let second = get_crash_diagnosis_impl(None, path.clone(), None).unwrap();
        let first_json = serde_json::to_string(&first).unwrap();
        assert_eq!(
            first_json,
            serde_json::to_string(&cached).unwrap(),
            "cache entry must hold the same diagnosis the first run returned"
        );
        assert_eq!(
            first_json,
            serde_json::to_string(&second).unwrap(),
            "second run must return the same diagnosis for unchanged inputs"
        );
    }

    /// Changing the analyzed source (report_id) must not collide with the
    /// default key — different source ⇒ different cache entry.
    #[test]
    fn cache_key_separates_sources() {
        let (_dir, path) = fixture_project();
        let none_key = crash_diagnosis_cache_key(&path, None);
        let latest_key = crash_diagnosis_cache_key(&path, Some("__latest_log__"));
        let session_key = crash_diagnosis_cache_key(&path, Some("session/abc"));
        assert_ne!(none_key, latest_key);
        assert_ne!(none_key, session_key);
        assert_ne!(latest_key, session_key);
    }
}
