//! Fabulously Optimized (FO) pack parsing — pure helpers, no network.
//!
//! The Optimize FO variant downloads the official FO `.mrpack` from Modrinth and
//! installs the **exact pinned builds** listed in its `modrinth.index.json`
//! (unlike the curated flow, where `resolve_dependencies` walks version deps).
//! Everything here is pure string/JSON handling so it is unit-testable without
//! Modrinth access; network calls live in the desktop command layer.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// One mod entry extracted from `modrinth.index.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FoIndexMod {
    /// Display fallback derived from the jar name (real names come from batch lookup).
    pub file_name: String,
    /// Modrinth project id parsed from the CDN download URL (may be empty pre-fallback).
    #[serde(default)]
    pub project_id: String,
    /// Modrinth version id parsed from the CDN download URL (may be empty pre-fallback).
    #[serde(default)]
    pub version_id: String,
    /// SHA-1 of the jar, used for the `get_version_by_hash` fallback.
    #[serde(default)]
    pub sha1: String,
}

/// Stats for a parsed index, surfaced as preview warnings.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FoIndexStats {
    pub total_files: usize,
    pub mod_files: usize,
    pub skipped_non_mod: usize,
    pub skipped_client_unsupported: usize,
}

#[derive(Debug, Deserialize)]
struct IndexFile {
    path: String,
    hashes: IndexHashes,
    #[serde(default)]
    downloads: Vec<String>,
    #[serde(default)]
    env: Option<HashMap<String, String>>,
}

