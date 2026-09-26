use rayon::prelude::*;
use serde::Deserialize;
use sha1::Digest;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    sync::{Arc, Mutex},
};
use thiserror::Error;

// In-process cache of resolved `InstalledVersion`s, keyed by the
// (minecraft version, loader kind, loader version) triple. Rebuilding the
// installed-version metadata (scanning every library jar, reading the asset
// index, re-extracting natives, merging loader profiles) is pure overhead
// when nothing has changed — which is the common case for repeated test
// launches. We still re-validate that the on-disk files still exist before
// trusting a cached entry, so a deleted/corrupted install falls through to
// a real re-install instead of launching a broken game.
lazy_static::lazy_static! {
    static ref INSTALL_CACHE: Mutex<std::collections::HashMap<String, InstalledVersion>> =
        Mutex::new(std::collections::HashMap::new());
}

fn install_cache_key(mc_version: &str, loader_kind: &str, loader_version: &str) -> String {
    format!("{mc_version}|{loader_kind}|{loader_version}")
}

fn cached_install(key: &str) -> Option<InstalledVersion> {
    let cache = INSTALL_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let version = cache.get(key)?;
    // Re-validate the critical files still exist; otherwise drop the cache.
    let ok = version.client_jar.exists()
        && version.libraries.iter().all(|p| p.exists())
        && version.natives_dir.exists();
    if ok {
        Some(version.clone())
    } else {
        None
    }
}

fn store_install(key: &str, version: InstalledVersion) {
    let mut cache = INSTALL_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache.insert(key.to_string(), version);
}

/// On-disk mirror of `INSTALL_CACHE` so resolved install metadata survives a
/// Tauri/process restart. Keyed the same way; stored as
/// `<launcher_dir>/install-cache/<key>.json`. Paths inside are absolute, so
/// the cache is only valid on the same machine — which is fine, the install
/// dir itself is machine-local too.
fn disk_cache_path(launcher_dir: &Path, key: &str) -> PathBuf {
    launcher_dir
        .join("install-cache")
        .join(key.replace(['|', '/', '\\', ':'], "_"))
        .with_extension("json")
}

fn load_disk_install(launcher_dir: &Path, key: &str) -> Option<InstalledVersion> {
    let path = disk_cache_path(launcher_dir, key);
    let text = fs::read_to_string(&path).ok()?;
    let version: InstalledVersion = serde_json::from_str(&text).ok()?;
    // Same validation as the in-process cache: if the install files are gone,
    // drop the cached metadata and let a real install happen.
    let ok = version.client_jar.exists()
        && version.libraries.iter().all(|p| p.exists())
        && version.natives_dir.exists();
    if ok {
        Some(version)
    } else {
        let _ = fs::remove_file(&path);
        None
    }
}

fn store_disk_install(launcher_dir: &Path, key: &str, version: &InstalledVersion) {
    let cache_dir = launcher_dir.join("install-cache");
    if fs::create_dir_all(&cache_dir).is_err() {
        return;
    }
    let path = disk_cache_path(launcher_dir, key);
    if let Ok(text) = serde_json::to_string_pretty(version) {
        let _ = fs::write(&path, text);
    }
}

/// Drop cached metadata and extracted natives after an integrity failure.
/// Native archives may still be valid in `libraries/`, but their old per-archive
/// markers would otherwise suppress re-extraction of a file antivirus or a
/// partial cleanup removed from `natives/`.
fn invalidate_cached_install(launcher_dir: &Path, key: &str, version: &InstalledVersion) {
    INSTALL_CACHE
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .remove(key);
    let _ = fs::remove_file(disk_cache_path(launcher_dir, key));
    let _ = fs::remove_dir_all(&version.natives_dir);
}

const MOJANG_VERSION_MANIFEST: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const RESOURCES_URL: &str = "https://resources.download.minecraft.net";

