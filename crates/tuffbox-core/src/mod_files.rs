//! Materializing mod/resourcepack/shaderpack/datapack files on disk.
//!
//! The manifest only stores *metadata* about an entry (source, version,
//! url, hashes, content type). Nothing elsewhere in the codebase used to
//! turn that metadata into an actual file inside the right instance
//! folder, which meant a freshly-added Modrinth mod would show up in the
//! UI/manifest but Minecraft would launch without it — and, separately,
//! resourcepacks/shaderpacks/datapacks were all written into `mods/`
//! regardless of their actual type. This module is the single place
//! responsible for keeping each content folder in sync with what the
//! manifest declares.

use crate::manifest::{ContentType, ModSpec, ProjectManifest, Side, SourceKind};
use crate::mc_install::{sha1_file, InstallError};
use crate::provider::{ContentProvider, ModrinthProvider};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ModFileError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("install error: {0}")]
    Install(#[from] InstallError),
    #[error("mod {0} has no file name")]
    NoFileName(String),
    #[error("mod {0} has no download url")]
    NoDownloadUrl(String),
}

/// Result of trying to materialize a single mod file.
#[derive(Debug, Clone)]
pub enum MaterializeOutcome {
    /// The file already existed on disk and matched the expected hash (or no
    /// hash was known, in which case existence alone was accepted).
    AlreadyPresent,
    /// The file was downloaded.
    Downloaded,
    /// The mod has no remote source to download from (e.g. purely local
    /// mods copied in by the user); nothing to do.
    Skipped,
}

/// Path to the `mods/` folder for a project, given the manifest path.
pub fn mods_dir_for_manifest(manifest_path: &Path) -> Option<PathBuf> {
    manifest_path.parent().map(|dir| dir.join("mods"))
}

/// Path to the project's instance root (the manifest's parent directory).
pub fn instance_dir_for_manifest(manifest_path: &Path) -> Option<PathBuf> {
    manifest_path.parent().map(|dir| dir.to_path_buf())
}

/// Resolves the correct target folder for a content entry under the
/// instance root, based on its [`ContentType`].
///
/// Note: datapacks are inherently world-specific in vanilla Minecraft
/// (they live under `saves/<world>/datapacks/`). Since a project can have
/// multiple/no worlds yet, datapack entries are tracked under a top-level
/// `datapacks/` folder in the project; exporters are responsible for
/// copying them into the target world when packaging.
pub fn content_dir_for(instance_dir: &Path, content_type: ContentType) -> PathBuf {
    instance_dir.join(content_type.folder_name())
}

/// Ensures a single content entry's file is present inside its
/// content-type-appropriate folder under `instance_dir`, downloading it
/// from `source.url` if missing or empty.
///
/// Warm launches skip SHA-1: hashes are verified at download time. Re-reading
/// every jar on Play (a 200-mod pack is hundreds of MB) is the common stall.
pub fn materialize_mod_file(
    instance_dir: &Path,
    module: &ModSpec,
) -> Result<MaterializeOutcome, ModFileError> {
    materialize_mod_file_with_progress(
        instance_dir,
        module,
        &crate::mc_install::ProgressCallback::new(),
    )
}