#[derive(Debug, Default, Deserialize)]
struct IndexHashes {
    #[serde(default)]
    sha1: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MrpackIndex {
    #[serde(default)]
    files: Vec<IndexFile>,
}

/// Extracts `(project_id, version_id)` from a Modrinth CDN download URL.
///
/// Expected shape: `https://cdn.modrinth.com/data/{project_id}/versions/{version_id}/{file}`.
/// Returns `None` for foreign hosts or malformed paths.
pub fn modrinth_ids_from_file_url(url: &str) -> Option<(String, String)> {
    if !url.contains("modrinth.com/data/") {
        return None;
    }
    let after_data = url.split("/data/").nth(1)?;
    let mut parts = after_data.split('/');
    let project_id = parts.next().unwrap_or_default();
    let versions_marker = parts.next().unwrap_or_default();
    let version_id = parts.next().unwrap_or_default();
    if versions_marker != "versions" {
        return None;
    }
    let project_id = project_id.trim();
    let version_id = version_id.split('?').next().unwrap_or_default().trim();
    if project_id.is_empty()
        || version_id.is_empty()
        || !project_id.chars().all(|c| c.is_ascii_alphanumeric())
        || !version_id.chars().all(|c| c.is_ascii_alphanumeric())
    {
        return None;
    }
    Some((project_id.to_string(), version_id.to_string()))
}

/// Human-readable fallback name from a jar file name
/// (`sodium-fabric-0.6.9.jar` → `sodium fabric 0.6.9`). Only used when the
/// batch project lookup has no entry for a parsed project id.
pub fn pretty_name_from_file_name(file_name: &str) -> String {
    let stem = file_name
        .rsplit_once('.')
        .map(|(base, _)| base)
        .unwrap_or(file_name);
    stem.replace(['-', '_', '+'], " ")
}

/// Parses `modrinth.index.json` content into client-side jar mods.
///
/// Kept: `mods/*.jar` entries whose `env.client` is not `"unsupported"`.
/// Everything else (resource packs, configs, server-only jars) is counted in stats.
pub fn fo_mods_from_index_json(index_json: &str) -> Result<(Vec<FoIndexMod>, FoIndexStats), String> {
    let index: MrpackIndex =
        serde_json::from_str(index_json).map_err(|e| format!("invalid modrinth.index.json: {e}"))?;
    let mut mods = Vec::new();
    let mut stats = FoIndexStats {
        total_files: index.files.len(),
        ..FoIndexStats::default()
    };
    for file in index.files {
        let file_name = file
            .path
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .to_string();
        let is_mod_jar = file.path.starts_with("mods/")
            && file_name.to_lowercase().ends_with(".jar")
            && !file_name.starts_with('.');
        if !is_mod_jar {
            stats.skipped_non_mod += 1;
            continue;
        }
        let client_env = file
            .env
            .as_ref()
            .and_then(|env| env.get("client"))
            .map(|v| v.to_lowercase());
        if client_env.as_deref() == Some("unsupported") {
            stats.skipped_client_unsupported += 1;
            continue;
        }
        let (project_id, version_id) = file
            .downloads
            .iter()
            .find_map(|url| modrinth_ids_from_file_url(url))
            .unwrap_or_default();
        mods.push(FoIndexMod {
            file_name,
            project_id,
            version_id,
            sha1: file.hashes.sha1.unwrap_or_default(),
        });
    }
    stats.mod_files = mods.len();
    Ok((mods, stats))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_standard_cdn_url() {
        let url = "https://cdn.modrinth.com/data/AANobbMI/versions/abc123XY/sodium-fabric-0.6.9.jar";
        assert_eq!(
            modrinth_ids_from_file_url(url),
            Some(("AANobbMI".to_string(), "abc123XY".to_string()))
        );
    }

    #[test]
    fn rejects_foreign_and_malformed_urls() {
        assert_eq!(modrinth_ids_from_file_url("https://example.com/data/A/versions/B/f.jar"), None);
        assert_eq!(modrinth_ids_from_file_url("https://cdn.modrinth.com/mods/sodium"), None);
        assert_eq!(modrinth_ids_from_file_url("https://cdn.modrinth.com/data//versions//f.jar"), None);
        assert_eq!(
            modrinth_ids_from_file_url("https://cdn.modrinth.com/data/AANobbMI/files/f.jar"),
            None
        );
    }

    #[test]
    fn pretty_name_replaces_separators() {
        assert_eq!(
            pretty_name_from_file_name("sodium-fabric-0.6.9.jar"),
            "sodium fabric 0.6.9"
        );
        assert_eq!(pretty_name_from_file_name("mod_menu-11.0.3.jar"), "mod menu 11.0.3");
    }

    #[test]
    fn index_filters_mods_and_counts_skips() {
        let index = r#"{
            "formatVersion": 1, "game": "minecraft", "versionId": "v1",
            "name": "FO", "files": [
                {"path": "mods/sodium-0.6.9.jar",
                 "hashes": {"sha1": "aaa"},
                 "downloads": ["https://cdn.modrinth.com/data/AANobbMI/versions/abc123XY/sodium-0.6.9.jar"],
                 "env": {"client": "required", "server": "unsupported"}},
                {"path": "mods/optional-mod.jar",
                 "hashes": {"sha1": "bbb"},
                 "downloads": ["https://cdn.modrinth.com/data/BBB11111/versions/ccc22222/optional-mod.jar"],
                 "env": {"client": "optional"}},
                {"path": "mods/no-env-mod.jar",
                 "hashes": {"sha1": "ccc"},
                 "downloads": ["https://cdn.modrinth.com/data/CCC11111/versions/ddd22222/no-env-mod.jar"]},
                {"path": "mods/server-only.jar",
                 "hashes": {"sha1": "ddd"},
                 "downloads": ["https://cdn.modrinth.com/data/DDD11111/versions/eee22222/server-only.jar"],
                 "env": {"client": "unsupported"}},
                {"path": "resourcepacks/rp.zip",
                 "hashes": {"sha1": "eee"},
                 "downloads": ["https://cdn.modrinth.com/data/EEE11111/versions/fff22222/rp.zip"]},
                {"path": "mods/readme.txt",
                 "hashes": {"sha1": "fff"},
                 "downloads": ["https://cdn.modrinth.com/data/FFF11111/versions/ggg22222/readme.txt"]}
            ]
        }"#;
        let (mods, stats) = fo_mods_from_index_json(index).expect("parse");
        assert_eq!(mods.len(), 3);
        assert_eq!(mods[0].project_id, "AANobbMI");
        assert_eq!(mods[0].version_id, "abc123XY");
        assert_eq!(mods[0].sha1, "aaa");
        assert_eq!(mods[2].project_id, "CCC11111");
        assert_eq!(stats.total_files, 6);
        assert_eq!(stats.mod_files, 3);
        assert_eq!(stats.skipped_non_mod, 2);
        assert_eq!(stats.skipped_client_unsupported, 1);
    }

    #[test]
    fn index_keeps_mods_missing_parseable_urls_for_hash_fallback() {
        let index = r#"{
            "formatVersion": 1, "game": "minecraft", "versionId": "v1",
            "name": "FO", "files": [
                {"path": "mods/mirror-mod.jar",
                 "hashes": {"sha1": "9dbc2e8"},
                 "downloads": ["https://edge.example.com/mirror-mod.jar"]}
            ]
        }"#;
        let (mods, stats) = fo_mods_from_index_json(index).expect("parse");
        assert_eq!(mods.len(), 1);
        assert_eq!(mods[0].project_id, "");
        assert_eq!(mods[0].sha1, "9dbc2e8");
        assert_eq!(stats.mod_files, 1);
    }

    #[test]
    fn invalid_json_errors() {
        assert!(fo_mods_from_index_json("{not json").is_err());
    }
}