#[derive(Debug, Error)]
pub enum InstallError {
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("archive error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("missing download: {0}")]
    MissingDownload(String),
    #[error("unsupported loader: {0}")]
    UnsupportedLoader(String),
    #[error("path traversal in archive: {0}")]
    PathTraversal(String),
    #[error("installed game integrity check failed: {0}")]
    Integrity(String),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InstalledVersion {
    pub id: String,
    pub main_class: String,
    pub client_jar: PathBuf,
    pub libraries: Vec<PathBuf>,
    pub natives_dir: PathBuf,
    pub asset_index_id: String,
    pub asset_dir: PathBuf,
    pub log_config: Option<PathBuf>,
    pub jvm_args: Vec<String>,
    pub game_args: Vec<String>,
}

/// Verify that an installed Minecraft runtime is safe to hand to the JVM.
///
/// Downloads already validate SHA-1 values, so this launch-time preflight is
/// intentionally metadata based: it catches the failures users actually hit
/// after an interrupted cleanup / antivirus action (missing or truncated jars,
/// assets, or native extraction) without hashing every asset on every Play.
/// The asset index carries the expected byte length for every object, which is
/// enough to identify partial downloads and lets `install_game` repair them.
pub fn verify_install_integrity(version: &InstalledVersion) -> Result<(), InstallError> {
    verify_nonempty_file(&version.client_jar, "Minecraft client jar")?;
    if version.libraries.is_empty() {
        return Err(InstallError::Integrity("classpath has no libraries".to_string()));
    }
    for library in &version.libraries {
        verify_nonempty_file(library, "classpath library")?;
    }

    // Modern Minecraft versions (1.13+) don't ship native classifiers; the
    // natives_dir is created empty during install.  Only enforce the check
    // when the directory actually exists *and* is non-empty — an empty dir
    // means no natives were expected.
    if version.natives_dir.is_dir()
        && fs::read_dir(&version.natives_dir)?
            .filter_map(|e| e.ok())
            .any(|e| e.path().is_file())
        && !has_native_library(&version.natives_dir)?
    {
        return Err(InstallError::Integrity(format!(
            "native library extraction incomplete in {}",
            version.natives_dir.display()
        )));
    }

    let index_path = version
        .asset_dir
        .join("indexes")
        .join(format!("{}.json", version.asset_index_id));
    verify_nonempty_file(&index_path, "asset index")?;
    let asset_index: AssetIndex = serde_json::from_str(&fs::read_to_string(&index_path)?)?;
    for (name, object) in &asset_index.objects {
        let hash = object.hash.trim();
        if hash.len() < 2 {
            return Err(InstallError::Integrity(format!(
                "asset '{name}' has an invalid SHA-1 in {}",
                index_path.display()
            )));
        }
        let object_path = version.asset_dir.join("objects").join(&hash[..2]).join(hash);
        let metadata = fs::metadata(&object_path).map_err(|_| {
            InstallError::Integrity(format!("asset '{name}' is missing: {}", object_path.display()))
        })?;
        if !metadata.is_file() {
            return Err(InstallError::Integrity(format!(
                "asset '{name}' is not a file: {}",
                object_path.display()
            )));
        }
        if object.size > 0 && metadata.len() != object.size {
            return Err(InstallError::Integrity(format!(
                "asset '{name}' has {} bytes; expected {}",
                metadata.len(), object.size
            )));
        }
    }

    Ok(())
}

fn verify_nonempty_file(path: &Path, label: &str) -> Result<(), InstallError> {
    let metadata = fs::metadata(path).map_err(|_| {
        InstallError::Integrity(format!("{label} is missing: {}", path.display()))
    })?;
    if !metadata.is_file() || metadata.len() == 0 {
        return Err(InstallError::Integrity(format!(
            "{label} is empty or not a file: {}",
            path.display()
        )));
    }
    Ok(())
}

fn has_native_library(dir: &Path) -> Result<bool, InstallError> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            if has_native_library(&path)? {
                return Ok(true);
            }
            continue;
        }
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if matches!(extension.as_str(), "dll" | "so" | "dylib" | "jnilib") {
            return Ok(true);
        }
    }
    Ok(false)
}

pub struct InstallProgress {
    pub log_path: PathBuf,
}

impl InstallProgress {
    pub fn log(&self, msg: &str) {
        if let Ok(mut f) = fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.log_path)
        {
            let _ = writeln!(f, "{msg}");
        }
    }
}

/// Per-file byte progress callback: `(mod_or_file_id, bytes_received, total_bytes)`.
/// Used by streaming downloads to report real-time progress to the UI without
/// buffering the whole file in memory.
pub type DownloadProgressFn = dyn Fn(&str, u64, u64) + Send + Sync;

/// Thread-safe wrapper around an optional progress callback so it can be
/// shared across rayon worker threads during parallel downloads.
/// Internally wraps the closure in `Arc` so the outer `ProgressCallback`
/// is cheaply cloneable.
#[derive(Clone)]
pub struct ProgressCallback(pub Arc<Option<Box<DownloadProgressFn>>>);

impl std::fmt::Debug for ProgressCallback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProgressCallback").finish()
    }
}

impl ProgressCallback {
    pub fn new() -> Self {
        Self(Arc::new(None))
    }

    pub fn with<F: Fn(&str, u64, u64) + Send + Sync + 'static>(f: F) -> Self {
        Self(Arc::new(Some(Box::new(f))))
    }

    pub fn call(&self, id: &str, received: u64, total: u64) {
        if let Some(cb) = self.0.as_ref() {
            cb(id, received, total);
        }
    }

    pub fn is_some(&self) -> bool {
        self.0.is_some()
    }
}

