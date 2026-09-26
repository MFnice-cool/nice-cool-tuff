//! Home / Dashboard bootstrap pipeline.
//!
//! P0: one cheap snapshot (JSON + cached auth + stats/icons from recent cache).
//! P1/P2: background enrich via `home:enrich` events (validate, banner, icons, size).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tuffbox_core::process;
use tuffbox_core::ProjectManifest;

use crate::auth::{cached_auth_state, cached_skin_path, AuthState};
use crate::helpers::{
    compute_instance_size_bytes, format_byte_size, instance_size_fingerprint, load_recent_projects,
    load_stats, manifest_parent, patch_recent_home_cache, resolve_manifest_path,
    RecentHomeCachePatch,
};
use crate::launcher_settings::{load_launcher_settings, LauncherSettings};
use crate::listing_api::try_read_listing_icon_data_url;
use crate::swarm_api::{get_crash_fix_banner, CrashFixBanner};
use crate::types::{ProjectSummary, RecentProjectEntry};

const HOME_ENRICH_EVENT: &str = "home:enrich";
const BRIEF_PARALLELISM: usize = 4;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeBootstrapRequest {
    #[serde(default)]
    pub selected_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeStatsBrief {
    pub playtime: u64,
    pub last_launch: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeRunningInstance {
    pub id: String,
    pub pid: u32,
    pub profile: String,
    pub started_at: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeSnapshot {
    pub recent: Vec<RecentProjectEntry>,
    pub last_opened: Option<String>,
    pub launcher_settings: LauncherSettings,
    pub auth: AuthState,
    pub skin_paths: HashMap<String, String>,
    pub running: Vec<HomeRunningInstance>,
    pub stats_by_path: HashMap<String, HomeStatsBrief>,
    pub icons_by_path: HashMap<String, String>,
    pub sizes_by_path: HashMap<String, String>,
    pub selected_summary: Option<ProjectSummary>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HomeEnrichPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_summary: Option<ProjectSummary>,
    /// Present when the enrich phase resolved the banner (value may be null).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crash_fix_banner: Option<CrashFixBanner>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clear_crash_fix_banner: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icons_by_path: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sizes_by_path: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stats_by_path: Option<HashMap<String, HomeStatsBrief>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<AuthState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skin_paths: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeProjectBrief {
    pub path: String,
    pub stats: HomeStatsBrief,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_data_url: Option<String>,
}

fn load_last_opened() -> Option<String> {
    let home = dirs::home_dir()?;
    let data_path = home.join("TuffBox").join(".tuffbox").join("launcher-data.json");
    let raw = std::fs::read_to_string(data_path).ok()?;
    let state: crate::types::LauncherDataState = serde_json::from_str(&raw).ok()?;
    state.last_opened
}

fn stats_brief_for_path(path: &str) -> HomeStatsBrief {
    let Ok(project_dir) = manifest_parent(path) else {
        return HomeStatsBrief {
            playtime: 0,
            last_launch: None,
        };
    };
    let stats = load_stats(&project_dir);
    let mut playtime = 0u64;
    let mut last = None;
    for inst in stats.instances.values() {
        playtime += inst.total_playtime_seconds;
        if inst.last_launch.is_some() {
            last = inst.last_launch.clone();
        }
    }
    HomeStatsBrief {
        playtime,
        last_launch: last,
    }
}

fn try_validate_summary(path: &str) -> Option<ProjectSummary> {
    let manifest_path = resolve_manifest_path(path).ok()?;
    let manifest = ProjectManifest::load_from_path(&manifest_path).ok()?;
    manifest.validate_basic().ok()?;
    let profile = manifest
        .profiles
        .iter()
        .find(|p| p.id == "client")
        .or_else(|| manifest.profiles.first())?;
    Some(ProjectSummary {
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
    })
}

fn collect_skin_paths(auth: &AuthState) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut uuids: Vec<String> = auth.accounts.iter().map(|a| a.uuid.clone()).collect();
    if let Some(ref profile) = auth.profile {
        uuids.push(profile.uuid.clone());
    }
    if let Some(ref active) = auth.active_account_uuid {
        uuids.push(active.clone());
    }
    uuids.sort();
    uuids.dedup();
    for uuid in uuids {
        let path = cached_skin_path(&uuid);
        if path.exists() {
            out.insert(uuid, path.to_string_lossy().to_string());
        }
    }
    out
}

fn list_running_home() -> Vec<HomeRunningInstance> {
    process::list_running()
        .into_iter()
        .map(|g| HomeRunningInstance {
            id: g.id,
            pid: g.pid,
            profile: g.profile_id,
            started_at: g.started_at,
        })
        .collect()
}

fn load_home_snapshot_cheap(selected_path: Option<String>) -> HomeSnapshot {
    let recent = load_recent_projects();
    let last_opened = load_last_opened();
    let launcher_settings = load_launcher_settings();
    let auth = cached_auth_state();
    let skin_paths = collect_skin_paths(&auth);
    let running = list_running_home();

    let mut stats_by_path = HashMap::new();
    let mut icons_by_path = HashMap::new();
    let mut sizes_by_path = HashMap::new();

    for entry in &recent {
        if let (Some(pt), last) = (
            entry.stats_playtime_seconds,
            entry.stats_last_launch.clone(),
        ) {
            stats_by_path.insert(
                entry.path.clone(),
                HomeStatsBrief {
                    playtime: pt,
                    last_launch: last,
                },
            );
        } else {
            let brief = stats_brief_for_path(&entry.path);
            let _ = patch_recent_home_cache(
                &entry.path,
                RecentHomeCachePatch {
                    stats_playtime_seconds: Some(Some(brief.playtime)),
                    stats_last_launch: Some(brief.last_launch.clone()),
                    ..Default::default()
                },
            );
            stats_by_path.insert(entry.path.clone(), brief);
        }
        if let Some(ref icon) = entry.icon_data_url {
            icons_by_path.insert(entry.path.clone(), icon.clone());
        }
        if let Some(ref label) = entry.size_label {
            sizes_by_path.insert(entry.path.clone(), label.clone());
        }
    }

    // Prefer validate of selected / lastOpened for immediate project restore (cheap enough).
    let candidate = selected_path
        .or_else(|| last_opened.clone())
        .or_else(|| recent.first().map(|r| r.path.clone()));
    let selected_summary = candidate.as_deref().and_then(try_validate_summary);

    HomeSnapshot {
        recent,
        last_opened,
        launcher_settings,
        auth,
        skin_paths,
        running,
        stats_by_path,
        icons_by_path,
        sizes_by_path,
        selected_summary,
    }
}

fn cached_size_label(path: &str) -> Option<String> {
    load_recent_projects()
        .into_iter()
        .find(|e| e.path == path)
        .and_then(|e| e.size_label)
}

fn size_label_for_project(path: &str, use_cache: bool) -> Option<(String, u64, String)> {
    let project_dir = manifest_parent(path).ok()?;
    let fingerprint = instance_size_fingerprint(&project_dir);
    if use_cache {
        if let Some(entry) = load_recent_projects().into_iter().find(|e| e.path == path) {
            if entry.size_fingerprint.as_deref() == Some(fingerprint.as_str()) {
                if let (Some(label), Some(bytes)) = (entry.size_label, entry.size_bytes) {
                    return Some((label, bytes, fingerprint));
                }
            }
        }
    }
    let bytes = compute_instance_size_bytes(&project_dir);
    let label = format_byte_size(bytes);
    Some((label, bytes, fingerprint))
}

fn emit_enrich(app: &AppHandle, payload: HomeEnrichPayload) {
    let _ = app.emit(HOME_ENRICH_EVENT, payload);
}

fn spawn_home_enrich(app: AppHandle, snap: HomeSnapshot, selected_override: Option<String>) {
    tauri::async_runtime::spawn(async move {
        let selected = selected_override
            .or_else(|| {
                snap.selected_summary
                    .as_ref()
                    .map(|s| s.manifest_path.clone())
            })
            .or(snap.last_opened.clone());

        // P1: crash-fix banner + missing icons (parallel via spawn_blocking batches).
        let selected_for_banner = selected.clone();
        let banner = tokio::task::spawn_blocking(move || {
            selected_for_banner
                .as_ref()
                .and_then(|p| get_crash_fix_banner(p.clone()).ok().flatten())
        })
        .await
        .ok()
        .flatten();

        emit_enrich(
            &app,
            HomeEnrichPayload {
                crash_fix_banner: banner.clone(),
                clear_crash_fix_banner: if banner.is_none() { Some(true) } else { None },
                phase: Some("banner".into()),
                ..Default::default()
            },
        );

        let paths_needing_icons: Vec<String> = snap
            .recent
            .iter()
            .filter(|e| !snap.icons_by_path.contains_key(&e.path))
            .map(|e| e.path.clone())
            .collect();

        if !paths_needing_icons.is_empty() {
            let icons = tokio::task::spawn_blocking(move || {
                let mut icons = HashMap::new();
                for chunk in paths_needing_icons.chunks(BRIEF_PARALLELISM) {
                    let chunk = chunk.to_vec();
                    let partial: Vec<(String, Option<String>)> = chunk
                        .into_iter()
                        .map(|p| {
                            let icon = try_read_listing_icon_data_url(&p);
                            if let Some(ref data) = icon {
                                let _ = patch_recent_home_cache(
                                    &p,
                                    RecentHomeCachePatch {
                                        icon_data_url: Some(Some(data.clone())),
                                        ..Default::default()
                                    },
                                );
                            }
                            (p, icon)
                        })
                        .collect();
                    for (p, icon) in partial {
                        if let Some(data) = icon {
                            icons.insert(p, data);
                        }
                    }
                }
                icons
            })
            .await
            .unwrap_or_default();

            if !icons.is_empty() {
                emit_enrich(
                    &app,
                    HomeEnrichPayload {
                        icons_by_path: Some(icons),
                        phase: Some("icons".into()),
                        ..Default::default()
                    },
                );
            }
        }

        // P2: selected instance size only (walk off the critical path).
        if let Some(ref sel) = selected {
            let sel_path = sel.clone();
            let size_result = tokio::task::spawn_blocking(move || {
                size_label_for_project(&sel_path, true).map(|(label, bytes, fp)| {
                    let _ = patch_recent_home_cache(
                        &sel_path,
                        RecentHomeCachePatch {
                            size_label: Some(Some(label.clone())),
                            size_bytes: Some(Some(bytes)),
                            size_fingerprint: Some(Some(fp)),
                            ..Default::default()
                        },
                    );
                    (sel_path, label)
                })
            })
            .await
            .ok()
            .flatten();

            if let Some((path, label)) = size_result {
                let mut sizes = HashMap::new();
                sizes.insert(path, label);
                emit_enrich(
                    &app,
                    HomeEnrichPayload {
                        sizes_by_path: Some(sizes),
                        phase: Some("size".into()),
                        ..Default::default()
                    },
                );
            }
        }

        // Soft auth refresh in background (network) — FE already has cached auth.
        if let Ok(auth) = crate::auth::mc_get_auth_status().await {
            let skin_paths = collect_skin_paths(&auth);
            emit_enrich(
                &app,
                HomeEnrichPayload {
                    auth: Some(auth),
                    skin_paths: Some(skin_paths),
                    phase: Some("auth".into()),
                    ..Default::default()
                },
            );
        }

        emit_enrich(
            &app,
            HomeEnrichPayload {
                phase: Some("ready".into()),
                ..Default::default()
            },
        );
    });
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_home_bootstrap(
    app: AppHandle,
    request: Option<HomeBootstrapRequest>,
) -> Result<HomeSnapshot, String> {
    let req = request.unwrap_or(HomeBootstrapRequest {
        selected_path: None,
    });
    let selected = req.selected_path.clone();
    let snap = tokio::task::spawn_blocking(move || load_home_snapshot_cheap(selected))
        .await
        .map_err(|e| e.to_string())?;
    spawn_home_enrich(app, snap.clone(), req.selected_path);
    Ok(snap)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_home_project_briefs(paths: Vec<String>) -> Result<Vec<HomeProjectBrief>, String> {
    tokio::task::spawn_blocking(move || {
        let mut out = Vec::with_capacity(paths.len());
        for chunk in paths.chunks(BRIEF_PARALLELISM) {
            let briefs: Vec<HomeProjectBrief> = chunk
                .iter()
                .map(|path| {
                    let stats = stats_brief_for_path(path);
                    let icon = try_read_listing_icon_data_url(path);
                    // Never walk disk here — size is selected-only / enrich / get_instance_size.
                    let size_label = cached_size_label(path);
                    let _ = patch_recent_home_cache(
                        path,
                        RecentHomeCachePatch {
                            icon_data_url: icon.as_ref().map(|i| Some(i.clone())),
                            stats_playtime_seconds: Some(Some(stats.playtime)),
                            stats_last_launch: Some(stats.last_launch.clone()),
                            ..Default::default()
                        },
                    );
                    HomeProjectBrief {
                        path: path.clone(),
                        stats,
                        size_label,
                        icon_data_url: icon,
                    }
                })
                .collect();
            out.extend(briefs);
        }
        Ok(out)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_account_skin_paths(uuids: Vec<String>) -> Result<HashMap<String, String>, String> {
    tokio::task::spawn_blocking(move || {
        let mut out = HashMap::new();
        for uuid in uuids {
            if uuid.is_empty() {
                continue;
            }
            let path = cached_skin_path(&uuid);
            if path.exists() {
                out.insert(uuid, path.to_string_lossy().to_string());
            }
        }
        out
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub fn invalidate_home_project_cache(path: String) -> Result<(), String> {
    crate::helpers::invalidate_recent_home_cache(&path);
    Ok(())
}

/// Shared size walk used by the legacy `get_instance_size` command.
pub(crate) fn instance_size_label(path: &str) -> Result<String, String> {
    let project_dir = manifest_parent(path)?;
    let bytes = compute_instance_size_bytes(&project_dir);
    Ok(format_byte_size(bytes))
}
