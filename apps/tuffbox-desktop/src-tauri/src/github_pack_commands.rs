use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::Emitter;
use tuffbox_core::github_pack::{
    LiveGitHubApi, LocalTransportMeta, PublishError, RepoTransportMeta, StageOptions,
    TRANSPORT_SCHEMA_VERSION, TransportKind, apply_managed_files, commit_sha, diff_manifests,
    extract_github_tarball, import_repo_tree, inspect_public_pack, managed_from_transport,
    materialize_release_assets, parse_github_source, pin_or_check_signer, publish_staged_tree,
    remove_obsolete_content_files, stage_repo_tree, update_available, validate_github_ref,
    verify_manifest_local_hashes,
};
use tuffbox_core::{ProjectManifest, SnapshotStore};

use crate::github_auth::{author_signing_key, stored_author_token};
use crate::helpers::{
    auto_snapshot_detailed, auto_snapshot_with_managed, persist_lockfile_for_manifest,
    save_manifest,
};

const APP_USER_AGENT: &str = "TuffBox-IDE/0.1";
const MAX_GITHUB_DOWNLOAD_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubPackStagePreview {
    pub pack_version: String,
    pub manifest_file: String,
    pub file_count: usize,
    pub managed_files: Vec<String>,
    pub share_url: Option<String>,
    pub content_digest: String,
    pub has_external_assets: bool,
}

#[tauri::command(rename_all = "camelCase")]
pub fn github_pack_parse_source(source: String) -> Result<serde_json::Value, String> {
    let src = parse_github_source(&source).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "owner": src.owner,
        "repo": src.repo,
        "ref": src.git_ref,
    }))
}