pub fn install_game(
    mc_version_raw: &str,
    loader_kind: &str,
    loader_version: &str,
    launcher_dir: &Path,
    java_path: &Path,
    progress: &InstallProgress,
) -> Result<InstalledVersion, InstallError> {
    // Resolve symbolic versions (`latest`, `release`, `snapshot`, ...) to a
    // concrete id up front. Without this, manifests that store an alias get
    // an exact `.find()` against the version manifest and fail with the
    // cryptic "Minecraft latest not found in manifest". Concrete versions
    // pass through untouched and make no network call.
    let resolved =
        crate::versions::resolve_minecraft_version_alias_offline(mc_version_raw, launcher_dir)
            .map_err(|e| {
                InstallError::MissingDownload(format!(
                    "could not resolve Minecraft version '{}': {e}. \
                 Pin a concrete version (e.g. 1.20.1) or check your network connection.",
                    mc_version_raw
                ))
            })?;
    let mc_version: &str = &resolved;
    if mc_version != mc_version_raw {
        progress.log(&format!(
            "# Resolved Minecraft '{}' -> {}",
            mc_version_raw, mc_version
        ));
    }

    let versions_dir = launcher_dir.join("versions");
    let libraries_dir = launcher_dir.join("libraries");
    let assets_dir = launcher_dir.join("assets");
    let natives_base = launcher_dir.join("natives");

    // Fast path: a previous launch (this process or a prior run, via the
    // on-disk cache) already resolved this exact version + loader combo and
    // the on-disk files are still present. Skip the whole
    // re-scan/re-extract/re-merge work below.
    let cache_key = install_cache_key(mc_version, loader_kind, loader_version);
    if let Some(cached) = cached_install(&cache_key) {
        match verify_install_integrity(&cached) {
            Ok(()) => {
                progress.log("# Using in-process cached install metadata (integrity verified).");
                return Ok(cached);
            }
            Err(error) => {
                progress.log(&format!("# Cached install needs repair: {error}"));
                invalidate_cached_install(launcher_dir, &cache_key, &cached);
            }
        }
    }
    if let Some(cached) = load_disk_install(launcher_dir, &cache_key) {
        match verify_install_integrity(&cached) {
            Ok(()) => {
                progress.log("# Using on-disk cached install metadata (integrity verified).");
                store_install(&cache_key, cached.clone());
                return Ok(cached);
            }
            Err(error) => {
                progress.log(&format!("# Cached install needs repair: {error}"));
                invalidate_cached_install(launcher_dir, &cache_key, &cached);
            }
        }
    }

    fs::create_dir_all(&versions_dir)?;
    fs::create_dir_all(&libraries_dir)?;
    fs::create_dir_all(&assets_dir)?;

    progress.log(&format!("# Checking Minecraft {mc_version}..."));
    let mut vanilla = install_vanilla(
        mc_version,
        &versions_dir,
        &libraries_dir,
        &assets_dir,
        &natives_base,
        progress,
    )?;

    if loader_kind == "fabric" || loader_kind == "quilt" {
        progress.log(&format!(
            "# Fetching {loader_kind} loader {loader_version}..."
        ));
        let fabric_profile = fetch_fabric_profile(
            mc_version,
            loader_version,
            loader_kind,
            launcher_dir,
            progress,
        )?;
        merge_fabric_profile(
            &mut vanilla,
            fabric_profile,
            &libraries_dir,
            mc_version,
            progress,
        )?;
    } else if loader_kind == "forge" {
        progress.log(&format!("# Fetching Forge {loader_version}..."));
        let forge_profile = crate::forge_install::fetch_forge_profile(mc_version, loader_version)?;
        crate::forge_install::download_forge_libraries(&forge_profile, &libraries_dir, progress)?;
        crate::forge_install::run_forge_processors(
            &forge_profile,
            &libraries_dir,
            launcher_dir,
            &vanilla.client_jar,
            mc_version,
            java_path,
            progress,
            None,
        )?;
        merge_forge_profile(&mut vanilla, forge_profile, &libraries_dir)?;
    } else if loader_kind == "neoforge" {
        progress.log(&format!("# Fetching NeoForge {loader_version}..."));
        let (neoforge_profile, installer_path) =
            crate::forge_install::fetch_neoforge_profile(loader_version, progress)?;
        crate::forge_install::download_forge_libraries(
            &neoforge_profile,
            &libraries_dir,
            progress,
        )?;
        crate::forge_install::run_forge_processors(
            &neoforge_profile,
            &libraries_dir,
            launcher_dir,
            &vanilla.client_jar,
            mc_version,
            java_path,
            progress,
            Some(&installer_path),
        )?;
        merge_forge_profile(&mut vanilla, neoforge_profile, &libraries_dir)?;
    } else if loader_kind != "vanilla" {
        return Err(InstallError::UnsupportedLoader(loader_kind.to_string()));
    }

    verify_install_integrity(&vanilla)?;
    progress.log("# Installation complete (integrity verified).");
    store_install(&cache_key, vanilla.clone());
    store_disk_install(launcher_dir, &cache_key, &vanilla);
    Ok(vanilla)
}