/// Like `materialize_mod_file`, but invokes `progress` with per-chunk
/// byte progress as the file is downloaded.
pub fn materialize_mod_file_with_progress(
    instance_dir: &Path,
    module: &ModSpec,
    progress: &crate::mc_install::ProgressCallback,
) -> Result<MaterializeOutcome, ModFileError> {
    // Local/"drop-in" mods are expected to already exist in the folder; we
    // don't have anywhere to download them from.
    if matches!(module.source.kind, SourceKind::Local) {
        return Ok(MaterializeOutcome::Skipped);
    }

    let Some(file_name) = &module.file_name else {
        return Err(ModFileError::NoFileName(module.id.clone()));
    };

    let target_dir = content_dir_for(instance_dir, module.content_type);
    let target = target_dir.join(file_name);

    // Fast path for Play: if the jar is already on disk, do not SHA-1 it and
    // do not resolve CurseForge CDN URLs. Both used to run on every launch.
    if target.is_file() {
        if std::fs::metadata(&target)
            .map(|m| m.len() > 0)
            .unwrap_or(false)
        {
            return Ok(MaterializeOutcome::AlreadyPresent);
        }
    }

    // Resolve CurseForge projectID/fileID → CDN URL only when we actually
    // need to download (missing or empty file).
    let mut resolved_url = module.source.url.clone();
    if matches!(module.source.kind, SourceKind::Curseforge) {
        let needs_resolve = resolved_url
            .as_ref()
            .map(|u| {
                u.trim().is_empty()
                    || u.contains("curseforge.com/minecraft") && !u.contains("forgecdn.net")
            })
            .unwrap_or(true);
        if needs_resolve {
            if let (Some(pid), Some(fid)) = (
                module
                    .source
                    .project_id
                    .as_ref()
                    .and_then(|s| s.parse().ok()),
                module.source.file_id.as_ref().and_then(|s| s.parse().ok()),
            ) {
                if let Ok(info) = crate::provider::CurseForgeProvider::new().get_file(pid, fid) {
                    if let Some(url) = info.resolved_download_url() {
                        resolved_url = Some(url);
                    }
                }
            }
        }
    }

    let Some(url) = &resolved_url else {
        return Err(ModFileError::NoDownloadUrl(module.id.clone()));
    };

    std::fs::create_dir_all(&target_dir)?;
    let expected_sha1 = module.hashes.as_ref().and_then(|h| h.sha1.as_deref());

    // Per-pack opt-out (`.tuffbox-no-dedup` in the project root): this pack
    // keeps fully independent files — no store consult, no recording. The
    // user can flip it back any time in Project Settings; turning it off
    // again materializes existing links back into plain copies.
    let dedup_off = crate::mod_store::dedup_disabled(instance_dir);

    // Dedup store (Shard-inspired): if the jar's exact bytes are already in
    // the shared object store, hard-link them into this instance — no
    // download, no extra disk usage. Requires a known sha1.
    if !dedup_off {
        if let Some(sha1) = expected_sha1 {
            if crate::mod_store::try_hardlink(&target, sha1) {
                return Ok(MaterializeOutcome::AlreadyPresent);
            }
        }
    }

    let download_result = if crate::provider::curseforge::url_needs_curseforge_key(url) {
        download_with_curseforge_key(url, &target, expected_sha1, &module.id, progress)
    } else {
        crate::mc_install::download_with_progress(url, &target, expected_sha1, &module.id, progress)
    };

    match download_result {
        Ok(()) => {
            // Feed the dedup store so other instances get this jar for free.
            if !dedup_off {
                if let Some(sha1) = expected_sha1 {
                    crate::mod_store::record(&target, sha1);
                }
            }
            return Ok(MaterializeOutcome::Downloaded);
        }
        Err(primary_error) => {
            let Some(sha1) = expected_sha1 else {
                return Err(ModFileError::Install(primary_error));
            };
            let mut api_url = format!(
                "https://api.modrinth.com/v2/version_file/{}/download?algorithm=sha1",
                sha1
            );
            if let Some(version_id) = module.source.file_id.as_deref() {
                api_url.push_str("&version_id=");
                api_url.push_str(version_id);
            }
            crate::mc_install::download_with_progress(
                &api_url,
                &target,
                Some(sha1),
                &module.id,
                progress,
            )
            .map_err(|fallback_error| {
                ModFileError::Install(crate::mc_install::InstallError::MissingDownload(format!(
                    "cdn failed ({primary_error}); modrinth redirect failed ({fallback_error})"
                )))
            })?;
            Ok(MaterializeOutcome::Downloaded)
        }
    }
}

