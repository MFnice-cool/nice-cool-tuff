//! Forge/NeoForge loader installation.
//!
//! Uses Modrinth launcher-meta for Forge profiles and falls back to Neoforged
//! maven metadata for NeoForge.

use crate::mc_install::{download_with_sha1, InstallError, InstallProgress};
use rayon::prelude::*;
use serde::Deserialize;
use std::{
    collections::HashMap,
    fs,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

const FORGE_MANIFEST_URL: &str = "https://launcher-meta.modrinth.com/forge/v0/manifest.json";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgeProfile {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub inherits_from: String,
    #[serde(default)]
    pub main_class: String,
    #[serde(default)]
    pub libraries: Vec<ForgeLibrary>,
    #[serde(default)]
    pub data: HashMap<String, SidedDataEntry>,
    #[serde(default)]
    pub processors: Vec<Processor>,
    pub arguments: Option<ForgeArguments>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ForgeArguments {
    #[serde(default)]
    pub jvm: Vec<ForgeArgument>,
    #[serde(default)]
    pub game: Vec<ForgeArgument>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ForgeArgument {
    Plain(String),
    Ruled {
        rules: Vec<serde_json::Value>,
        value: serde_json::Value,
    },
}

#[derive(Debug, Deserialize)]
pub struct SidedDataEntry {
    pub client: String,
    #[serde(default)]
    pub server: String,
}

#[derive(Debug, Deserialize)]
pub struct Processor {
    pub jar: String,
    #[serde(default)]
    pub classpath: Vec<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub outputs: HashMap<String, String>,
    #[serde(default)]
    pub sides: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct ForgeLibrary {
    pub name: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub downloads: Option<ForgeDownloads>,
    #[serde(default = "default_true")]
    pub include_in_classpath: bool,
    #[serde(default = "default_true")]
    pub downloadable: bool,
}

#[derive(Debug, Deserialize)]
pub struct ForgeDownloads {
    pub artifact: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct ForgeArtifact {
    pub path: String,
    pub url: String,
    pub sha1: Option<String>,
    #[allow(dead_code)]
    pub size: u64,
}

fn default_true() -> bool {
    true
}

/// Parses a Forge/NeoForge library `downloads.artifact` entry.
///
/// The Modrinth launcher-meta API actually returns this as a plain JSON
/// object (`{"path": ..., "url": ..., "sha1": ..., "size": ...}`) for every
/// Forge version tested (e.g. 1.20.1-47.2.20). The previous implementation
/// only handled a serialized `"@{path=...;url=...}"` *string* format,
/// which doesn't match any real response from this endpoint — so
/// `resolve_library` silently returned `None` for every library with an
/// object-shaped artifact (including `mcp_config`, a hard dependency of
/// the Forge install processors), those files were never downloaded, and
/// the install then crashed with a raw "No such file or directory" once a
/// processor tried to read them. This now handles both shapes.
fn parse_artifact_value(v: &serde_json::Value) -> Option<ForgeArtifact> {
    if let Some(obj) = v.as_object() {
        return Some(ForgeArtifact {
            path: obj.get("path")?.as_str()?.to_string(),
            url: obj.get("url")?.as_str()?.to_string(),
            sha1: obj
                .get("sha1")
                .and_then(|s| s.as_str())
                .map(|s| s.to_string()),
            size: obj.get("size").and_then(|s| s.as_u64()).unwrap_or(0),
        });
    }

    let s = v.as_str()?;
    if !s.starts_with("@{") || !s.ends_with('}') {
        return None;
    }
    let inner = &s[2..s.len() - 1];
    let mut map: HashMap<String, String> = HashMap::new();
    for part in inner.split(';') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let mut kv = part.splitn(2, '=');
        let k = kv.next()?.trim().to_string();
        let v = kv.next()?.trim().to_string();
        map.insert(k, v);
    }
    Some(ForgeArtifact {
        path: map.get("path")?.clone(),
        url: map.get("url")?.clone(),
        sha1: map.get("sha1").cloned(),
        size: map.get("size").and_then(|s| s.parse().ok()).unwrap_or(0),
    })
}

/// Converts maven coordinates to a local path under `libraries_dir`.
fn maven_path(libraries_dir: &Path, name: &str) -> Result<PathBuf, InstallError> {
    let parts: Vec<&str> = name.split(':').collect();
    if parts.len() < 3 {
        return Err(InstallError::MissingDownload(format!(
            "Invalid maven coordinate: {name}"
        )));
    }
    let group_path = parts[0].replace('.', "/");
    let artifact = parts[1];
    let version_ext = parts[2];
    let version_ext_parts: Vec<&str> = version_ext.split('@').collect();
    let version = version_ext_parts[0];
    let ext = version_ext_parts.get(1).copied().unwrap_or("jar");

    let filename = if parts.len() >= 4 {
        let classifier = parts[3];
        format!("{}-{}-{}.{}", artifact, version, classifier, ext)
    } else {
        format!("{}-{}.{}", artifact, version, ext)
    };

    Ok(libraries_dir
        .join(group_path)
        .join(artifact)
        .join(version)
        .join(filename))
}

/// Build the download URL for a maven-style library hosted at `base`.
fn maven_url(base: &str, name: &str) -> Result<String, InstallError> {
    let path = maven_path(Path::new(""), name)?;
    let base = base.trim_end_matches('/');
    Ok(format!(
        "{}/{}",
        base,
        path.to_string_lossy().replace('\\', "/")
    ))
}

pub fn fetch_forge_profile_url(
    mc_version: &str,
    loader_version: &str,
) -> Result<String, InstallError> {
    let manifest: serde_json::Value = crate::http::get_json(FORGE_MANIFEST_URL)?;
    let game_versions = manifest
        .get("gameVersions")
        .and_then(|v| v.as_array())
        .ok_or_else(|| InstallError::MissingDownload("Forge manifest malformed".to_string()))?;
    for gv in game_versions {
        if gv.get("id").and_then(|v| v.as_str()) == Some(mc_version) {
            let loaders = gv
                .get("loaders")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            for loader in loaders {
                if loader.get("id").and_then(|v| v.as_str()) == Some(loader_version) {
                    return loader
                        .get("url")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .ok_or_else(|| {
                            InstallError::MissingDownload("Forge loader URL missing".to_string())
                        });
                }
            }
        }
    }
    Err(InstallError::MissingDownload(format!(
        "Forge {loader_version} for Minecraft {mc_version} not found"
    )))
}

pub fn fetch_forge_profile(
    mc_version: &str,
    loader_version: &str,
) -> Result<ForgeProfile, InstallError> {
    let url = fetch_forge_profile_url(mc_version, loader_version)?;
    Ok(crate::http::get_json(&url)?)
}

pub fn fetch_neoforge_profile(
    loader_version: &str,
    progress: &InstallProgress,
) -> Result<(ForgeProfile, PathBuf), InstallError> {
    let url = format!(
        "https://maven.neoforged.net/releases/net/neoforged/neoforge/{0}/neoforge-{0}-installer.jar",
        loader_version
    );
    let installer_path =
        std::env::temp_dir().join(format!("neoforge-{loader_version}-installer.jar"));
    progress.log(&format!(
        "# Downloading NeoForge installer {loader_version}..."
    ));
    download_with_sha1(&url, &installer_path, None)?;

    let file = fs::File::open(&installer_path)?;
    let mut archive = zip::ZipArchive::new(file)?;

    let install_profile: serde_json::Value = read_zip_json(&mut archive, "install_profile.json")?;
    let version_json: serde_json::Value = read_zip_json(&mut archive, "version.json")?;

    let mut profile: ForgeProfile = serde_json::from_value(install_profile)?;

    if let Some(main) = version_json.get("mainClass").and_then(|v| v.as_str()) {
        profile.main_class = main.to_string();
    }

    if let Some(args) = version_json.get("arguments") {
        profile.arguments = serde_json::from_value(args.clone()).ok();
    }

    if let Some(libraries) = version_json.get("libraries").and_then(|v| v.as_array()) {
        for lib in libraries {
            if let Ok(forge_lib) = serde_json::from_value::<ForgeLibrary>(lib.clone()) {
                profile.libraries.push(forge_lib);
            }
        }
    }

    Ok((profile, installer_path))
}

fn read_zip_json(
    archive: &mut zip::ZipArchive<fs::File>,
    name: &str,
) -> Result<serde_json::Value, InstallError> {
    let mut file = archive.by_name(name)?;
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    Ok(serde_json::from_str(&text)?)
}

/// Resolves the download information for a Forge library.
pub(crate) fn resolve_library(lib: &ForgeLibrary) -> Option<ForgeArtifact> {
    if !lib.downloadable {
        return None;
    }
    if let Some(downloads) = &lib.downloads {
        if let Some(artifact) = &downloads.artifact {
            if let Some(parsed) = parse_artifact_value(artifact) {
                return Some(parsed);
            }
        }
    }
    if let Some(base) = &lib.url {
        let url = maven_url(base, &lib.name).ok()?;
        let path = maven_path(Path::new(""), &lib.name).ok()?;
        return Some(ForgeArtifact {
            path: path.to_string_lossy().replace('\\', "/"),
            url,
            sha1: None,
            size: 0,
        });
    }
    None
}

pub fn download_forge_libraries(
    profile: &ForgeProfile,
    libraries_dir: &Path,
    progress: &InstallProgress,
) -> Result<Vec<PathBuf>, InstallError> {
    let mut classpath_libs: Vec<PathBuf> = Vec::new();
    let mut tasks: Vec<(ForgeArtifact, PathBuf)> = Vec::new();

    for lib in &profile.libraries {
        let Some(artifact) = resolve_library(lib) else {
            continue;
        };
        let path = libraries_dir.join(&artifact.path);
        if lib.include_in_classpath {
            classpath_libs.push(path.clone());
        }
        if !path.exists() {
            tasks.push((artifact, path));
        }
    }

    if tasks.is_empty() {
        progress.log("# All Forge libraries already present.");
    } else {
        let total = tasks.len();
        progress.log(&format!(
            "# Downloading {total} Forge libraries in parallel..."
        ));
        let counter = AtomicUsize::new(0);
        let errs: Vec<String> = tasks
            .into_par_iter()
            .filter_map(|(artifact, path)| {
                if let Some(parent) = path.parent() {
                    if let Err(e) = fs::create_dir_all(parent) {
                        return Some(format!("{}: {e}", artifact.url));
                    }
                }
                if let Err(e) = download_with_sha1(&artifact.url, &path, artifact.sha1.as_deref()) {
                    return Some(format!("{}: {e}", artifact.url));
                }
                let n = counter.fetch_add(1, Ordering::Relaxed) + 1;
                if n % 10 == 0 || n == total {
                    progress.log(&format!("# Forge libs: {n}/{total}..."));
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
        progress.log("# Forge library download complete.");
    }

    Ok(classpath_libs)
}

fn processor_main_class(jar: &Path) -> Result<Option<String>, InstallError> {
    let file = fs::File::open(jar)?;
    let mut archive = zip::ZipArchive::new(file)?;
    let manifest = archive.by_name("META-INF/MANIFEST.MF")?;
    let reader = BufReader::new(manifest);
    for line in reader.lines() {
        let mut line = line?;
        line.retain(|c| !c.is_whitespace());
        if let Some(class) = line.strip_prefix("Main-Class:") {
            return Ok(Some(class.to_string()));
        }
    }
    Ok(None)
}

fn processor_classpath(
    libraries_dir: &Path,
    processor: &Processor,
) -> Result<String, InstallError> {
    let mut paths: Vec<PathBuf> = Vec::new();
    for name in processor
        .classpath
        .iter()
        .chain(std::iter::once(&processor.jar))
    {
        paths.push(maven_path(libraries_dir, name)?);
    }
    // `;` is only the classpath separator on Windows; using it
    // unconditionally meant every processor invocation on Linux/macOS
    // built a single-entry classpath string that Java parsed as one giant
    // (nonexistent) path, so it could never find any class from the
    // dependency jars — including the processor's own Main-Class — and
    // failed with `ClassNotFoundException` on every real install.
    std::env::join_paths(&paths)
        .map(|joined| joined.to_string_lossy().to_string())
        .map_err(|e| InstallError::Io(std::io::Error::other(e.to_string())))
}

fn resolve_data_value(
    libraries_dir: &Path,
    data: &HashMap<String, SidedDataEntry>,
    key: &str,
) -> Result<String, InstallError> {
    let entry = data
        .get(key)
        .ok_or_else(|| InstallError::MissingDownload(format!("Missing processor data {key}")))?;
    let value = entry.client.trim_matches('\'');
    if let Some(inner) = value.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        let path = maven_path(libraries_dir, inner)?;
        return Ok(path.to_string_lossy().to_string());
    }
    Ok(value.to_string())
}

fn processor_arguments(
    libraries_dir: &Path,
    processor: &Processor,
    data: &HashMap<String, SidedDataEntry>,
    mc_jar: &Path,
    launcher_dir: &Path,
    mc_version: &str,
    installer_path: Option<&Path>,
) -> Result<Vec<String>, InstallError> {
    processor
        .args
        .iter()
        .map(|arg| {
            if let Some(inner) = arg.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                let path = maven_path(libraries_dir, inner)?;
                return Ok(path.to_string_lossy().to_string());
            }

            let mut result = arg.clone();
            for key in data.keys() {
                let value = resolve_data_value(libraries_dir, data, key)?;
                result = result.replace(&format!("{{{key}}}"), &value);
            }
            result = result.replace("{MINECRAFT_JAR}", &mc_jar.to_string_lossy());
            result = result.replace("{MINECRAFT_VERSION}", mc_version);
            result = result.replace("{ROOT}", &launcher_dir.to_string_lossy());
            result = result.replace("{LIBRARY_DIR}", &libraries_dir.to_string_lossy());
            result = result.replace("{SIDE}", "client");
            if let Some(installer) = installer_path {
                result = result.replace("{INSTALLER}", &installer.to_string_lossy());
            }
            Ok(result)
        })
        .collect()
}

fn outputs_up_to_date(
    libraries_dir: &Path,
    data: &HashMap<String, SidedDataEntry>,
    processor: &Processor,
) -> Result<bool, InstallError> {
    if processor.outputs.is_empty() {
        return Ok(false);
    }
    for (key, expected_sha1) in &processor.outputs {
        let raw = key.trim_start_matches('{').trim_end_matches('}');
        let path = if let Some(lib) = raw.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            maven_path(libraries_dir, lib)?
        } else {
            let resolved = resolve_data_value(libraries_dir, data, raw)?;
            PathBuf::from(resolved)
        };
        if !path.exists() {
            return Ok(false);
        }
        let expected = expected_sha1.trim_matches('\'');
        let actual = crate::mc_install::sha1_file(&path)?;
        if !actual.eq_ignore_ascii_case(expected) {
            return Ok(false);
        }
    }
    Ok(true)
}

pub fn run_forge_processors(
    profile: &ForgeProfile,
    libraries_dir: &Path,
    launcher_dir: &Path,
    mc_jar: &Path,
    mc_version: &str,
    java_path: &Path,
    progress: &InstallProgress,
    installer_path: Option<&Path>,
) -> Result<(), InstallError> {
    if profile.processors.is_empty() {
        return Ok(());
    }

    let data = &profile.data;
    let total = profile.processors.len();
    progress.log(&format!("# Running {total} Forge processors..."));

    for (index, processor) in profile.processors.iter().enumerate() {
        if !processor.sides.is_empty() && !processor.sides.contains(&"client".to_string()) {
            progress.log(&format!(
                "# Skipping processor {}/{} (not client)",
                index + 1,
                total
            ));
            continue;
        }
        if outputs_up_to_date(libraries_dir, data, processor)? {
            progress.log(&format!(
                "# Processor {}/{} already up to date.",
                index + 1,
                total
            ));
            continue;
        }

        let jar_path = maven_path(libraries_dir, &processor.jar)?;
        let cp = processor_classpath(libraries_dir, processor)?;
        let main_class = processor_main_class(&jar_path)?.ok_or_else(|| {
            InstallError::MissingDownload(format!("No Main-Class in {}", processor.jar))
        })?;
        let args = processor_arguments(
            libraries_dir,
            processor,
            data,
            mc_jar,
            launcher_dir,
            mc_version,
            installer_path,
        )?;

        progress.log(&format!(
            "# Running processor {}/{}: {main_class}",
            index + 1,
            total
        ));
        let mut c = Command::new(java_path);
        c.arg("-cp").arg(cp).arg(main_class).args(args);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        let output = c.output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(InstallError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Forge processor failed: {stderr}"),
            )));
        }
    }

    progress.log("# Forge processors complete.");
    Ok(())
}

/// Converts Forge arguments into flat strings (skips ruled args for simplicity).
pub fn flatten_arguments(args: &ForgeArguments) -> (Vec<String>, Vec<String>) {
    let mut jvm = Vec::new();
    let mut game = Vec::new();
    for arg in &args.jvm {
        if let ForgeArgument::Plain(s) = arg {
            jvm.push(s.clone());
        }
    }
    for arg in &args.game {
        if let ForgeArgument::Plain(s) = arg {
            game.push(s.clone());
        }
    }
    (jvm, game)
}