fn install_vanilla(
    mc_version: &str,
    versions_dir: &Path,
    libraries_dir: &Path,
    assets_dir: &Path,
    natives_base: &Path,
    progress: &InstallProgress,
) -> Result<InstalledVersion, InstallError> {
    let version_dir = versions_dir.join(mc_version);
    let version_json_path = version_dir.join(format!("{mc_version}.json"));

    let version_json: VersionJson = if version_json_path.exists() {
        progress.log("# Loading cached version JSON...");
        serde_json::from_str(&fs::read_to_string(&version_json_path)?)?
    } else {
        progress.log("# Fetching version manifest...");
        let manifest: VersionManifest = crate::http::get_json(MOJANG_VERSION_MANIFEST)?;
        let version_url = manifest
            .versions
            .into_iter()
            .find(|v| v.id == mc_version)
            .map(|v| v.url)
            .ok_or_else(|| {
                InstallError::MissingDownload(format!(
                    "Minecraft {mc_version} not found in manifest"
                ))
            })?;
        progress.log("# Fetching version JSON...");
        let raw = crate::http::get_text(&version_url)?;
        fs::create_dir_all(&version_dir)?;
        fs::write(&version_json_path, &raw)?;
        serde_json::from_str(&raw)?
    };

    let client_jar = version_dir.join(format!("{}.jar", version_json.id));
    if !client_jar.is_file() || fs::metadata(&client_jar).map(|m| m.len() == 0).unwrap_or(true) {
        progress.log("# Downloading client jar...");
        download_with_sha1(
            &version_json.downloads.client.url,
            &client_jar,
            Some(&version_json.downloads.client.sha1),
        )?;
    }

    let natives_dir = natives_base.join(&version_json.id);
    fs::create_dir_all(&natives_dir)?;

    let mut libraries: Vec<PathBuf> = Vec::new();
    let mut lib_tasks: Vec<(String, PathBuf, Option<String>)> = Vec::new();
    let mut native_tasks: Vec<(String, PathBuf, Option<String>)> = Vec::new();
    for lib in &version_json.libraries {
        if !library_allowed(lib) {
            continue;
        }
        let artifact = &lib.downloads.artifact;
        let path = maven_path(libraries_dir, &artifact.path);
        if !path.is_file() || fs::metadata(&path).map(|m| m.len() == 0).unwrap_or(true) {
            lib_tasks.push((artifact.url.clone(), path.clone(), artifact.sha1.clone()));
        }
        libraries.push(path);

        if let Some(classifiers) = lib.downloads.classifiers.as_ref() {
            if let Some(native) = classifiers.get(native_classifier()) {
                let native_path = maven_path(libraries_dir, &native.path);
                if !native_path.is_file()
                    || fs::metadata(&native_path).map(|m| m.len() == 0).unwrap_or(true)
                {
                    native_tasks.push((
                        native.url.clone(),
                        native_path.clone(),
                        native.sha1.clone(),
                    ));
                }
            }
        }
    }

    if lib_tasks.is_empty() && native_tasks.is_empty() {
        progress.log("# All libraries already present.");
    } else {
        let total = lib_tasks.len() + native_tasks.len();
        progress.log(&format!("# Downloading {total} libraries in parallel..."));
        let counter = AtomicUsize::new(0);
        let errs: Vec<String> = lib_tasks
            .into_par_iter()
            .chain(native_tasks.into_par_iter())
            .filter_map(|(url, path, sha1)| {
                if let Some(parent) = path.parent() {
                    if let Err(e) = fs::create_dir_all(parent) {
                        return Some(format!("{url}: {e}"));
                    }
                }
                if let Err(e) = download_with_sha1(&url, &path, sha1.as_deref()) {
                    return Some(format!("{url}: {e}"));
                }
                let n = counter.fetch_add(1, Ordering::Relaxed) + 1;
                progress.log(&format!("# Downloaded library {n}/{total}."));
                None
            })
            .collect();
        if !errs.is_empty() {
            return Err(InstallError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                errs.join("\n"),
            )));
        }
        progress.log("# Library download complete.");
    }

    for lib in &version_json.libraries {
        if !library_allowed(lib) {
            continue;
        }
        if let Some(classifiers) = lib.downloads.classifiers.as_ref() {
            if let Some(native) = classifiers.get(native_classifier()) {
                let native_path = maven_path(libraries_dir, &native.path);
                extract_natives(&native_path, &natives_dir)?;
            }
        }
    }

    libraries.push(client_jar.clone());

    let asset_index_path = assets_dir
        .join("indexes")
        .join(format!("{}.json", version_json.asset_index.id));
    if !asset_index_path.is_file() {
        progress.log("# Downloading asset index...");
        if let Some(parent) = asset_index_path.parent() {
            fs::create_dir_all(parent)?;
        }
        download_with_sha1(
            &version_json.asset_index.url,
            &asset_index_path,
            Some(&version_json.asset_index.sha1),
        )?;
    }
    // A truncated index used to survive because the cache fast path only
    // checked file existence. Re-fetch it once before declaring preflight
    // failure so a repair is automatic when the network is available.
    let asset_index: AssetIndex = match fs::read_to_string(&asset_index_path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
    {
        Some(index) => index,
        None => {
            progress.log("# Asset index is unreadable; downloading a fresh copy...");
            if asset_index_path.is_file() {
                fs::remove_file(&asset_index_path)?;
            }
            download_with_sha1(
                &version_json.asset_index.url,
                &asset_index_path,
                Some(&version_json.asset_index.sha1),
            )?;
            serde_json::from_str(&fs::read_to_string(&asset_index_path)?)?
        }
    };
    install_assets_index(&asset_index, assets_dir, progress)?;

    let log_config = version_json
        .logging
        .as_ref()
        .and_then(|l| l.client.as_ref())
        .map(|c| {
            let path = version_dir.join(&c.file.id);
            if !path.exists() {
                let _ = download_with_sha1(&c.file.url, &path, Some(&c.file.sha1));
            }
            path
        });

    let (jvm_args, game_args) =
        parse_arguments(&version_json.arguments, &version_json.legacy_arguments);

    Ok(InstalledVersion {
        id: version_json.id.clone(),
        main_class: version_json.main_class.clone(),
        client_jar,
        libraries,
        natives_dir,
        asset_index_id: version_json.asset_index.id.clone(),
        asset_dir: assets_dir.to_path_buf(),
        log_config,
        jvm_args,
        game_args,
    })
}

fn fetch_fabric_profile(
    mc_version: &str,
    loader_version: &str,
    loader_kind: &str,
    launcher_dir: &Path,
    progress: &InstallProgress,
) -> Result<FabricProfileJson, InstallError> {
    let base = if loader_kind == "quilt" {
        "https://meta.quiltmc.org/v3/versions/loader"
    } else {
        "https://meta.fabricmc.net/v2/versions/loader"
    };
    let url = format!("{base}/{mc_version}/{loader_version}/profile/json");

    let cache_dir = launcher_dir.join("fabric-cache");
    let cache_path = cache_dir.join(format!("{loader_kind}-{mc_version}-{loader_version}.json"));

    progress.log(&format!("# {loader_kind} profile URL: {url}"));

    match crate::http::get_text(&url) {
        Ok(text) => {
            let _ = fs::create_dir_all(&cache_dir);
            let _ = fs::write(&cache_path, &text);
            progress.log("# Profile downloaded, parsing...");
            Ok(serde_json::from_str(&text)?)
        }
        Err(e) => {
            if cache_path.exists() {
                progress.log("# Profile fetch failed, using cached version...");
                let text = fs::read_to_string(&cache_path)?;
                Ok(serde_json::from_str(&text)?)
            } else {
                Err(InstallError::Network(e))
            }
        }
    }
}