fn download_with_curseforge_key(
    url: &str,
    path: &Path,
    expected_sha1: Option<&str>,
    id: &str,
    progress: &crate::mc_install::ProgressCallback,
) -> Result<(), InstallError> {
    use sha1::{Digest, Sha1};
    use std::io::{Read, Write};

    let api_key = crate::provider::CurseForgeProvider::new()
        .api_key()
        .to_string();
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .user_agent("TuffBox-IDE/0.1.0")
        .redirect(reqwest::redirect::Policy::limited(8))
        .build()
        .map_err(|e| InstallError::MissingDownload(e.to_string()))?;

    let response = client
        .get(url)
        .header("x-api-key", api_key)
        .send()
        .map_err(|e| InstallError::MissingDownload(format!("{e} (url: {url})")))?;
    if !response.status().is_success() {
        return Err(InstallError::MissingDownload(format!(
            "HTTP {} for {url}",
            response.status()
        )));
    }

    let total_size = response.content_length().unwrap_or(0);
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut file = tempfile::Builder::new()
        .prefix(".tuffbox-download-")
        .suffix(".part")
        .tempfile_in(parent)
        .map_err(InstallError::Io)?;
    let mut hasher = Sha1::new();
    let mut received: u64 = 0;
    let mut stream = response;
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let n = stream.read(&mut buffer).map_err(InstallError::Io)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
        file.write_all(&buffer[..n]).map_err(InstallError::Io)?;
        received += n as u64;
        progress.call(id, received, total_size);
    }
    file.flush().map_err(InstallError::Io)?;
    let actual = format!("{:x}", hasher.finalize());
    if let Some(expected) = expected_sha1 {
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(InstallError::MissingDownload(format!(
                "sha1 mismatch: expected {expected}, got {actual}"
            )));
        }
    }
    file.persist(path).map_err(|e| InstallError::Io(e.error))?;
    Ok(())
}

/// Report for a full mods-folder sync pass.
#[derive(Debug, Default, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModSyncReport {
    pub downloaded: Vec<String>,
    pub already_present: Vec<String>,
    pub skipped: Vec<String>,
    pub failed: Vec<ModSyncFailure>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModSyncFailure {
    pub mod_id: String,
    pub error: String,
}

/// Downloads every entry declared in the manifest that isn't already
/// present (with a matching hash) in its content-type-appropriate folder
/// under `instance_dir` (`mods/`, `resourcepacks/`, `shaderpacks/`,
/// `datapacks/`).
///
/// This is the safety net that runs before a test launch: even if an entry
/// was added to the manifest through some path that didn't already
/// download it, the game will still see the right files before Java
/// starts.
///
/// Downloads run in parallel via rayon (up to the global thread pool size)
/// using the streaming downloader — each file is written to a `.part` temp
/// file and atomically renamed on success, so a failed or interrupted
/// download never leaves a half-written jar in the mods folder.
///
/// If `progress` is provided, it is invoked with `(mod_id, bytes_received,
/// total_bytes)` as each chunk is received, enabling real-time UI progress
/// bars without buffering whole files in memory.
pub fn ensure_project_mods_downloaded(
    manifest: &ProjectManifest,
    instance_dir: &Path,
) -> ModSyncReport {
    ensure_project_mods_downloaded_with_progress(
        manifest,
        instance_dir,
        &crate::mc_install::ProgressCallback::new(),
    )
}

/// Like `ensure_project_mods_downloaded`, but polls `task_progress` for a
/// cancel request on the given task id between files (cooperative cancel).
pub fn ensure_project_mods_downloaded_cancellable(
    manifest: &ProjectManifest,
    instance_dir: &Path,
    progress: &crate::mc_install::ProgressCallback,
    cancel_task_id: Option<&str>,
) -> ModSyncReport {
    ensure_project_mods_downloaded_with_progress_filtered_impl(
        manifest,
        instance_dir,
        progress,
        None,
        cancel_task_id.map(|s| s.to_string()),
    )
}

/// Same as `ensure_project_mods_downloaded`, but with a progress callback
/// that fires per-chunk during each download.
pub fn ensure_project_mods_downloaded_with_progress(
    manifest: &ProjectManifest,
    instance_dir: &Path,
    progress: &crate::mc_install::ProgressCallback,
) -> ModSyncReport {
    ensure_project_mods_downloaded_with_progress_filtered(manifest, instance_dir, progress, None)
}

