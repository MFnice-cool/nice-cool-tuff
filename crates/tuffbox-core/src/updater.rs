//! Update checking for mods (safe vs risky updates) with `pin` support.
//!
//! Inspired by packwiz's per-mod updaters (`modrinth/updater.go`,
//! `curseforge/updater.go`): each [`crate::manifest::ModSpec`] carries a
//! source; this module queries the matching provider for the latest compatible
//! version and classifies the change as `Safe` (patch/minor within the same
//! release channel) or `Risky` (new major version or a different release
//! channel such as `alpha`/`beta`). Mods with a `pin` flag (stored on the
//! manifest via [`crate::manifest::ModSpec::pinned`]) are never updated.

use crate::manifest::{ModSpec, SourceKind};
use crate::provider::{
    ContentProvider, CurseForgeProvider, ModrinthProvider, ProviderFileHashes, ProviderFileInfo,
    ProviderSearchQuery, VersionInfo,
};
use semver::Version;
use std::cmp::Ordering;
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("provider error: {0}")]
    Provider(#[from] crate::provider::ProviderError),
    #[error("mod {0} has no project identifier to update from")]
    NoProjectId(String),
    #[error("failed to parse version {version}: {source}")]
    VersionParse {
        version: String,
        #[source]
        source: semver::Error,
    },
    #[error("network error: {0}")]
    Network(String),
}

/// Whether an available update is safe to apply automatically.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateRisk {
    /// Same major version, same release channel (e.g. 1.2.3 → 1.2.4).
    Safe,
    /// New minor/major, or a pre-release channel (beta/alpha). Needs a
    /// confirmation step before applying.
    Risky,
}

/// A pending update for a single mod.
#[derive(Debug, Clone)]
pub struct ModUpdate {
    pub mod_id: String,
    pub current_version: String,
    pub latest_version: String,
    pub latest_version_id: String,
    pub channel: Option<String>,
    pub risk: UpdateRisk,
    /// `true` when the user pinned the mod and it must not be updated.
    pub pinned: bool,
}

/// Returns the update classification for a given current → latest pair.
pub fn classify_update(
    current: &str,
    latest: &str,
    channel: Option<&str>,
) -> Result<UpdateRisk, UpdateError> {
    let cur = parse_lenient(current)?;
    let lat = parse_lenient(latest)?;
    match cur.major.cmp(&lat.major) {
        Ordering::Less => Ok(UpdateRisk::Risky),
        Ordering::Equal => {
            if matches!(channel, Some("alpha") | Some("beta")) {
                Ok(UpdateRisk::Risky)
            } else {
                Ok(UpdateRisk::Safe)
            }
        }
        Ordering::Greater => Ok(UpdateRisk::Safe),
    }
}

fn parse_lenient(v: &str) -> Result<Version, UpdateError> {
    match Version::parse(v) {
        Ok(ver) => Ok(ver),
        Err(_) => {
            let trimmed = v.trim_start_matches('v');
            Version::parse(trimmed).map_err(|source| UpdateError::VersionParse {
                version: v.to_string(),
                source,
            })
        }
    }
}

fn pinned_update(module: &ModSpec) -> ModUpdate {
    ModUpdate {
        mod_id: module.id.clone(),
        current_version: module.version.clone(),
        latest_version: module.version.clone(),
        latest_version_id: module.source.file_id.clone().unwrap_or_default(),
        channel: None,
        risk: UpdateRisk::Safe,
        pinned: true,
    }
}

/// Checks a single mod for an available update.
///
/// Supports Modrinth, CurseForge, and GitHub Releases (project_id = `owner/repo`).
/// Pinned mods return `Ok(Some(ModUpdate { pinned: true }))`.
pub fn check_mod_update(
    module: &ModSpec,
    minecraft_version: &str,
    loader: &str,
) -> Result<Option<ModUpdate>, UpdateError> {
    if module.pinned() {
        return Ok(Some(pinned_update(module)));
    }

    match module.source.kind {
        SourceKind::Modrinth => check_modrinth_update(module, minecraft_version, loader),
        SourceKind::Curseforge => check_curseforge_update(module, minecraft_version, loader),
        SourceKind::Github => check_github_update(module),
        SourceKind::Local | SourceKind::Direct => Ok(None),
    }
}

fn check_modrinth_update(
    module: &ModSpec,
    minecraft_version: &str,
    loader: &str,
) -> Result<Option<ModUpdate>, UpdateError> {
    let Some(project_id) = &module.source.project_id else {
        return Err(UpdateError::NoProjectId(module.id.clone()));
    };

    let provider = ModrinthProvider::new();
    let query = ProviderSearchQuery {
        minecraft_version: Some(minecraft_version.to_string()),
        loader: Some(loader.to_string()),
        ..Default::default()
    };
    let versions: Vec<VersionInfo> = provider.get_versions(project_id, &query)?;
    let Some(latest) = versions.first() else {
        return Ok(None);
    };

    if latest.version_number == module.version {
        return Ok(None);
    }
    let risk = classify_update(
        &module.version,
        &latest.version_number,
        latest.version_type.as_deref(),
    )?;

    Ok(Some(ModUpdate {
        mod_id: module.id.clone(),
        current_version: module.version.clone(),
        latest_version: latest.version_number.clone(),
        latest_version_id: latest.id.clone(),
        channel: latest.version_type.clone(),
        risk,
        pinned: false,
    }))
}