fn merge_forge_profile(
    vanilla: &mut InstalledVersion,
    forge: crate::forge_install::ForgeProfile,
    libraries_dir: &Path,
) -> Result<(), InstallError> {
    if !forge.main_class.is_empty() {
        vanilla.main_class = forge.main_class;
    }
    for lib in &forge.libraries {
        if !lib.include_in_classpath {
            continue;
        }
        if let Some(artifact) = crate::forge_install::resolve_library(lib) {
            vanilla.libraries.push(libraries_dir.join(&artifact.path));
        }
    }
    if let Some(args) = forge.arguments {
        let (jvm, game) = crate::forge_install::flatten_arguments(&args);
        if !jvm.is_empty() {
            vanilla.jvm_args = jvm;
        }
        if !game.is_empty() {
            vanilla.game_args = game;
        }
    }
    Ok(())
}

fn merge_fabric_profile(
    vanilla: &mut InstalledVersion,
    fabric: FabricProfileJson,
    libraries_dir: &Path,
    mc_version: &str,
    progress: &InstallProgress,
) -> Result<(), InstallError> {
    progress.log(&format!("# Fabric main class: {}", fabric.main_class));
    if !fabric.main_class.is_empty() {
        vanilla.main_class = fabric.main_class;
    }

    let mut lib_tasks: Vec<DownloadTask> = Vec::new();
    for lib in &fabric.libraries {
        // Replace ${modrinth.gameVersion} placeholder with actual MC version.
        let name = lib.name.replace("${modrinth.gameVersion}", mc_version);
        let Some((group_path, artifact, version, classifier)) = parse_maven_name(&name) else {
            progress.log(&format!(
                "# Skipping unsupported loader library coordinate: {name}"
            ));
            continue;
        };
        let jar = match classifier {
            Some(classifier) => format!("{artifact}-{version}-{classifier}.jar"),
            None => format!("{artifact}-{version}.jar"),
        };
        let path = libraries_dir
            .join(&group_path)
            .join(&artifact)
            .join(&version)
            .join(&jar);
        if !path.exists() || lib.sha1.is_some() {
            let urls = fabric_library_urls(&lib.url, &group_path, &artifact, &version, &jar);
            lib_tasks.push(DownloadTask {
                urls,
                path: path.clone(),
                sha1: lib.sha1.clone(),
            });
        }
        vanilla.libraries.push(path);
    }

    if lib_tasks.is_empty() {
        progress.log("# All loader libraries already present.");
    } else {
        download_tasks_with_sequential_retry("loader library", lib_tasks, progress, 5)?;
        progress.log("# Loader library download complete.");
    }

    let (jvm_args, game_args) = parse_arguments(&fabric.arguments, &None);
    if !jvm_args.is_empty() {
        vanilla.jvm_args = jvm_args;
    }
    if !game_args.is_empty() {
        vanilla.game_args = game_args;
    }

    Ok(())
}

fn asset_file_is_usable(path: &Path, expected_size: u64) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    metadata.is_file() && (expected_size == 0 || metadata.len() == expected_size)
}

fn install_assets_index(
    index: &AssetIndex,
    assets_dir: &Path,
    progress: &InstallProgress,
) -> Result<(), InstallError> {
    let objects_dir = assets_dir.join("objects");
    let mut to_download: Vec<(String, String)> = Vec::new();
    for (_name, obj) in &index.objects {
        let prefix = &obj.hash[..2];
        let object_path = objects_dir.join(prefix).join(&obj.hash);
        if !asset_file_is_usable(&object_path, obj.size) {
            to_download.push((prefix.to_string(), obj.hash.clone()));
        }
    }
    if to_download.is_empty() {
        progress.log("# All assets already present.");
        return Ok(());
    }
    let total = to_download.len();
    progress.log(&format!("# Downloading {total} assets in parallel..."));
    let counter = AtomicUsize::new(0);
    let errs: Vec<String> = to_download
        .into_par_iter()
        .filter_map(|(prefix, hash)| {
            let object_dir = objects_dir.join(&prefix);
            let object_path = object_dir.join(&hash);
            let url = format!("{}/{}/{}", RESOURCES_URL, prefix, hash);
            if let Err(e) = fs::create_dir_all(&object_dir) {
                return Some(format!("{url}: {e}"));
            }
            if let Err(e) = download_with_sha1(&url, &object_path, Some(&hash)) {
                return Some(format!("{url}: {e}"));
            }
            let n = counter.fetch_add(1, Ordering::Relaxed) + 1;
            if n % 50 == 0 || n == total {
                progress.log(&format!("# Assets: {n}/{total}..."));
            }
            None
        })
        .collect();
    if !errs.is_empty() {
        return Err(InstallError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            errs.join("\n"),
        )));
    }
    progress.log("# Asset download complete.");
    Ok(())
}

#[derive(Debug, Clone)]
struct DownloadTask {
    urls: Vec<String>,
    path: PathBuf,
    sha1: Option<String>,
}