#[tauri::command(rename_all = "camelCase")]
pub fn github_pack_inspect_source(source: String) -> Result<serde_json::Value, String> {
    let src = parse_github_source(&source).map_err(|e| e.to_string())?;
    let client = anonymous_client()?;
    inspect_public_pack(&client, &src.owner, &src.repo).map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub fn github_pack_stage_preview(path: String) -> Result<GithubPackStagePreview, String> {
    let manifest_path = PathBuf::from(&path);
    let manifest = ProjectManifest::load_from_path(&manifest_path).map_err(|e| e.to_string())?;
    let staging = tempfile::tempdir().map_err(|e| e.to_string())?;
    let staged = stage_repo_tree(
        &manifest,
        &manifest_path,
        staging.path(),
        None,
        StageOptions::default(),
    )
    .map_err(|e| e.to_string())?;
    Ok(GithubPackStagePreview {
        pack_version: staged.transport.pack_version,
        manifest_file: staged.transport.manifest_file,
        file_count: staged.files.len(),
        managed_files: staged.transport.managed_files,
        share_url: None,
        content_digest: staged.transport.content_digest,
        has_external_assets: !staged.pending_release_assets.is_empty(),
    })
}

#[tauri::command(rename_all = "camelCase")]
pub async fn github_pack_install(
    app: tauri::AppHandle,
    source: String,
    target_dir: String,
    instance_name: Option<String>,
    dedup: Option<bool>,
) -> Result<serde_json::Value, String> {
    let parsed = parse_github_source(&source).map_err(|e| e.to_string())?;
    let git_ref = parsed.git_ref.clone().unwrap_or_else(|| "HEAD".into());
    let client = LiveGitHubApi::new(stored_author_token()).map_err(|e| e.to_string())?;
    let commit = commit_sha(&client, &parsed.owner, &parsed.repo, &git_ref)
        .unwrap_or_else(|_| git_ref.clone());
    let url = format!(
        "https://api.github.com/repos/{}/{}/tarball/{}",
        parsed.owner, parsed.repo, commit
    );
    let bytes = download_github_bytes(&url).await?;

    let task_id = format!("gh-pack-{}", parsed.repo);
    if !tuffbox_core::task_progress::try_start_task(
        task_id.clone(),
        format!("Install GitHub pack {}/{}", parsed.owner, parsed.repo),
    ) {
        return Err(format!(
            "Install of {}/{} is already running — wait for it to finish",
            parsed.owner, parsed.repo
        ));
    }
    tuffbox_core::task_progress::set_progress(&task_id, 0.15, Some("Downloading tarball…".into()));

    tokio::task::spawn_blocking(move || {
        // Keep TaskProgress accurate across the blocking install; the helper
        // below marks success/failure so the panel never shows a stuck task.
        let result = install_from_tarball_bytes(
            &app,
            &bytes,
            &parsed.owner,
            &parsed.repo,
            parsed.git_ref.as_deref(),
            Some(&commit),
            &target_dir,
            instance_name,
            dedup,
        );
        match &result {
            Ok(v) => tuffbox_core::task_progress::succeed(
                &task_id,
                Some(format!(
                    "{} mods",
                    v.get("modCount").and_then(|m| m.as_u64()).unwrap_or(0)
                )),
            ),
            Err(e) => tuffbox_core::task_progress::fail(&task_id, e.clone()),
        }
        result
    })
    .await
    .map_err(|e| e.to_string())?
}

fn install_from_tarball_bytes(
    app: &tauri::AppHandle,
    bytes: &[u8],
    owner: &str,
    repo: &str,
    git_ref: Option<&str>,
    commit_sha_val: Option<&str>,
    target_dir: &str,
    instance_name: Option<String>,
    dedup: Option<bool>,
) -> Result<serde_json::Value, String> {
    let extract_dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    extract_github_tarball(bytes, extract_dir.path()).map_err(|e| e.to_string())?;
    let (mut manifest, transport) =
        import_repo_tree(extract_dir.path()).map_err(|e| e.to_string())?;
    if transport.as_ref().is_some_and(|t| t.status == "publishing") {
        return Err(
            "This pack is still publishing oversized assets. Try again when it is ready.".into(),
        );
    }
    if let Some(incoming) = transport
        .as_ref()
        .and_then(|t| t.signer_public_key.as_deref())
    {
        pin_or_check_signer(None, incoming).map_err(|e| e.to_string())?;
    }
    if let Some(meta) = &transport {
        let client = LiveGitHubApi::new(stored_author_token()).map_err(|e| e.to_string())?;
        materialize_release_assets(&client, owner, repo, extract_dir.path(), meta)
            .map_err(|e| e.to_string())?;
    }
    verify_manifest_local_hashes(extract_dir.path(), &manifest).map_err(|e| e.to_string())?;
    if let Some(name) = instance_name.filter(|n| !n.trim().is_empty()) {
        manifest.project.name = name.clone();
        manifest.project.id = crate::helpers::slugify_project_name(&name);
    } else {
        manifest.project.id = crate::helpers::slugify_project_name(&manifest.project.id);
    }
    if manifest.project.id.is_empty() {
        return Err("GitHub pack project id is empty after sanitization".into());
    }
    let target_root = PathBuf::from(target_dir);
    fs::create_dir_all(&target_root).map_err(|e| e.to_string())?;
    let final_instance_dir = target_root.join(&manifest.project.id);
    ensure_install_target_available(&final_instance_dir)?;
    let install_staging = tempfile::tempdir_in(&target_root).map_err(|e| e.to_string())?;
    let staged_instance_dir = install_staging.path().join(&manifest.project.id);
    fs::create_dir_all(&staged_instance_dir).map_err(|e| e.to_string())?;
    copy_tree(extract_dir.path(), &staged_instance_dir)?;
    let staged_manifest_path =
        staged_instance_dir.join(format!("{}.tuffbox.json", manifest.project.id));
    save_manifest(&staged_manifest_path, &manifest).map_err(|e| e.to_string())?;
    if let Some(transport) = &transport {
        remove_stale_manifest(
            &staged_instance_dir.join(&transport.manifest_file),
            &staged_manifest_path,
        )?;
    }
    persist_lockfile_for_manifest(&staged_manifest_path, &manifest).map_err(|e| e.to_string())?;
    let local = LocalTransportMeta {
        schema_version: TRANSPORT_SCHEMA_VERSION,
        kind: TransportKind::Github,
        repo: format!("{owner}/{repo}"),
        git_ref: git_ref.map(|s| s.to_string()),
        manifest_file: format!("{}.tuffbox.json", manifest.project.id),
        installed_version: manifest.project.version.clone(),
        installed_commit_sha: commit_sha_val.map(|s| s.to_string()),
        installed_at: Some(tuffbox_core::time_util::rfc3339_now()),
        pinned_signer_public_key: transport.and_then(|t| t.signer_public_key),
    };
    write_local_transport(&staged_instance_dir, &local)?;
    let report =
        crate::download_project_mods_tracked(app, &staged_manifest_path, &manifest, None, true);
    if !report.failed.is_empty() {
        let failures = report
            .failed
            .iter()
            .map(|failure| format!("{}: {}", failure.mod_id, failure.error))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(format!("GitHub pack materialization failed: {failures}"));
    }
    verify_manifest_local_hashes(&staged_instance_dir, &manifest).map_err(|e| e.to_string())?;
    promote_staged_install(&staged_instance_dir, &final_instance_dir)?;
    let manifest_path = final_instance_dir.join(format!("{}.tuffbox.json", manifest.project.id));
    if let Err(error) = auto_snapshot_detailed(
        &manifest_path,
        "github_pack_install",
        &[],
        &["Imported GitHub pack".into()],
    ) {
        let _ = fs::remove_dir_all(&final_instance_dir);
        return Err(error.to_string());
    }
    // Import-time dedup choice (docs/17 §4.4) — same contract as the
    // non-GitHub install paths: Some(true) links the pack into the shared
    // store, Some(false) writes the opt-out marker, None keeps defaults.
    let dedup_info = crate::apply_import_dedup_choice(&final_instance_dir, dedup);
    Ok(serde_json::json!({
        "path": manifest_path.to_string_lossy(),
        "name": manifest.project.name,
        "modCount": manifest.mods.len(),
        "provider": "github",
        "repo": format!("{owner}/{repo}"),
        "dedup": dedup_info,
    }))
}

fn write_local_transport(instance_dir: &Path, local: &LocalTransportMeta) -> Result<(), String> {
    let transport_path = instance_dir.join(".tuffbox").join("transport.json");
    if let Some(parent) = transport_path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(
        &transport_path,
        serde_json::to_vec_pretty(local).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

fn load_local_transport(project_dir: &Path) -> Result<LocalTransportMeta, String> {
    let transport_path = project_dir.join(".tuffbox").join("transport.json");
    serde_json::from_str(&fs::read_to_string(transport_path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

fn load_repo_transport(project_dir: &Path) -> Result<Option<RepoTransportMeta>, String> {
    let path = project_dir.join(".tuffbox").join("repo-transport.json");
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let metadata: RepoTransportMeta =
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    metadata.validate().map_err(|error| error.to_string())?;
    Ok(Some(metadata))
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    for entry in fs::read_dir(from).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let src = entry.path();
        let dest = to.join(entry.file_name());
        if src.is_dir() {
            fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
            copy_tree(&src, &dest)?;
        } else if src.is_file() {
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            // copy_replacing: never write into an inode that may be shared
            // with the dedup store (re-installing over an existing pack).
            tuffbox_core::fs_util::copy_replacing(&src, &dest).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn ensure_install_target_available(target: &Path) -> Result<(), String> {
    if !target.exists() {
        return Ok(());
    }
    if !target.is_dir() {
        return Err(format!(
            "install target is not a directory: {}",
            target.display()
        ));
    }
    let mut entries = fs::read_dir(target).map_err(|e| e.to_string())?;
    if entries
        .next()
        .transpose()
        .map_err(|e| e.to_string())?
        .is_some()
    {
        return Err(format!("install target is not empty: {}", target.display()));
    }
    Ok(())
}

fn promote_staged_install(staged: &Path, target: &Path) -> Result<(), String> {
    ensure_install_target_available(target)?;
    if target.is_dir() {
        fs::remove_dir(target).map_err(|e| e.to_string())?;
    }
    fs::rename(staged, target).map_err(|e| e.to_string())
}

fn remove_stale_manifest(source: &Path, canonical: &Path) -> Result<(), String> {
    if source != canonical && source.is_file() {
        fs::remove_file(source).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn anonymous_client() -> Result<LiveGitHubApi, String> {
    LiveGitHubApi::new(stored_author_token()).map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub fn github_pack_check_update(path: String) -> Result<serde_json::Value, String> {
    let manifest_path = PathBuf::from(&path);
    let project_dir = manifest_path.parent().ok_or("manifest has no parent")?;
    let transport_path = project_dir.join(".tuffbox").join("transport.json");
    if !transport_path.is_file() {
        return Ok(serde_json::json!({ "updateAvailable": false, "reason": "not a GitHub pack" }));
    }
    let local = load_local_transport(project_dir)?;
    let parsed = parse_github_source(&local.repo).map_err(|e| e.to_string())?;
    let git_ref = local.git_ref.clone().unwrap_or_else(|| "HEAD".into());
    let client = anonymous_client()?;
    let remote_sha =
        commit_sha(&client, &parsed.owner, &parsed.repo, &git_ref).map_err(|e| e.to_string())?;
    let manifest = ProjectManifest::load_from_path(&manifest_path).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "updateAvailable": update_available(local.installed_commit_sha.as_deref(), &remote_sha),
        "repo": local.repo,
        "installedVersion": manifest.project.version,
        "installedCommitSha": local.installed_commit_sha,
        "remoteCommitSha": remote_sha,
    }))
}

#[tauri::command(rename_all = "camelCase")]
pub async fn github_pack_preview_update(path: String) -> Result<serde_json::Value, String> {
    let manifest_path = PathBuf::from(&path);
    let project_dir = manifest_path
        .parent()
        .ok_or("manifest has no parent")?
        .to_path_buf();
    let local = load_local_transport(&project_dir)?;
    let parsed = parse_github_source(&local.repo).map_err(|e| e.to_string())?;
    let git_ref = local.git_ref.clone().unwrap_or_else(|| "HEAD".into());
    let (bytes, remote_sha) = download_tarball(&parsed.owner, &parsed.repo, &git_ref).await?;
    tokio::task::spawn_blocking(move || {
        let extract_dir = tempfile::tempdir().map_err(|e| e.to_string())?;
        extract_github_tarball(&bytes, extract_dir.path()).map_err(|e| e.to_string())?;
        let (incoming, transport) =
            import_repo_tree(extract_dir.path()).map_err(|e| e.to_string())?;
        if transport.as_ref().is_some_and(|t| t.status == "publishing") {
            return Err("This pack is still publishing oversized assets. Try again when it is ready.".into());
        }
        let incoming_key = transport.as_ref().and_then(|t| t.signer_public_key.clone());
        let signer_state = match enforce_pinned_signer(
            local.pinned_signer_public_key.as_deref(),
            incoming_key.as_deref(),
        ) {
            Ok(()) if incoming_key.is_some() => "ok",
            Ok(()) => "unsigned",
            Err(_) => "changed",
        };
        let current = ProjectManifest::load_from_path(&manifest_path).map_err(|e| e.to_string())?;
        let diff = diff_manifests(&current, &incoming);
        Ok(serde_json::json!({
            "repo": local.repo,
            "installedVersion": current.project.version,
            "incomingVersion": incoming.project.version,
            "remoteCommitSha": remote_sha,
            "requiresFullReinstall": diff.requires_full_reinstall(),
            "signerState": signer_state,
            "changes": diff.changes,
            "customFiles": diff.changes.iter().any(|c| matches!(
                c,
                tuffbox_core::github_pack::PackChange::ModAdded { origin: tuffbox_core::github_pack::TrustOrigin::Custom, .. }
                | tuffbox_core::github_pack::PackChange::ModBumped { origin: tuffbox_core::github_pack::TrustOrigin::Custom, .. }
            )),
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn github_pack_apply_update(
    app: tauri::AppHandle,
    path: String,
    expected_commit_sha: String,
) -> Result<serde_json::Value, String> {
    let manifest_path = PathBuf::from(&path);
    let project_dir = manifest_path
        .parent()
        .ok_or("manifest has no parent")?
        .to_path_buf();
    let local = load_local_transport(&project_dir)?;
    let parsed = parse_github_source(&local.repo).map_err(|e| e.to_string())?;
    if expected_commit_sha.len() != 40
        || !expected_commit_sha.chars().all(|ch| ch.is_ascii_hexdigit())
    {
        return Err("expected commit SHA must be a 40-character hexadecimal value".into());
    }
    let bytes =
        download_tarball_at_commit(&parsed.owner, &parsed.repo, &expected_commit_sha).await?;
    let remote_sha = expected_commit_sha;
    tokio::task::spawn_blocking(move || {
        let extract_dir = tempfile::tempdir().map_err(|e| e.to_string())?;
        extract_github_tarball(&bytes, extract_dir.path()).map_err(|e| e.to_string())?;
        let (incoming, transport) =
            import_repo_tree(extract_dir.path()).map_err(|e| e.to_string())?;
        if transport.as_ref().is_some_and(|t| t.status == "publishing") {
            return Err(
                "This pack is still publishing oversized assets. Try again when it is ready."
                    .into(),
            );
        }
        let incoming_key = transport
            .as_ref()
            .and_then(|t| t.signer_public_key.as_deref());
        enforce_pinned_signer(local.pinned_signer_public_key.as_deref(), incoming_key)?;
        if let Some(meta) = &transport {
            let client = LiveGitHubApi::new(stored_author_token()).map_err(|e| e.to_string())?;
            materialize_release_assets(
                &client,
                &parsed.owner,
                &parsed.repo,
                extract_dir.path(),
                meta,
            )
            .map_err(|e| e.to_string())?;
        }
        verify_manifest_local_hashes(extract_dir.path(), &incoming).map_err(|e| e.to_string())?;
        let current = ProjectManifest::load_from_path(&manifest_path).map_err(|e| e.to_string())?;
        let previous_transport = load_repo_transport(&project_dir)?;
        let previous_managed = managed_from_transport(previous_transport.as_ref());
        let new_managed = managed_from_transport(transport.as_ref());
        let mut snapshot_paths: Vec<PathBuf> = previous_managed.iter().map(PathBuf::from).collect();
        snapshot_paths.extend(current.mods.iter().filter_map(|module| {
            module
                .file_name
                .as_ref()
                .map(|name| PathBuf::from(module.content_type.folder_name()).join(name))
        }));
        snapshot_paths.push(PathBuf::from(".tuffbox/transport.json"));
        snapshot_paths.sort();
        snapshot_paths.dedup();
        let snapshot = auto_snapshot_with_managed(
            &manifest_path,
            "github_pack_update",
            &[],
            &["GitHub pack update".into()],
            snapshot_paths,
        )
        .map_err(|e| e.to_string())?;
        let apply_result = apply_managed_files(
            extract_dir.path(),
            &project_dir,
            &previous_managed,
            &new_managed,
        );
        if let Err(e) = apply_result {
            return Err(rollback_after_failure(
                &project_dir,
                &snapshot.id,
                e.to_string(),
            ));
        }
        let diff = diff_manifests(&current, &incoming);
        if diff.requires_full_reinstall() {
            if let Err(e) = remove_obsolete_content_files(&project_dir, &current, &incoming) {
                return Err(rollback_after_failure(
                    &project_dir,
                    &snapshot.id,
                    e.to_string(),
                ));
            }
        }
        if let Err(e) = save_manifest(&manifest_path, &incoming) {
            return Err(rollback_after_failure(
                &project_dir,
                &snapshot.id,
                e.to_string(),
            ));
        }
        if let Err(e) = persist_lockfile_for_manifest(&manifest_path, &incoming) {
            return Err(rollback_after_failure(
                &project_dir,
                &snapshot.id,
                e.to_string(),
            ));
        }
        if let Err(e) = verify_manifest_local_hashes(&project_dir, &incoming) {
            return Err(rollback_after_failure(
                &project_dir,
                &snapshot.id,
                e.to_string(),
            ));
        }
        let local = LocalTransportMeta {
            schema_version: TRANSPORT_SCHEMA_VERSION,
            kind: TransportKind::Github,
            repo: local.repo,
            git_ref: local.git_ref,
            manifest_file: transport
                .as_ref()
                .map(|t| t.manifest_file.clone())
                .unwrap_or(local.manifest_file),
            installed_version: incoming.project.version.clone(),
            installed_commit_sha: Some(remote_sha),
            installed_at: Some(tuffbox_core::time_util::rfc3339_now()),
            pinned_signer_public_key: transport
                .and_then(|t| t.signer_public_key)
                .or(local.pinned_signer_public_key),
        };
        let report =
            crate::download_project_mods_tracked(&app, &manifest_path, &incoming, None, true);
        if !report.failed.is_empty() {
            let failures = report
                .failed
                .iter()
                .map(|failure| format!("{}: {}", failure.mod_id, failure.error))
                .collect::<Vec<_>>()
                .join("; ");
            return Err(rollback_after_failure(
                &project_dir,
                &snapshot.id,
                format!("GitHub pack materialization failed: {failures}"),
            ));
        }
        if let Err(e) = verify_manifest_local_hashes(&project_dir, &incoming) {
            return Err(rollback_after_failure(
                &project_dir,
                &snapshot.id,
                e.to_string(),
            ));
        }
        if let Err(e) = write_local_transport(&project_dir, &local) {
            return Err(rollback_after_failure(&project_dir, &snapshot.id, e));
        }
        Ok(serde_json::json!({
            "ok": true,
            "snapshotId": snapshot.id,
            "version": incoming.project.version,
            "changes": diff.changes,
            "requiresFullReinstall": diff.requires_full_reinstall(),
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

async fn download_tarball(
    owner: &str,
    repo: &str,
    git_ref: &str,
) -> Result<(Vec<u8>, String), String> {
    validate_github_ref(git_ref).map_err(|error| error.to_string())?;
    let client = anonymous_client()?;
    let commit = commit_sha(&client, owner, repo, git_ref).unwrap_or_else(|_| git_ref.to_string());
    validate_github_ref(&commit).map_err(|error| error.to_string())?;
    let url = format!("https://api.github.com/repos/{owner}/{repo}/tarball/{commit}");
    Ok((download_github_bytes(&url).await?, commit))
}

async fn download_tarball_at_commit(
    owner: &str,
    repo: &str,
    commit_sha: &str,
) -> Result<Vec<u8>, String> {
    validate_github_ref(commit_sha).map_err(|error| error.to_string())?;
    let url = format!("https://api.github.com/repos/{owner}/{repo}/tarball/{commit_sha}");
    download_github_bytes(&url).await
}

async fn download_github_bytes(url: &str) -> Result<Vec<u8>, String> {
    let parsed = reqwest::Url::parse(url).map_err(|error| error.to_string())?;
    if parsed.scheme() != "https"
        || !matches!(
            parsed.host_str(),
            Some("api.github.com")
                | Some("uploads.github.com")
                | Some("objects.githubusercontent.com")
                | Some("github.com")
                | Some("codeload.github.com")
        )
    {
        return Err(format!(
            "blocked non-GitHub download host: {}",
            parsed.host_str().unwrap_or("<none>")
        ));
    }
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(90))
        .build()
        .map_err(|error| error.to_string())?;
    let mut req = http
        .get(parsed.clone())
        .header("User-Agent", APP_USER_AGENT)
        .header("Accept", "application/vnd.github+json");
    if parsed.host_str() == Some("api.github.com") {
        if let Some(token) = stored_author_token() {
            req = req.bearer_auth(token);
        }
    }
    let mut response = req
        .send()
        .await
        .map_err(|error| format!("GitHub download failed: {error}"))?
        .error_for_status()
        .map_err(|error| format!("GitHub download failed: {error}"))?;
    if response
        .content_length()
        .is_some_and(|size| size > MAX_GITHUB_DOWNLOAD_BYTES)
    {
        return Err("GitHub download exceeds 512 MiB limit".into());
    }
    let mut bytes = Vec::new();
    loop {
        let chunk = response
            .chunk()
            .await
            .map_err(|error| format!("GitHub download failed: {error}"))?;
        let Some(chunk) = chunk else {
            break;
        };
        let next_len = u64::try_from(bytes.len().saturating_add(chunk.len())).unwrap_or(u64::MAX);
        if next_len > MAX_GITHUB_DOWNLOAD_BYTES {
            return Err("GitHub download exceeds 512 MiB limit".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn enforce_pinned_signer(pinned: Option<&str>, incoming: Option<&str>) -> Result<(), String> {
    match (pinned, incoming) {
        (Some(_), None) => Err("signed GitHub pack update cannot become unsigned".into()),
        (pinned, Some(incoming)) => pin_or_check_signer(pinned, incoming)
            .map(|_| ())
            .map_err(|error| error.to_string()),
        (None, None) => Ok(()),
    }
}

fn rollback_after_failure(project_dir: &Path, snapshot_id: &str, cause: String) -> String {
    match SnapshotStore::new(project_dir).rollback(snapshot_id) {
        Ok(_) => cause,
        Err(rollback_error) => {
            format!("{cause}; rollback {snapshot_id} also failed: {rollback_error}")
        }
    }
}

#[tauri::command(rename_all = "camelCase")]
pub async fn github_pack_publish(
    app: tauri::AppHandle,
    path: String,
    repository: String,
) -> Result<serde_json::Value, String> {
    let token = stored_author_token().ok_or_else(|| {
        "GitHub author token missing. Log in with device flow or paste a PAT in Settings."
            .to_string()
    })?;
    let src = parse_github_source(&repository).map_err(|e| e.to_string())?;
    let signer = Some(author_signing_key()?);
    let manifest_path = PathBuf::from(&path);
    let manifest = ProjectManifest::load_from_path(&manifest_path).map_err(|e| e.to_string())?;
    semver::Version::parse(manifest.project.version.trim()).map_err(|_| {
        format!(
            "pack version {:?} is not semver (expected like 1.2.3)",
            manifest.project.version
        )
    })?;
    persist_lockfile_for_manifest(&manifest_path, &manifest).map_err(|e| e.to_string())?;
    let _ = app.emit(
        "github-pack-progress",
        serde_json::json!({ "phase": "staging" }),
    );
    let staging = tempfile::tempdir().map_err(|e| e.to_string())?;
    let staged = stage_repo_tree(
        &manifest,
        &manifest_path,
        staging.path(),
        None,
        StageOptions {
            owner: Some(src.owner.clone()),
            repo: Some(src.repo.clone()),
            signer,
            ..StageOptions::default()
        },
    )
    .map_err(|e| e.to_string())?;
    let _ = app.emit(
        "github-pack-progress",
        serde_json::json!({ "phase": "commit" }),
    );
    let two_phase = !staged.pending_release_assets.is_empty();
    // Blocking GitHub Data API + asset uploads must not occupy the async IPC
    // executor (same rationale as github_pack_install).
    let publish_app = app.clone();
    let message = format!(
        "TuffBox pack {} {}",
        manifest.project.name, manifest.project.version
    );
    let result = tokio::task::spawn_blocking(move || -> Result<serde_json::Value, String> {
        let client = match LiveGitHubApi::new(Some(token)) {
            Ok(c) => c,
            Err(e) => return Err(e.to_string()),
        };
        // Per-asset progress: emit an index per asset so the UI can show
        // "Uploading Release assets (2/3)…" instead of a single flat step.
        let total = staged.pending_release_assets.len();
        let emit_phase = |phase: &str, done: usize| {
            let _ = publish_app.emit(
                "github-pack-progress",
                serde_json::json!({ "phase": phase, "assetIndex": done, "assetTotal": total }),
            );
        };
        emit_phase("assets", 0);
        for (idx, _) in staged.pending_release_assets.iter().enumerate() {
            // Placeholder markers keep the phase ordering deterministic; the
            // real upload happens inside publish_staged_tree below.
            emit_phase("assets", idx + 1);
        }
        let outcome = publish_staged_tree(
            &client,
            &src.owner,
            &src.repo,
            src.git_ref.as_deref(),
            &staged,
            &message,
        );
        match outcome {
            Ok(result) => Ok(serde_json::json!({
                "ok": true,
                "commitSha": result.commit_sha,
                "branch": result.branch,
                "shareUrl": result.share_url,
                "releaseUrl": result.release_url,
                "twoPhase": result.two_phase,
                "assetCount": total,
                "preview": {
                    "packVersion": staged.transport.pack_version,
                    "fileCount": staged.files.len(),
                    "managedFiles": staged.transport.managed_files,
                },
            })),
            Err(PublishError::NoOp) => Ok(serde_json::json!({
                "ok": true,
                "noop": true,
                "shareUrl": format!("https://github.com/{}/{}", src.owner, src.repo),
                "message": "Nothing to publish — remote already has this pack.",
            })),
            Err(PublishError::Conflict(msg)) => Ok(serde_json::json!({
                "ok": false,
                "conflict": true,
                "message": msg,
            })),
            Err(e) => Err(e.to_string()),
        }
    })
    .await
    .map_err(|e| e.to_string())?;

    let _ = app.emit(
        "github-pack-progress",
        serde_json::json!({ "phase": "done" }),
    );
    let _ = two_phase;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_rejects_existing_nonempty_target() {
        let root = tempfile::tempdir().expect("temp root");
        let target = root.path().join("demo");
        fs::create_dir_all(&target).expect("target directory");
        fs::write(target.join("user-file.txt"), b"keep").expect("user file");

        let error = ensure_install_target_available(&target).expect_err("must reject overwrite");

        assert!(error.contains("not empty"));
        assert_eq!(
            fs::read(target.join("user-file.txt")).expect("preserved user file"),
            b"keep"
        );
    }

    #[test]
    fn install_accepts_missing_or_empty_target() {
        let root = tempfile::tempdir().expect("temp root");
        let missing = root.path().join("missing");
        ensure_install_target_available(&missing).expect("missing target is available");

        fs::create_dir_all(&missing).expect("empty target");
        ensure_install_target_available(&missing).expect("empty target is available");
    }

    #[test]
    fn renamed_instance_removes_stale_manifest_copy() {
        let root = tempfile::tempdir().expect("temp root");
        let old = root.path().join("upstream.tuffbox.json");
        let canonical = root.path().join("renamed.tuffbox.json");
        fs::write(&old, b"old").expect("old manifest");
        fs::write(&canonical, b"new").expect("canonical manifest");

        remove_stale_manifest(&old, &canonical).expect("remove stale manifest");

        assert!(!old.exists());
        assert_eq!(fs::read(canonical).expect("canonical remains"), b"new");
    }

    #[test]
    fn staged_install_is_promoted_only_into_available_target() {
        let root = tempfile::tempdir().expect("temp root");
        let staged = root.path().join("staged");
        let target = root.path().join("target");
        fs::create_dir_all(&staged).expect("staged directory");
        fs::write(staged.join("verified.txt"), b"ok").expect("staged file");

        promote_staged_install(&staged, &target).expect("promote verified tree");

        assert!(!staged.exists());
        assert_eq!(
            fs::read(target.join("verified.txt")).expect("target file"),
            b"ok"
        );
    }

    #[test]
    fn pinned_signer_rejects_unsigned_downgrade() {
        let error = enforce_pinned_signer(Some("pinned-key"), None)
            .expect_err("signed install must reject unsigned update");

        assert!(error.contains("unsigned"));
    }
}