fn check_curseforge_update(
    module: &ModSpec,
    minecraft_version: &str,
    loader: &str,
) -> Result<Option<ModUpdate>, UpdateError> {
    let Some(project_id) = &module.source.project_id else {
        return Err(UpdateError::NoProjectId(module.id.clone()));
    };
    let mod_id: u64 = project_id
        .parse()
        .map_err(|_| UpdateError::NoProjectId(module.id.clone()))?;

    let provider = CurseForgeProvider::new();
    let files = provider.get_mod_files(mod_id, Some(minecraft_version))?;
    let Some(latest) = CurseForgeProvider::pick_best_file(&files, minecraft_version, loader) else {
        return Ok(None);
    };

    let latest_version = if latest.display_name.is_empty() {
        latest.file_name.clone()
    } else {
        latest.display_name.clone()
    };
    let current_file_id = module.source.file_id.as_deref().unwrap_or("");
    if current_file_id == latest.id.to_string() || latest_version == module.version {
        return Ok(None);
    }

    let channel = match latest.release_type {
        1 => Some("release"),
        2 => Some("beta"),
        3 => Some("alpha"),
        _ => None,
    };
    let risk =
        classify_update(&module.version, &latest_version, channel).unwrap_or(UpdateRisk::Risky);

    Ok(Some(ModUpdate {
        mod_id: module.id.clone(),
        current_version: module.version.clone(),
        latest_version,
        latest_version_id: latest.id.to_string(),
        channel: channel.map(str::to_string),
        risk,
        pinned: false,
    }))
}

fn check_github_update(module: &ModSpec) -> Result<Option<ModUpdate>, UpdateError> {
    let Some(repo) = &module.source.project_id else {
        return Err(UpdateError::NoProjectId(module.id.clone()));
    };
    // project_id = "owner/repo"
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("TuffBox-IDE/0.1.0")
        .build()
        .map_err(|e| UpdateError::Network(e.to_string()))?;
    let resp = client
        .get(&url)
        .send()
        .map_err(|e| UpdateError::Network(e.to_string()))?;
    if !resp.status().is_success() {
        return Ok(None);
    }
    let body: serde_json::Value = resp
        .json()
        .map_err(|e| UpdateError::Network(e.to_string()))?;
    let tag = body
        .get("tag_name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim_start_matches('v');
    if tag.is_empty() {
        return Ok(None);
    }
    let current = module
        .source
        .file_id
        .as_deref()
        .unwrap_or(&module.version)
        .trim_start_matches('v');
    if current == tag {
        return Ok(None);
    }
    let risk = classify_update(current, tag, Some("release")).unwrap_or(UpdateRisk::Risky);
    Ok(Some(ModUpdate {
        mod_id: module.id.clone(),
        current_version: module.version.clone(),
        latest_version: tag.to_string(),
        latest_version_id: tag.to_string(),
        channel: Some("release".into()),
        risk,
        pinned: false,
    }))
}

/// Checks every mod in the manifest, returning a map of `mod_id → update`.
pub fn check_manifest_updates(
    manifest: &crate::manifest::ProjectManifest,
) -> Result<HashMap<String, ModUpdate>, UpdateError> {
    let mut out = HashMap::new();
    let loader = manifest.loader.kind.as_str();
    for module in &manifest.mods {
        if let Some(update) = check_mod_update(module, &manifest.minecraft.version, loader)? {
            out.insert(module.id.clone(), update);
        }
    }
    Ok(out)
}

/// Applies the download metadata from a resolved latest version back onto a
/// [`ModSpec`], returning a new spec with bumped version/file/hashes. The
/// caller is responsible for persisting the manifest and re-materializing the
/// file via [`crate::mod_files`].
pub fn apply_update(module: &ModSpec, latest: &VersionInfo) -> ModSpec {
    let file = ProviderFileInfo::select_file_for_loader(latest, module.source.kind.as_str());
    let mut updated = module.clone();
    updated.version = latest.version_number.clone();
    updated.source.file_id = Some(latest.id.clone());
    if let Some(f) = file {
        updated.source.url = Some(f.url.clone());
        updated.file_name = Some(f.filename.clone());
        updated.hashes = Some(crate::manifest::FileHashes {
            sha1: f.hashes.sha1.clone(),
            sha512: f.hashes.sha512.clone(),
        });
    }
    updated
}

/// Apply a CurseForge file picked by [`check_curseforge_update`] onto a mod.
pub fn apply_curseforge_update(
    module: &ModSpec,
    file_id: u64,
    display_name: &str,
    file_name: &str,
    download_url: Option<String>,
    hashes: ProviderFileHashes,
) -> ModSpec {
    let mut updated = module.clone();
    updated.version = display_name.to_string();
    updated.source.file_id = Some(file_id.to_string());
    updated.file_name = Some(file_name.to_string());
    if let Some(url) = download_url {
        updated.source.url = Some(url);
    }
    updated.hashes = Some(crate::manifest::FileHashes {
        sha1: hashes.sha1,
        sha512: hashes.sha512,
    });
    updated
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_update_is_safe() {
        assert_eq!(
            classify_update("1.2.3", "1.2.4", Some("release")).unwrap(),
            UpdateRisk::Safe
        );
    }

    #[test]
    fn major_update_is_risky() {
        assert_eq!(
            classify_update("1.2.3", "2.0.0", Some("release")).unwrap(),
            UpdateRisk::Risky
        );
    }

    #[test]
    fn beta_channel_is_risky() {
        assert_eq!(
            classify_update("1.2.3", "1.2.4", Some("beta")).unwrap(),
            UpdateRisk::Risky
        );
    }

    #[test]
    fn no_v_prefix_parses() {
        assert_eq!(
            classify_update("v1.2.3", "1.2.4", Some("release")).unwrap(),
            UpdateRisk::Safe
        );
    }
}