fn download_tasks_with_sequential_retry(
    label: &str,
    tasks: Vec<DownloadTask>,
    progress: &InstallProgress,
    log_every: usize,
) -> Result<(), InstallError> {
    let total = tasks.len();
    progress.log(&format!("# Downloading {total} {label}s in parallel..."));
    let counter = AtomicUsize::new(0);

    let failed: Vec<(DownloadTask, String)> = {
        let jobs: Vec<(String, PathBuf, Option<String>)> = tasks
            .iter()
            .map(|t| {
                (
                    t.urls.first().cloned().unwrap_or_default(),
                    t.path.clone(),
                    t.sha1.clone(),
                )
            })
            .collect();
        // Prefer bounded batch when every task has a primary URL; fall back per-task on errors.
        let _ = crate::download_engine::download_batch_limited(
            &jobs,
            crate::download_engine::configured_concurrency(),
        );
        tasks
            .into_par_iter()
            .filter_map(|task| {
                if task.path.exists() {
                    let n = counter.fetch_add(1, Ordering::Relaxed) + 1;
                    if n % log_every == 0 || n == total {
                        progress.log(&format!("# {label}s: {n}/{total}..."));
                    }
                    return None;
                }
                if let Some(parent) = task.path.parent() {
                    if let Err(e) = fs::create_dir_all(parent) {
                        return Some((task, e.to_string()));
                    }
                }
                match download_task(&task) {
                    Ok(()) => {
                        let n = counter.fetch_add(1, Ordering::Relaxed) + 1;
                        if n % log_every == 0 || n == total {
                            progress.log(&format!("# {label}s: {n}/{total}..."));
                        }
                        None
                    }
                    Err(e) => Some((task, e.to_string())),
                }
            })
            .collect()
    };

    if failed.is_empty() {
        return Ok(());
    }

    progress.log(&format!(
        "# {} {label}(s) failed in parallel; retrying sequentially with fallback URLs...",
        failed.len()
    ));

    let mut errors = Vec::new();
    for (task, first_error) in failed {
        match download_task(&task) {
            Ok(()) => {
                let n = counter.fetch_add(1, Ordering::Relaxed) + 1;
                progress.log(&format!("# {label}s: {n}/{total}..."));
            }
            Err(e) => {
                let urls = task.urls.join(", ");
                errors.push(format!(
                    "{urls}: first error: {first_error}; retry error: {e}"
                ));
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(InstallError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            errors.join("\n"),
        )))
    }
}

fn download_task(task: &DownloadTask) -> Result<(), InstallError> {
    let mut last_error = None;
    for url in &task.urls {
        match download_with_sha1(url, &task.path, task.sha1.as_deref()) {
            Ok(()) => return Ok(()),
            Err(e) => last_error = Some(e),
        }
    }
    Err(last_error.unwrap_or_else(|| {
        InstallError::MissingDownload(format!("no URL for {}", task.path.display()))
    }))
}

fn fabric_library_urls(
    base_url: &str,
    group_path: &str,
    artifact: &str,
    version: &str,
    jar: &str,
) -> Vec<String> {
    let base = base_url.trim_end_matches('/');
    let primary = format!("{base}/{group_path}/{artifact}/{version}/{jar}");
    let mut urls = vec![primary.clone()];

    // Fabric's canonical host is maven.fabricmc.net, but some networks/proxies
    // intermittently fail parallel TLS connections to it. maven2.fabricmc.net is
    // an official alias/CDN endpoint for the same artifacts, so keep it as a
    // deterministic fallback before surfacing the error to the UI log.
    if base == "https://maven.fabricmc.net" || base == "http://maven.fabricmc.net" {
        urls.push(format!(
            "https://maven2.fabricmc.net/{group_path}/{artifact}/{version}/{jar}"
        ));
    }
    urls
}

fn parse_maven_name(name: &str) -> Option<(String, String, String, Option<String>)> {
    let parts: Vec<&str> = name.split(':').collect();
    if parts.len() < 3 {
        return None;
    }
    Some((
        parts[0].replace('.', "/"),
        parts[1].to_string(),
        parts[2].to_string(),
        parts.get(3).map(|s| s.to_string()),
    ))
}

/// Downloads a file to `path` and verifies its SHA1 if `expected_sha1` is
/// provided. Uses streaming I/O (no full-file memory buffer) and atomic
/// writes (downloads to `.part`, renames on success) so a failed or
/// interrupted download never leaves a half-written file at `path`.
pub fn download_with_sha1(
    url: &str,
    path: &Path,
    expected_sha1: Option<&str>,
) -> Result<(), InstallError> {
    if path.exists() {
        let usable_without_checksum = path
            .metadata()
            .map(|metadata| metadata.is_file() && metadata.len() > 0)
            .unwrap_or(false);
        if let Some(expected) = expected_sha1 {
            if usable_without_checksum {
                let existing = sha1_file(path)?;
                if existing.eq_ignore_ascii_case(expected) {
                    return Ok(());
                }
            }
        } else if usable_without_checksum {
            return Ok(());
        }

        // `download_streaming` writes atomically through a `.part` file. A
        // bad/empty destination must be removed first or a no-checksum
        // artifact would be mistaken for a completed download forever.
        // The destination may be a read-only dedup-store hardlink — clear
        // the attribute first (remove on a read-only file fails on Windows).
        if path.is_file() {
            crate::fs_util::clear_readonly(path);
            fs::remove_file(path)?;
        } else {
            return Err(InstallError::Integrity(format!(
                "download destination is not a file: {}",
                path.display()
            )));
        }
    }

    // Dedup store (docs/17, Part 1): Mojang-manifest downloads (client jar,
    // libraries, natives, assets) all carry a known sha1 — if the exact bytes
    // already live in the shared object store, hard-link them instead of
    // downloading again. Requires a known sha1; any store miss/failure falls
    // through to the network path (dedup is an optimization, never a gate).
    if let Some(sha1) = expected_sha1 {
        if crate::mod_store::try_hardlink(path, sha1) {
            return Ok(());
        }
    }

    crate::http::download_streaming(
        url,
        path,
        expected_sha1,
        None::<Box<dyn FnMut(u64, u64) + Send>>,
    )
    .map_err(|e| InstallError::MissingDownload(format!("{} (url: {})", e, url)))?;

    // Feed the store so other instances (same file, another version group)
    // get these bytes for free.
    if let Some(sha1) = expected_sha1 {
        crate::mod_store::record(path, sha1);
    }

    Ok(())
}

