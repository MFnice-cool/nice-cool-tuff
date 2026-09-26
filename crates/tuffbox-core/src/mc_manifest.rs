//! Minecraft version manifest and per-version metadata from Mojang.
//!
//! Mirrors `daedalus::minecraft` for the sync/blocking world.  The manifest
//! is fetched from `piston-meta.mojang.com` and cached to disk so repeated
//! launches don't hit the network.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::http;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

pub const VERSION_MANIFEST_URL: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

/// How long a cached manifest stays fresh before we re-fetch.
const CACHE_TTL: Duration = Duration::from_secs(6 * 60 * 60); // 6 hours

const CACHE_PATH: &str = ".tuffbox/mc-version-manifest.json";

// ---------------------------------------------------------------------------
// Types — mirror daedalus::minecraft
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VersionType {
    Release,
    Snapshot,
    OldAlpha,
    OldBeta,
}

impl VersionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Release => "release",
            Self::Snapshot => "snapshot",
            Self::OldAlpha => "old_alpha",
            Self::OldBeta => "old_beta",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionEntry {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: VersionType,
    pub url: String,
    pub release_time: String,
    pub sha1: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatestVersion {
    pub release: String,
    pub snapshot: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionManifest {
    pub latest: LatestVersion,
    pub versions: Vec<VersionEntry>,
}

// ---------------------------------------------------------------------------
// Per-version detail (the URL each VersionEntry points to)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionDetail {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: VersionType,
    pub main_class: Option<String>,
    pub java_version: Option<JavaVersion>,
    pub libraries: Vec<Library>,
    pub downloads: Option<Downloads>,
    pub asset_index: Option<AssetIndex>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JavaVersion {
    pub component: String,
    pub major_version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetIndex {
    pub id: String,
    pub sha1: String,
    pub size: u32,
    pub total_size: u32,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Downloads {
    pub client: Option<DownloadArtifact>,
    pub server: Option<DownloadArtifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadArtifact {
    pub sha1: String,
    pub size: u32,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Library {
    pub name: String,
    #[serde(default)]
    pub downloads: Option<LibraryDownloads>,
    #[serde(default)]
    pub rules: Option<Vec<Rule>>,
    #[serde(default)]
    pub extract: Option<Extract>,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryDownloads {
    pub artifact: Option<LibraryDownloadArtifact>,
    #[serde(default)]
    pub classifiers: Option<HashMap<String, LibraryDownloadArtifact>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryDownloadArtifact {
    pub path: Option<String>,
    pub sha1: String,
    pub size: u32,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Extract {
    #[serde(default)]
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub action: String,
    #[serde(default)]
    pub os: Option<RuleOs>,
    #[serde(default)]
    pub features: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleOs {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
}

// ---------------------------------------------------------------------------
// Fetching + caching
// ---------------------------------------------------------------------------

/// Returns the on-disk path where the manifest cache is stored.
fn cache_path(instance_dir: &Path) -> PathBuf {
    instance_dir.join(CACHE_PATH)
}

/// Returns `true` if the cached manifest file exists and was written
/// less than `CACHE_TTL` ago.
fn cache_is_fresh(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    let Ok(modified) = meta.modified() else {
        return false;
    };
    SystemTime::now()
        .duration_since(modified)
        .map(|age| age < CACHE_TTL)
        .unwrap_or(false)
}

/// Fetches the version manifest, preferring a disk cache.
pub fn fetch_manifest(instance_dir: &Path) -> Result<VersionManifest, String> {
    let path = cache_path(instance_dir);

    if cache_is_fresh(&path) {
        if let Ok(raw) = std::fs::read_to_string(&path) {
            if let Ok(m) = serde_json::from_str::<VersionManifest>(&raw) {
                return Ok(m);
            }
        }
    }

    let manifest: VersionManifest = http::get_json_with_context(VERSION_MANIFEST_URL)
        .map_err(|e| format!("failed to fetch manifest: {e}"))?;

    // Best-effort cache write.
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(
        &path,
        serde_json::to_string_pretty(&manifest).unwrap_or_default(),
    );

    Ok(manifest)
}

/// Fetches the full detail for a specific version (libraries, downloads,
/// java version, etc.) by following the `url` field from the manifest.
pub fn fetch_version_detail(url: &str) -> Result<VersionDetail, String> {
    http::get_json_with_context(url).map_err(|e| format!("failed to fetch version detail: {e}"))
}

/// Looks up a version by ID in the manifest, returning its detail URL.
pub fn find_version<'a>(manifest: &'a VersionManifest, id: &str) -> Option<&'a VersionEntry> {
    manifest.versions.iter().find(|v| v.id == id)
}

/// Returns the latest release version entry.
pub fn latest_release(manifest: &VersionManifest) -> Option<&VersionEntry> {
    find_version(manifest, &manifest.latest.release)
}

/// Returns all release-type versions (no snapshots), newest first.
pub fn releases_only(manifest: &VersionManifest) -> Vec<&VersionEntry> {
    manifest
        .versions
        .iter()
        .filter(|v| v.type_ == VersionType::Release)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_type_roundtrip() {
        let json = r#""release""#;
        let v: VersionType = serde_json::from_str(json).unwrap();
        assert_eq!(v, VersionType::Release);
        assert_eq!(v.as_str(), "release");
    }

    #[test]
    fn find_version_returns_matching_entry() {
        let manifest = VersionManifest {
            latest: LatestVersion {
                release: "1.21".into(),
                snapshot: "24w30a".into(),
            },
            versions: vec![
                VersionEntry {
                    id: "1.21".into(),
                    type_: VersionType::Release,
                    url: "".into(),
                    release_time: "".into(),
                    sha1: "".into(),
                },
                VersionEntry {
                    id: "1.20.6".into(),
                    type_: VersionType::Release,
                    url: "".into(),
                    release_time: "".into(),
                    sha1: "".into(),
                },
            ],
        };
        assert!(find_version(&manifest, "1.21").is_some());
        assert!(find_version(&manifest, "1.19").is_none());
    }
}