/// Like [`ensure_project_mods_downloaded_with_progress`], but only materializes
/// mods whose ids appear in `only_mod_ids` when that set is provided.
pub fn ensure_project_mods_downloaded_with_progress_filtered(
    manifest: &ProjectManifest,
    instance_dir: &Path,
    progress: &crate::mc_install::ProgressCallback,
    only_mod_ids: Option<&std::collections::HashSet<String>>,
) -> ModSyncReport {
    ensure_project_mods_downloaded_with_progress_filtered_impl(
        manifest,
        instance_dir,
        progress,
        only_mod_ids,
        None,
    )
}

fn ensure_project_mods_downloaded_with_progress_filtered_impl(
    manifest: &ProjectManifest,
    instance_dir: &Path,
    progress: &crate::mc_install::ProgressCallback,
    only_mod_ids: Option<&std::collections::HashSet<String>>,
    cancel_task_id: Option<String>,
) -> ModSyncReport {
    use rayon::prelude::*;
    use std::sync::atomic::Ordering;
    use std::sync::Mutex;

    let report = Mutex::new(ModSyncReport::default());
    let progress = progress.clone();

    // Modrinth-style bounded concurrency: unbounded par_iter over 30–80 mods
    // saturates the CDNs' per-host limits and trips circuit breakers, which
    // made pack installs SLOWER than fewer parallel streams. Reuse the same
    // user-configurable limit as the asset/library batch downloader.
    let limit = crate::download_engine::configured_concurrency();
    let in_flight = std::sync::atomic::AtomicUsize::new(0);
    let gate = |active: &std::sync::atomic::AtomicUsize| loop {
        let cur = active.load(Ordering::SeqCst);
        if cur < limit
            && active
                .compare_exchange(cur, cur + 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
        {
            break;
        }
        std::thread::yield_now();
    };

    manifest
        .mods
        .par_iter()
        .filter(|module| {
            // Cooperative cancellation (Millida jobs.rs-style): the task id
            // is the instance's sync key; long sweeps bail between files.
            if cancel_task_id
                .as_deref()
                .is_some_and(|id| crate::task_progress::is_cancel_requested(id))
            {
                report.lock().unwrap_or_else(|e| e.into_inner()).skipped.push(format!(
                    "__cancelled__{}",
                    module.id
                ));
                return false;
            }
            if module
                .status
                .iter()
                .any(|s| s.eq_ignore_ascii_case("disabled"))
            {
                return false;
            }
            only_mod_ids
                .map(|ids| ids.contains(&module.id))
                .unwrap_or(true)
        })
        .for_each_init(
            || gate(&in_flight),
            |_permit, module| {
                let outcome = materialize_mod_file_with_progress(instance_dir, module, &progress);
                in_flight.fetch_sub(1, Ordering::SeqCst);
                let mut report = report.lock().unwrap_or_else(|e| e.into_inner());
                match outcome {
                    Ok(MaterializeOutcome::Downloaded) => report.downloaded.push(module.id.clone()),
                    Ok(MaterializeOutcome::AlreadyPresent) => {
                        report.already_present.push(module.id.clone())
                    }
                    Ok(MaterializeOutcome::Skipped) => report.skipped.push(module.id.clone()),
                    Err(e) => report.failed.push(ModSyncFailure {
                        mod_id: module.id.clone(),
                        error: e.to_string(),
                    }),
                }
            },
        );

    report.into_inner().unwrap()
}

/// Removes mod files from `mods_dir` that are no longer declared for the
/// given profile side, and (re)downloads/validates the ones that are.
///
/// This keeps a shared project `mods/` folder honest when the user removes a
/// mod from the manifest, or when switching between client/server profiles
/// that include different mod sets.
pub fn sync_mods_dir_to_manifest(
    manifest: &ProjectManifest,
    mods_dir: &Path,
) -> Result<ModSyncReport, ModFileError> {
    std::fs::create_dir_all(mods_dir)?;

    // Orphan cleanup reserved for stricter modes later:
    // let expected_file_names: std::collections::HashSet<String> = manifest.mods.iter()
    //     .filter(|m| m.content_type == ContentType::Mod)
    //     .filter_map(|m| m.file_name.clone()).collect();

    let instance_dir = mods_dir.parent().unwrap_or(mods_dir);
    Ok(ensure_project_mods_downloaded(manifest, instance_dir))
}

/// Attempts to identify an untracked local `.jar` file against Modrinth by
/// content hash, returning a fully-populated [`ModSpec`] if a match is
/// found. Side is taken from the project's Modrinth `client_side` /
/// `server_side` fields.
pub fn identify_local_jar_via_modrinth(
    provider: &ModrinthProvider,
    jar_path: &Path,
) -> Result<Option<(ModSpec, Option<String>, Option<String>)>, ModFileError> {
    let sha1 = sha1_file(jar_path)?;
    let Some((project, version)) = provider
        .identify_local_jar(&sha1)
        .map_err(|e| ModFileError::Io(std::io::Error::other(e.to_string())))?
    else {
        return Ok(None);
    };

    let file = crate::provider::ProviderFileInfo::primary_file(&version).cloned();
    let file_name = jar_path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown.jar".to_string());

    let content_type = ContentType::from_modrinth_project_type(&project.project_type);

    let dependencies = provider
        .resolve_dependencies(&version.id)
        .unwrap_or_default();

    let side = Side::from_modrinth(
        project.client_side.as_deref(),
        project.server_side.as_deref(),
    );
    let authors = project
        .author
        .as_ref()
        .map(|a| vec![a.clone()])
        .unwrap_or_default();

    Ok(Some((
        ModSpec {
            id: project.slug.clone(),
            name: project.name,
            source: crate::manifest::ModSource {
                kind: SourceKind::Modrinth,
                project_id: Some(project.id),
                file_id: Some(version.id),
                url: file.as_ref().map(|f| f.url.clone()),
                path: None,
                icon_url: project.icon_url.clone(),
                categories: project.categories.clone(),
            },
            version: version.version_number,
            file_name: Some(file_name),
            hashes: Some(crate::manifest::FileHashes {
                sha1: Some(sha1),
                sha512: file.and_then(|f| f.hashes.sha512),
            }),
            side,
            dependencies,
            status: vec!["ok".to_string()],
            content_type,
            authors,
            option: None,
        },
        project.client_side,
        project.server_side,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{ModSource, Side};

    fn sample_mod(kind: SourceKind, file_name: Option<&str>, url: Option<&str>) -> ModSpec {
        sample_content(kind, file_name, url, ContentType::Mod)
    }

    fn sample_content(
        kind: SourceKind,
        file_name: Option<&str>,
        url: Option<&str>,
        content_type: ContentType,
    ) -> ModSpec {
        ModSpec {
            id: "example".to_string(),
            name: "Example".to_string(),
            source: ModSource {
                kind,
                project_id: Some("example".to_string()),
                file_id: Some("v1".to_string()),
                url: url.map(|s| s.to_string()),
                path: None,
                icon_url: None,
                categories: Vec::new(),
            },
            version: "1.0.0".to_string(),
            file_name: file_name.map(|s| s.to_string()),
            hashes: None,
            side: Side::Both,
            dependencies: Vec::new(),
            status: vec!["ok".to_string()],
            content_type,
            authors: Vec::new(),
            option: None,
        }
    }

    #[test]
    fn local_mods_are_skipped_not_downloaded() {
        let dir = tempfile::tempdir().unwrap();
        let module = sample_mod(SourceKind::Local, Some("example.jar"), None);
        let outcome = materialize_mod_file(dir.path(), &module).unwrap();
        assert!(matches!(outcome, MaterializeOutcome::Skipped));
    }

    #[test]
    fn remote_mod_without_url_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let module = sample_mod(SourceKind::Modrinth, Some("example.jar"), None);
        let result = materialize_mod_file(dir.path(), &module);
        assert!(matches!(result, Err(ModFileError::NoDownloadUrl(_))));
    }

    #[test]
    fn remote_mod_without_file_name_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let module = sample_mod(
            SourceKind::Modrinth,
            None,
            Some("https://example.com/mod.jar"),
        );
        let result = materialize_mod_file(dir.path(), &module);
        assert!(matches!(result, Err(ModFileError::NoFileName(_))));
    }

    #[test]
    fn existing_file_without_hash_is_accepted_without_download() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("mods")).unwrap();
        std::fs::write(dir.path().join("mods").join("example.jar"), b"already here").unwrap();
        let module = sample_mod(
            SourceKind::Modrinth,
            Some("example.jar"),
            Some("https://example.invalid/should-not-be-fetched.jar"),
        );
        let outcome = materialize_mod_file(dir.path(), &module).unwrap();
        assert!(matches!(outcome, MaterializeOutcome::AlreadyPresent));
    }

    #[test]
    fn existing_file_with_sha1_skips_rehash_and_redownload() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("mods")).unwrap();
        std::fs::write(dir.path().join("mods").join("example.jar"), b"already here").unwrap();
        let mut module = sample_mod(
            SourceKind::Modrinth,
            Some("example.jar"),
            Some("https://example.invalid/should-not-be-fetched.jar"),
        );
        module.hashes = Some(crate::manifest::FileHashes {
            sha1: Some("deadbeef".into()),
            sha512: None,
        });
        let outcome = materialize_mod_file(dir.path(), &module).unwrap();
        assert!(matches!(outcome, MaterializeOutcome::AlreadyPresent));
    }

    #[test]
    fn curseforge_existing_file_skips_api_even_without_url() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("mods")).unwrap();
        std::fs::write(dir.path().join("mods").join("example.jar"), b"cf-mod").unwrap();
        let module = sample_mod(SourceKind::Curseforge, Some("example.jar"), None);
        let outcome = materialize_mod_file(dir.path(), &module).unwrap();
        assert!(matches!(outcome, MaterializeOutcome::AlreadyPresent));
    }

    #[test]
    fn empty_on_disk_file_is_not_treated_as_present() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("mods")).unwrap();
        std::fs::write(dir.path().join("mods").join("example.jar"), b"").unwrap();
        let module = sample_mod(SourceKind::Modrinth, Some("example.jar"), None);
        let result = materialize_mod_file(dir.path(), &module);
        assert!(matches!(result, Err(ModFileError::NoDownloadUrl(_))));
    }

    #[test]
    fn mods_dir_for_manifest_appends_mods_folder() {
        let manifest_path = Path::new("/tmp/project/pack.tuffbox.json");
        let mods_dir = mods_dir_for_manifest(manifest_path).unwrap();
        assert_eq!(mods_dir, Path::new("/tmp/project/mods"));
    }

    #[test]
    fn resourcepacks_are_routed_to_resourcepacks_folder_not_mods() {
        let dir = tempfile::tempdir().unwrap();
        let module = sample_content(
            SourceKind::Modrinth,
            Some("pack.zip"),
            Some("https://example.invalid/should-not-be-fetched.zip"),
            ContentType::Resourcepack,
        );
        // Pre-place the file where it *should* end up so no network call happens.
        std::fs::create_dir_all(dir.path().join("resourcepacks")).unwrap();
        std::fs::write(dir.path().join("resourcepacks").join("pack.zip"), b"pack").unwrap();

        let outcome = materialize_mod_file(dir.path(), &module).unwrap();
        assert!(matches!(outcome, MaterializeOutcome::AlreadyPresent));
        assert!(!dir.path().join("mods").join("pack.zip").exists());
    }

    #[test]
    fn shaderpacks_are_routed_to_shaderpacks_folder() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            content_dir_for(dir.path(), ContentType::Shaderpack),
            dir.path().join("shaderpacks")
        );
        assert_eq!(
            content_dir_for(dir.path(), ContentType::Datapack),
            dir.path().join("datapacks")
        );
        assert_eq!(
            content_dir_for(dir.path(), ContentType::Mod),
            dir.path().join("mods")
        );
    }
}