/// Like `download_with_sha1`, but invokes `progress` with
/// `(bytes_received, total_bytes)` as each chunk is received. The callback
/// fires on the downloading thread, so the caller is responsible for any
/// thread-safety (e.g. `Arc<AtomicU64>` or a channel).
pub fn download_with_progress(
    url: &str,
    path: &Path,
    expected_sha1: Option<&str>,
    id: &str,
    progress: &ProgressCallback,
) -> Result<(), InstallError> {
    if path.exists() {
        let usable_without_checksum = path
            .metadata()
            .map(|metadata| metadata.is_file() && metadata.len() > 0)
            .unwrap_or(false);
        if let Some(expected) = expected_sha1 {
            if usable_without_checksum {
                let existing = sha1_file(path)?;
                if existing.eq_ignore_ascii_case(expected) {
                    return Ok(());
                }
            }
        } else if usable_without_checksum {
            return Ok(());
        }
        if path.is_file() {
            fs::remove_file(path)?;
        } else {
            return Err(InstallError::Integrity(format!(
                "download destination is not a file: {}",
                path.display()
            )));
        }
    }

    let id_owned = id.to_string();
    let cb_arc = progress.0.clone();
    let mut stream_cb: Option<Box<dyn FnMut(u64, u64) + Send>> = None;
    if cb_arc.is_some() {
        let id_for_cb = id_owned.clone();
        // Arc<Box<dyn Fn + Send + Sync>> is itself Send + Sync. We
        // move the Arc clone into the closure, then call the inner Fn
        // through the Arc reference.
        let c_arc_for_cb = cb_arc.clone();
        stream_cb = Some(Box::new(move |received: u64, total: u64| {
            if let Some(cb) = c_arc_for_cb.as_ref() {
                cb(&id_for_cb, received, total);
            }
        }));
    }

    crate::http::download_streaming(url, path, expected_sha1, stream_cb)
        .map_err(|e| InstallError::MissingDownload(format!("{} (url: {})", e, url)))?;
    Ok(())
}

pub fn sha1_file(path: &Path) -> Result<String, InstallError> {
    let bytes = fs::read(path)?;
    Ok(format!("{:x}", sha1::Sha1::digest(&bytes)))
}

fn maven_path(libraries_dir: &Path, maven_path: &str) -> PathBuf {
    libraries_dir.join(maven_path.replace('/', std::path::MAIN_SEPARATOR_STR))
}

fn library_allowed(lib: &Library) -> bool {
    if lib.rules.is_empty() {
        return true;
    }
    let mut allowed = false;
    for rule in &lib.rules {
        let applies = rule_applies(rule);
        if applies {
            allowed = rule.action == "allow";
        }
    }
    allowed
}

fn rule_applies(rule: &Rule) -> bool {
    match (&rule.os, &rule.features) {
        (Some(_), Some(_)) => false,
        (Some(os), None) => os.name.as_deref() == Some(current_os()),
        (None, Some(_)) => false,
        (None, None) => true,
    }
}

fn current_os() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else {
        "linux"
    }
}

fn native_classifier() -> &'static str {
    if cfg!(target_os = "windows") {
        "natives-windows"
    } else if cfg!(target_os = "macos") {
        "natives-osx"
    } else {
        "natives-linux"
    }
}

fn extract_natives(archive_path: &Path, natives_dir: &Path) -> Result<(), InstallError> {
    fs::create_dir_all(natives_dir)?;

    // A directory-level "has any file" shortcut is not enough: Minecraft
    // commonly has several native archives, so it could leave only the first
    // one extracted. Keep one marker per archive instead. Existing installs
    // without markers are safely re-extracted once and then become fast.
    let archive_name = archive_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("native")
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect::<String>();
    let marker = natives_dir.join(format!(".tuffbox-native-{archive_name}.ok"));
    if marker.is_file() {
        return Ok(());
    }

    let file = fs::File::open(archive_path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        if entry.is_file() {
            let name = entry.name().replace('\\', "/");
            if name.contains("..") || name.starts_with('/') {
                return Err(InstallError::PathTraversal(name.to_string()));
            }
            let out_path = natives_dir.join(&name);
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut out = fs::File::create(out_path)?;
            std::io::copy(&mut entry, &mut out)?;
        }
    }
    fs::write(marker, b"ok\n")?;
    Ok(())
}

fn parse_arguments(
    args: &Option<Arguments>,
    legacy: &Option<String>,
) -> (Vec<String>, Vec<String>) {
    if let Some(legacy) = legacy {
        let game_args = legacy.split_whitespace().map(String::from).collect();
        return (Vec::new(), game_args);
    }

    let empty = Arguments::default();
    let args = args.as_ref().unwrap_or(&empty);
    let jvm = args
        .jvm
        .iter()
        .flat_map(|a| match a {
            Argument::String(s) => vec![s.clone()],
            Argument::Object { rules, value } => {
                if rule_matches(rules) {
                    match value {
                        ArgValue::Single(s) => vec![s.clone()],
                        ArgValue::Many(v) => v.clone(),
                    }
                } else {
                    Vec::new()
                }
            }
        })
        .collect();
    let game = args
        .game
        .iter()
        .flat_map(|a| match a {
            Argument::String(s) => vec![s.clone()],
            Argument::Object { rules, value } => {
                if rule_matches(rules) {
                    match value {
                        ArgValue::Single(s) => vec![s.clone()],
                        ArgValue::Many(v) => v.clone(),
                    }
                } else {
                    Vec::new()
                }
            }
        })
        .collect();
    (jvm, game)
}

fn rule_matches(rules: &[Rule]) -> bool {
    rules.iter().all(|r| rule_applies(r))
}

#[derive(Debug, Deserialize)]
struct VersionManifest {
    versions: Vec<ManifestEntry>,
}

#[derive(Debug, Deserialize)]
struct ManifestEntry {
    id: String,
    url: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VersionJson {
    id: String,
    main_class: String,
    downloads: Downloads,
    libraries: Vec<Library>,
    asset_index: AssetIndexEntry,
    arguments: Option<Arguments>,
    #[serde(rename = "minecraftArguments")]
    legacy_arguments: Option<String>,
    logging: Option<LoggingConfig>,
}

#[derive(Debug, Deserialize)]
struct Downloads {
    client: Download,
}

#[derive(Debug, Deserialize)]
struct Download {
    url: String,
    sha1: String,
}

#[derive(Debug, Deserialize)]
struct Library {
    #[serde(default)]
    #[allow(dead_code)]
    name: String,
    downloads: LibraryDownloads,
    #[serde(default)]
    rules: Vec<Rule>,
}

#[derive(Debug, Deserialize)]
struct LibraryDownloads {
    artifact: Artifact,
    #[serde(default)]
    classifiers: Option<std::collections::HashMap<String, Artifact>>,
}

#[derive(Debug, Deserialize)]
struct Artifact {
    path: String,
    url: String,
    sha1: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AssetIndexEntry {
    id: String,
    url: String,
    sha1: String,
}

#[derive(Debug, Deserialize)]
struct AssetIndex {
    objects: std::collections::HashMap<String, AssetObject>,
}

#[derive(Debug, Deserialize)]
struct AssetObject {
    hash: String,
    #[serde(default)]
    #[allow(dead_code)]
    size: u64,
}

#[derive(Debug, Deserialize, Default)]
struct Arguments {
    #[serde(default)]
    jvm: Vec<Argument>,
    #[serde(default)]
    game: Vec<Argument>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Argument {
    String(String),
    Object { rules: Vec<Rule>, value: ArgValue },
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ArgValue {
    Single(String),
    Many(Vec<String>),
}

#[derive(Debug, Deserialize)]
struct Rule {
    action: String,
    os: Option<RuleOs>,
    features: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct RuleOs {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LoggingConfig {
    client: Option<LoggingEntry>,
}

#[derive(Debug, Deserialize)]
struct LoggingEntry {
    #[serde(default)]
    #[allow(dead_code)]
    argument: String,
    file: LoggingFile,
    #[serde(rename = "type")]
    #[allow(dead_code)]
    kind: String,
}

#[derive(Debug, Deserialize)]
struct LoggingFile {
    id: String,
    url: String,
    sha1: String,
    #[serde(default)]
    #[allow(dead_code)]
    size: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FabricProfileJson {
    #[serde(default)]
    libraries: Vec<FabricLibrary>,
    #[serde(default)]
    main_class: String,
    arguments: Option<Arguments>,
}

#[derive(Debug, Deserialize)]
struct FabricLibrary {
    name: String,
    #[serde(default = "default_fabric_maven_url")]
    url: String,
    #[serde(default)]
    sha1: Option<String>,
}

fn default_fabric_maven_url() -> String {
    "https://maven.fabricmc.net/".to_string()
}

#[cfg(test)]
mod integrity_tests {
    use super::*;

    fn minimal_install(root: &std::path::Path) -> InstalledVersion {
        let versions = root.join("versions");
        let libraries = root.join("libraries");
        let natives = root.join("natives");
        let assets = root.join("assets");
        fs::create_dir_all(&versions).unwrap();
        fs::create_dir_all(&libraries).unwrap();
        fs::create_dir_all(&natives).unwrap();
        fs::create_dir_all(assets.join("indexes")).unwrap();
        fs::create_dir_all(assets.join("objects").join("ab")).unwrap();

        let client = versions.join("client.jar");
        let library = libraries.join("library.jar");
        fs::write(&client, b"client").unwrap();
        fs::write(&library, b"library").unwrap();
        // The verifier accepts platform native extensions so this fixture is
        // deterministic on every CI host.
        fs::write(natives.join("fixture.so"), b"native").unwrap();
        fs::write(
            assets.join("indexes").join("fixture.json"),
            r#"{"objects":{"example":{"hash":"abcdef","size":3}}}"#,
        )
        .unwrap();
        fs::write(assets.join("objects").join("ab").join("abcdef"), b"abc").unwrap();

        InstalledVersion {
            id: "fixture".to_string(),
            main_class: "example.Main".to_string(),
            client_jar: client,
            libraries: vec![library],
            natives_dir: natives,
            asset_index_id: "fixture".to_string(),
            asset_dir: assets,
            log_config: None,
            jvm_args: Vec::new(),
            game_args: Vec::new(),
        }
    }

    #[test]
    fn integrity_preflight_rejects_missing_asset_before_launch() {
        let temp = tempfile::tempdir().unwrap();
        let install = minimal_install(temp.path());
        assert!(verify_install_integrity(&install).is_ok());

        fs::remove_file(
            install
                .asset_dir
                .join("objects")
                .join("ab")
                .join("abcdef"),
        )
        .unwrap();
        assert!(matches!(
            verify_install_integrity(&install),
            Err(InstallError::Integrity(message)) if message.contains("asset 'example' is missing")
        ));
    }
}
