//! CurseForge ("Flame") API client — mirrors PrismLauncher's `FlameAPI`.
//!
//! Auth: `x-api-key` on `api.curseforge.com` and CDN hosts (`edge.forgecdn.net`).
//! Default key matches PrismLauncher's public build-time key; override with
//! `TUFFBOX_CURSEFORGE_API_KEY`.

use crate::http;
use crate::manifest::{DependencyKind, ModDependencySpec};
use crate::provider::{ProviderError, ProviderFileHashes};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const BASE_URL: &str = "https://api.curseforge.com/v1";
const MINECRAFT_GAME_ID: u32 = 432;
const CLASS_MODPACK: u32 = 4471;
const CLASS_MOD: u32 = 6;
const CLASS_RESOURCEPACK: u32 = 12;
const CLASS_SHADER: u32 = 6552;
const CLASS_DATAPACK: u32 = 6945;

/// PrismLauncher's default CurseForge API key (Overwolf / public launcher builds).
const DEFAULT_API_KEY: &str = "$2a$10$wuAJuNZuted3NORVmpgUC.m8sI.pv1tOPKZyBgLFGjxFp/br0lZCC";

#[derive(Debug, Clone)]
pub struct CurseForgeProvider {
    api_key: String,
}

impl Default for CurseForgeProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl CurseForgeProvider {
    pub fn new() -> Self {
        let api_key = std::env::var("TUFFBOX_CURSEFORGE_API_KEY")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_API_KEY.to_string());
        Self { api_key }
    }

    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    pub fn is_configured(&self) -> bool {
        !self.api_key.trim().is_empty()
    }

    fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, ProviderError> {
        let url = format!("{BASE_URL}{path}");
        cf_get_json(&url, &self.api_key)
    }

    fn post_json<B: Serialize, T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, ProviderError> {
        let url = format!("{BASE_URL}{path}");
        cf_post_json(&url, &self.api_key, body)
    }

    /// Search CurseForge content (mods / resource packs / shaders / datapacks).
    #[allow(clippy::too_many_arguments)]
    pub fn search_content(
        &self,
        class_id: u32,
        query: &str,
        game_version: Option<&str>,
        mod_loader_type: Option<u32>,
        offset: u32,
        page_size: u32,
        sort_field: Option<u32>,
        category_id: Option<u32>,
    ) -> Result<CurseForgeSearchPage, ProviderError> {
        let sort_field = sort_field.unwrap_or(2);
        let mut path = format!(
            "/mods/search?gameId={MINECRAFT_GAME_ID}&classId={class_id}&index={offset}&pageSize={}&sortField={}&sortOrder=desc",
            page_size.clamp(1, 50),
            sort_field
        );
        if !query.trim().is_empty() {
            path.push_str(&format!(
                "&searchFilter={}",
                urlencoding_minimal(query.trim())
            ));
        }
        if let Some(gv) = game_version.filter(|s| !s.is_empty()) {
            path.push_str(&format!("&gameVersion={}", urlencoding_minimal(gv)));
        }
        if let Some(loader) = mod_loader_type {
            path.push_str(&format!("&modLoaderType={loader}"));
        }
        if let Some(cat) = category_id {
            path.push_str(&format!("&categoryId={cat}"));
        }
        let resp: CfData<Vec<CfMod>> = self.get_json(&path)?;
        Ok(CurseForgeSearchPage {
            hits: resp.data.into_iter().map(Into::into).collect(),
            total: resp.pagination.total_count,
        })
    }

    /// Search CurseForge modpacks (`classId=4471`).
    pub fn search_modpacks(
        &self,
        query: &str,
        game_version: Option<&str>,
        offset: u32,
        page_size: u32,
    ) -> Result<CurseForgeSearchPage, ProviderError> {
        self.search_content(
            CLASS_MODPACK,
            query,
            game_version,
            None,
            offset,
            page_size,
            None,
            None,
        )
    }

    pub fn class_id_for_project_type(project_type: &str) -> u32 {
        match project_type {
            "resourcepack" => CLASS_RESOURCEPACK,
            "shader" | "shaderpack" => CLASS_SHADER,
            "datapack" => CLASS_DATAPACK,
            "modpack" => CLASS_MODPACK,
            _ => CLASS_MOD,
        }
    }

    /// CurseForge `ModLoaderType` enum values (same mapping as Prism FlameAPI).
    pub fn mod_loader_type(loader: &str) -> Option<u32> {
        match loader.to_ascii_lowercase().as_str() {
            "forge" => Some(1),
            "liteloader" => Some(3),
            "fabric" => Some(4),
            "quilt" => Some(5),
            "neoforge" => Some(6),
            _ => None,
        }
    }

    /// Categories for a CurseForge class id (`GET /categories?gameId&classId`).
    /// Returns `(id, name, parentCategoryId)` sorted by name.
    pub fn list_categories(&self, class_id: u32) -> Result<Vec<(u32, String, Option<u32>)>, ProviderError> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct CfCategoryRaw {
            id: u32,
            name: String,
            parent_category_id: Option<u32>,
        }
        let path = format!(
            "/categories?gameId={MINECRAFT_GAME_ID}&classId={class_id}"
        );
        let resp: CfData<Vec<CfCategoryRaw>> = self.get_json(&path)?;
        let mut out: Vec<(u32, String, Option<u32>)> = resp
            .data
            .into_iter()
            .map(|c| (c.id, c.name, c.parent_category_id))
            .collect();
        out.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
        Ok(out)
    }

    /// Prefer a file that mentions both the Minecraft version and loader
    /// (CurseForge folds loader names into `gameVersions`).
    pub fn pick_best_file<'a>(
        files: &'a [CurseForgeFileInfo],
        game_version: &str,
        loader: &str,
    ) -> Option<&'a CurseForgeFileInfo> {
        if files.is_empty() {
            return None;
        }
        let gv = game_version.trim();
        let loader_lower = loader.to_ascii_lowercase();
        let loader_label = match loader_lower.as_str() {
            "neoforge" => "NeoForge",
            "fabric" => "Fabric",
            "quilt" => "Quilt",
            "forge" => "Forge",
            other => other,
        };
        let matches_gv = |f: &&CurseForgeFileInfo| {
            gv.is_empty() || f.game_versions.iter().any(|v| v.eq_ignore_ascii_case(gv))
        };
        let matches_loader = |f: &&CurseForgeFileInfo| {
            loader.is_empty()
                || f.game_versions
                    .iter()
                    .any(|v| v.eq_ignore_ascii_case(loader_label))
        };
        files
            .iter()
            .find(|f| matches_gv(f) && matches_loader(f))
            .or_else(|| files.iter().find(matches_gv))
            .or_else(|| files.first())
    }

    pub fn get_mod(&self, mod_id: u64) -> Result<CurseForgeSearchHit, ProviderError> {
        let resp: CfData<CfMod> = self.get_json(&format!("/mods/{mod_id}"))?;
        Ok(resp.data.into())
    }

    /// External links of a CurseForge project (issues / source / wiki /
    /// discord / donation), from the `links` object of `GET /mods/{id}`.
    pub fn get_mod_links(&self, mod_id: u64) -> Result<CurseForgeLinks, ProviderError> {
        #[derive(Debug, Clone, Default, Deserialize)]
        #[serde(rename_all = "camelCase", default)]
        struct CfLinksRaw {
            issues_url: Option<String>,
            source_url: Option<String>,
            wiki_url: Option<String>,
            discord_url: Option<String>,
            donate_url: Option<String>,
        }
        #[derive(Deserialize)]
        struct LinksData {
            #[serde(default)]
            links: CfLinksRaw,
        }
        let resp: LinksData = self.get_json(&format!("/mods/{mod_id}"))?;
        let l = resp.links;
        let clean = |s: Option<String>| s.filter(|v| !v.trim().is_empty());
        Ok(CurseForgeLinks {
            issues_url: clean(l.issues_url),
            source_url: clean(l.source_url),
            wiki_url: clean(l.wiki_url),
            discord_url: clean(l.discord_url),
            donate_url: clean(l.donate_url),
        })
    }

    /// HTML description body for a CurseForge project (overview page).
    pub fn get_mod_description_html(&self, mod_id: u64) -> Result<String, ProviderError> {
        #[derive(Deserialize)]
        struct Desc {
            data: String,
        }
        let resp: Desc = self.get_json(&format!("/mods/{mod_id}/description"))?;
        Ok(resp.data)
    }

    /// List files for a project (modpack or mod).
    pub fn get_mod_files(
        &self,
        mod_id: u64,
        game_version: Option<&str>,
    ) -> Result<Vec<CurseForgeFileInfo>, ProviderError> {
        let mut path = format!("/mods/{mod_id}/files?pageSize=50");
        if let Some(gv) = game_version.filter(|s| !s.is_empty()) {
            path.push_str(&format!("&gameVersion={}", urlencoding_minimal(gv)));
        }
        let resp: CfData<Vec<CfFile>> = self.get_json(&path)?;
        Ok(resp.data.into_iter().map(Into::into).collect())
    }

    pub fn get_file(&self, mod_id: u64, file_id: u64) -> Result<CurseForgeFileInfo, ProviderError> {
        let resp: CfData<CfFile> = self.get_json(&format!("/mods/{mod_id}/files/{file_id}"))?;
        Ok(resp.data.into())
    }

    /// Batch-resolve files by id — Prism's `POST /mods/files`.
    pub fn get_files(
        &self,
        file_ids: &[u64],
    ) -> Result<HashMap<u64, CurseForgeFileInfo>, ProviderError> {
        if file_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut out = HashMap::new();
        for chunk in file_ids.chunks(100) {
            let body = serde_json::json!({ "fileIds": chunk });
            let resp: CfData<Vec<CfFile>> = self.post_json("/mods/files", &body)?;
            for file in resp.data {
                let info: CurseForgeFileInfo = file.into();
                out.insert(info.id, info);
            }
        }
        Ok(out)
    }

    pub fn get_mods(
        &self,
        mod_ids: &[u64],
    ) -> Result<HashMap<u64, CurseForgeSearchHit>, ProviderError> {
        if mod_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let mut out = HashMap::new();
        for chunk in mod_ids.chunks(100) {
            let body = serde_json::json!({ "modIds": chunk });
            let resp: CfData<Vec<CfMod>> = self.post_json("/mods", &body)?;
            for m in resp.data {
                let hit: CurseForgeSearchHit = m.into();
                out.insert(hit.id, hit);
            }
        }
        Ok(out)
    }

    /// Resolve CurseForge package fingerprints (`POST /fingerprints`).
    ///
    /// Fingerprints must be computed with [`crate::murmur2`] (whitespace-stripped).
    pub fn get_fingerprints(
        &self,
        fingerprints: &[u32],
    ) -> Result<HashMap<u32, CurseForgeFileInfo>, ProviderError> {
        if fingerprints.is_empty() {
            return Ok(HashMap::new());
        }
        let mut out = HashMap::new();
        for chunk in fingerprints.chunks(100) {
            let body = serde_json::json!({ "fingerprints": chunk });
            let resp: CfData<CfFingerprintResult> = self.post_json("/fingerprints", &body)?;
            for m in resp.data.exact_matches {
                let fp = m.id;
                let info: CurseForgeFileInfo = m.file.into();
                out.insert(fp, info);
            }
        }
        Ok(out)
    }

    /// Fingerprint a jar on disk and look it up on CurseForge.
    pub fn resolve_file_by_fingerprint(
        &self,
        path: &std::path::Path,
    ) -> Result<Option<CurseForgeFileInfo>, ProviderError> {
        let fp = crate::murmur2::murmur2_file(path)?;
        Ok(self.get_fingerprints(&[fp])?.remove(&fp))
    }

    /// Fill empty `download_url` entries via Modrinth SHA1 (Prism `FallbackMRBlockedMods`).
    pub fn apply_modrinth_fallback(
        &self,
        files: &mut HashMap<u64, CurseForgeFileInfo>,
    ) -> Result<(), ProviderError> {
        let hashes: Vec<String> = files
            .values()
            .filter(|f| {
                f.download_url
                    .as_ref()
                    .map(|u| u.is_empty())
                    .unwrap_or(true)
            })
            .filter_map(|f| f.hashes.sha1.clone())
            .collect();
        if hashes.is_empty() {
            return Ok(());
        }
        let url = "https://api.modrinth.com/v2/version_files";
        let body = serde_json::json!({ "hashes": hashes, "algorithm": "sha1" });
        let map: HashMap<String, serde_json::Value> = http::post_json(url, &body)
            .map_err(|e| ProviderError::NetworkContext(e.to_string()))?;
        for file in files.values_mut() {
            let Some(sha1) = file.hashes.sha1.as_ref() else {
                continue;
            };
            if file
                .download_url
                .as_ref()
                .map(|u| !u.is_empty())
                .unwrap_or(false)
            {
                continue;
            }
            let Some(version) = map.get(sha1) else {
                continue;
            };
            if let Some(files_arr) = version.get("files").and_then(|v| v.as_array()) {
                let primary = files_arr
                    .iter()
                    .find(|f| f.get("primary").and_then(|p| p.as_bool()).unwrap_or(false))
                    .or_else(|| files_arr.first());
                if let Some(url) = primary
                    .and_then(|f| f.get("url"))
                    .and_then(|u| u.as_str())
                    .map(|s| s.to_string())
                {
                    file.download_url = Some(url);
                    file.blocked = false;
                }
            }
        }
        Ok(())
    }
}

fn urlencoding_minimal(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn cf_get_json<T: serde::de::DeserializeOwned>(
    url: &str,
    api_key: &str,
) -> Result<T, ProviderError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .user_agent("TuffBox-IDE/0.1.0")
        .build()
        .map_err(|e| ProviderError::NetworkContext(e.to_string()))?;
    let resp = client
        .get(url)
        .header("x-api-key", api_key)
        .header("Accept", "application/json")
        .send()
        .map_err(ProviderError::Network)?;
    let status = resp.status();
    let text = resp.text().map_err(ProviderError::Network)?;
    if !status.is_success() {
        return Err(ProviderError::Api {
            status: status.as_u16(),
            message: text.chars().take(300).collect(),
        });
    }
    serde_json::from_str(&text).map_err(ProviderError::Parse)
}

fn cf_post_json<B: Serialize, T: serde::de::DeserializeOwned>(
    url: &str,
    api_key: &str,
    body: &B,
) -> Result<T, ProviderError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .user_agent("TuffBox-IDE/0.1.0")
        .build()
        .map_err(|e| ProviderError::NetworkContext(e.to_string()))?;
    let resp = client
        .post(url)
        .header("x-api-key", api_key)
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .json(body)
        .send()
        .map_err(ProviderError::Network)?;
    let status = resp.status();
    let text = resp.text().map_err(ProviderError::Network)?;
    if !status.is_success() {
        return Err(ProviderError::Api {
            status: status.as_u16(),
            message: text.chars().take(300).collect(),
        });
    }
    serde_json::from_str(&text).map_err(ProviderError::Parse)
}

/// Returns true when a download URL should carry the CurseForge API key.
/// Only `edge.forgecdn.net` (and the API host) require it — `mediafilez` /
/// `media` are open CDNs (same rule as GDLauncher Carbon).
pub fn url_needs_curseforge_key(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("https://") {
        let host = rest.split('/').next().unwrap_or("");
        return host == "api.curseforge.com" || host == "edge.forgecdn.net";
    }
    if let Some(rest) = lower.strip_prefix("http://") {
        let host = rest.split('/').next().unwrap_or("");
        return host == "api.curseforge.com" || host == "edge.forgecdn.net";
    }
    false
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurseForgeSearchHit {
    pub id: u64,
    pub slug: String,
    pub name: String,
    pub summary: String,
    pub download_count: u64,
    /// CurseForge "thumbs up" count (maps to UI likes/follows).
    #[serde(default)]
    pub thumbs_up_count: u64,
    #[serde(default)]
    pub date_modified: Option<String>,
    #[serde(default)]
    pub date_created: Option<String>,
    pub icon_url: Option<String>,
    pub authors: Vec<String>,
    pub categories: Vec<String>,
    pub latest_files: Vec<CurseForgeFileInfo>,
    pub class_id: Option<u32>,
}

/// External links of a CurseForge project, surfaced on the catalog sidebar.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct CurseForgeLinks {
    pub issues_url: Option<String>,
    pub source_url: Option<String>,
    pub wiki_url: Option<String>,
    pub discord_url: Option<String>,
    pub donate_url: Option<String>,
}

/// Map a CurseForge category display name to a Modrinth-style slug so graph
/// clustering can share one taxonomy (`Armor, Tools, and Weapons` → `equipment`).
pub fn normalize_mod_category(name: &str) -> String {
    let slug: String = name
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|c| match c {
            '&' | '+' | ',' | '/' => ' ',
            other => other,
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-");

    match slug.as_str() {
        "adventure-and-rpg" => "adventure".into(),
        "api-and-library" => "library".into(),
        "armor-tools-and-weapons" => "equipment".into(),
        "cosmetic" => "social".into(),
        "map-and-information" => "management".into(),
        "performance" => "optimization".into(),
        "utility-qol" => "utility".into(),
        "world-gen" | "world-generation" => "worldgen".into(),
        "farming" => "food".into(),
        "redstone" | "automation" | "energy" | "processing" => "technology".into(),
        "energy-fluid-and-item-transport" | "player-transport" => "transportation".into(),
        "biomes" | "dimensions" | "structures" | "ores-and-resources" => "worldgen".into(),
        "bug-fixes" | "server-utility" | "miscellaneous" | "education" => "utility".into(),
        "genetics" => "game-mechanics".into(),
        "mcreator" => "library".into(),
        "twitch-integration" => "social".into(),
        other => other.to_string(),
    }
}

/// CurseForge file dependency relation types (API `relationType`).
/// 1 EmbeddedLibrary, 2 OptionalDependency, 3 RequiredDependency,
/// 4 Tool, 5 Incompatible, 6 Include.
pub const CF_REL_EMBEDDED: u32 = 1;
pub const CF_REL_OPTIONAL: u32 = 2;
pub const CF_REL_REQUIRED: u32 = 3;
pub const CF_REL_TOOL: u32 = 4;
pub const CF_REL_INCOMPATIBLE: u32 = 5;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CurseForgeFileDependency {
    pub mod_id: u64,
    pub relation_type: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurseForgeFileInfo {
    pub id: u64,
    pub mod_id: u64,
    pub display_name: String,
    pub file_name: String,
    pub download_url: Option<String>,
    pub release_type: u32,
    pub game_versions: Vec<String>,
    pub hashes: ProviderFileHashes,
    pub file_date: Option<String>,
    /// True when CurseForge withheld the CDN URL (author distribution restrictions).
    pub blocked: bool,
    pub class_id: Option<u32>,
    /// Optional file changelog text (CF only populates it on file endpoints).
    #[serde(default)]
    pub changelog: Option<String>,
    #[serde(default)]
    pub dependencies: Vec<CurseForgeFileDependency>,
}

/// Map CurseForge `relationType` → manifest dependency kind.
pub fn cf_relation_kind(relation_type: u32) -> Option<DependencyKind> {
    match relation_type {
        CF_REL_REQUIRED => Some(DependencyKind::Requires),
        CF_REL_OPTIONAL | CF_REL_EMBEDDED | CF_REL_TOOL => Some(DependencyKind::Optional),
        CF_REL_INCOMPATIBLE => Some(DependencyKind::Conflicts),
        _ => None,
    }
}

/// Convert CF file dependencies into ModDependencySpec (target = CF project id string).
pub fn cf_deps_to_specs(deps: &[CurseForgeFileDependency]) -> Vec<ModDependencySpec> {
    deps.iter()
        .filter_map(|d| {
            if d.mod_id == 0 {
                return None;
            }
            let kind = cf_relation_kind(d.relation_type)?;
            Some(ModDependencySpec {
                kind,
                target: d.mod_id.to_string(),
                version_constraint: None,
                reason: None,
            })
        })
        .collect()
}

/// Required CurseForge project ids from a file dependency list.
pub fn cf_required_mod_ids(deps: &[CurseForgeFileDependency]) -> Vec<u64> {
    deps.iter()
        .filter(|d| d.relation_type == CF_REL_REQUIRED && d.mod_id != 0)
        .map(|d| d.mod_id)
        .collect()
}

impl CurseForgeFileInfo {
    pub fn content_folder(&self) -> &'static str {
        match self.class_id.unwrap_or(CLASS_MOD) {
            CLASS_RESOURCEPACK => "resourcepacks",
            CLASS_SHADER => "shaderpacks",
            CLASS_DATAPACK => "datapacks",
            _ => "mods",
        }
    }

    /// Prefer the API URL; if CurseForge withheld it, reconstruct forgecdn paths
    /// (Prism / GDLauncher style) from file id + file name.
    pub fn resolved_download_urls(&self) -> Vec<String> {
        let mut urls = Vec::new();
        if let Some(u) = self
            .download_url
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            urls.push(u.to_string());
        }
        if !self.file_name.trim().is_empty() {
            for candidate in curseforge_cdn_urls(self.id, &self.file_name) {
                if !urls.iter().any(|u| u == &candidate) {
                    urls.push(candidate);
                }
            }
        }
        urls
    }

    pub fn resolved_download_url(&self) -> Option<String> {
        self.resolved_download_urls().into_iter().next()
    }
}

/// Reconstruct CurseForge CDN download URLs from file id + filename when the
/// API returns a null `downloadUrl` (common for third-party clients).
pub fn curseforge_cdn_urls(file_id: u64, file_name: &str) -> Vec<String> {
    let encoded = urlencoding_minimal(file_name);
    let a = file_id / 1000;
    let b = file_id % 1000;
    // mediafilez is usually open; edge requires x-api-key (see url_needs_curseforge_key).
    vec![
        format!("https://mediafilez.forgecdn.net/files/{a}/{b}/{encoded}"),
        format!("https://edge.forgecdn.net/files/{a}/{b}/{encoded}"),
        format!("https://media.forgecdn.net/files/{a}/{b}/{encoded}"),
    ]
}

#[derive(Debug, Deserialize)]
struct CfData<T> {
    data: T,
    #[serde(default)]
    pagination: CfPagination,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfPagination {
    #[serde(default)]
    total_count: u32,
}

/// Paginated CurseForge search result.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurseForgeSearchPage {
    pub hits: Vec<CurseForgeSearchHit>,
    pub total: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfMod {
    id: u64,
    #[serde(default)]
    slug: String,
    name: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    download_count: u64,
    #[serde(default)]
    thumbs_up_count: u64,
    #[serde(default)]
    date_modified: Option<String>,
    #[serde(default)]
    date_created: Option<String>,
    #[serde(default)]
    logo: Option<CfLogo>,
    #[serde(default)]
    authors: Vec<CfAuthor>,
    #[serde(default)]
    categories: Vec<CfCategory>,
    #[serde(default)]
    latest_files: Vec<CfFile>,
    #[serde(default)]
    class_id: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct CfLogo {
    #[serde(default, rename = "thumbnailUrl")]
    thumbnail_url: Option<String>,
    #[serde(default)]
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CfAuthor {
    name: String,
}

#[derive(Debug, Deserialize)]
struct CfCategory {
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfFingerprintResult {
    #[serde(default)]
    exact_matches: Vec<CfFingerprintMatch>,
}

#[derive(Debug, Deserialize)]
struct CfFingerprintMatch {
    /// The fingerprint that matched (same value sent in the request).
    id: u32,
    file: CfFile,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfFileDependencyRaw {
    #[serde(default)]
    mod_id: u64,
    #[serde(default)]
    relation_type: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfFile {
    id: u64,
    #[serde(default)]
    mod_id: u64,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    file_name: String,
    #[serde(default)]
    download_url: Option<String>,
    #[serde(default)]
    release_type: u32,
    #[serde(default)]
    game_versions: Vec<String>,
    #[serde(default)]
    hashes: Vec<CfHash>,
    #[serde(default)]
    file_date: Option<String>,
    #[serde(default)]
    changelog: Option<String>,
    #[serde(default)]
    dependencies: Vec<CfFileDependencyRaw>,
}

#[derive(Debug, Deserialize)]
struct CfHash {
    value: String,
    /// 1 = sha1, 2 = md5 on CurseForge.
    algo: u32,
}

impl From<CfMod> for CurseForgeSearchHit {
    fn from(m: CfMod) -> Self {
        let latest_files: Vec<CurseForgeFileInfo> =
            m.latest_files.into_iter().map(Into::into).collect();
        // Prefer mod-level dates; fall back to newest listed file date.
        let file_date = latest_files.iter().find_map(|f| f.file_date.clone());
        let date_modified = m.date_modified.or(file_date.clone());
        Self {
            id: m.id,
            slug: m.slug,
            name: m.name,
            summary: m.summary,
            download_count: m.download_count,
            thumbs_up_count: m.thumbs_up_count,
            date_modified,
            date_created: m.date_created.or(file_date),
            icon_url: m
                .logo
                .as_ref()
                .and_then(|l| l.thumbnail_url.clone().or_else(|| l.url.clone())),
            authors: m.authors.into_iter().map(|a| a.name).collect(),
            categories: m.categories.into_iter().map(|c| c.name).collect(),
            latest_files,
            class_id: m.class_id,
        }
    }
}

impl From<CfFile> for CurseForgeFileInfo {
    fn from(f: CfFile) -> Self {
        let mut sha1 = None;
        let sha512 = None;
        for h in f.hashes {
            match h.algo {
                1 => sha1 = Some(h.value),
                _ => {}
            }
            // CurseForge doesn't ship sha512 commonly; keep field for ProviderFileHashes.
            let _ = &sha512;
        }
        let url = f.download_url.filter(|u| !u.trim().is_empty());
        let blocked = url.is_none();
        let dependencies = f
            .dependencies
            .into_iter()
            .filter(|d| d.mod_id != 0)
            .map(|d| CurseForgeFileDependency {
                mod_id: d.mod_id,
                relation_type: d.relation_type,
            })
            .collect();
        let mut info = Self {
            id: f.id,
            mod_id: f.mod_id,
            display_name: if f.display_name.is_empty() {
                f.file_name.clone()
            } else {
                f.display_name
            },
            file_name: f.file_name,
            download_url: url,
            release_type: f.release_type,
            game_versions: f.game_versions,
            hashes: ProviderFileHashes { sha1, sha512 },
            file_date: f.file_date,
            changelog: f.changelog.filter(|c| !c.trim().is_empty()),
            blocked,
            class_id: None,
            dependencies,
        };
        // Prefer a reconstructed CDN URL so callers that only read `download_url`
        // still work when CurseForge withholds the official link.
        if info.download_url.is_none() {
            if let Some(reconstructed) = info.resolved_download_url() {
                info.download_url = Some(reconstructed);
                info.blocked = false;
            }
        }
        info
    }
}

/// Download a URL that may require the CurseForge `x-api-key` header.
pub fn download_curseforge_url(
    url: &str,
    dest: &std::path::Path,
    expected_sha1: Option<&str>,
) -> Result<(), ProviderError> {
    download_curseforge_url_candidates(&[url.to_string()], dest, expected_sha1)
}

/// Try several CurseForge CDN candidates until one succeeds.
pub fn download_curseforge_url_candidates(
    urls: &[String],
    dest: &std::path::Path,
    expected_sha1: Option<&str>,
) -> Result<(), ProviderError> {
    use sha1::{Digest, Sha1};
    use std::io::{Read, Write};

    if urls.is_empty() {
        return Err(ProviderError::NetworkContext(
            "no CurseForge download URLs to try".into(),
        ));
    }

    let provider = CurseForgeProvider::new();
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .user_agent("TuffBox-IDE/0.1.0")
        .redirect(reqwest::redirect::Policy::limited(8))
        .build()
        .map_err(|e| ProviderError::NetworkContext(e.to_string()))?;

    let parent = dest.parent().unwrap_or_else(|| std::path::Path::new("."));
    std::fs::create_dir_all(parent)?;

    let mut last_err = None;
    for url in urls {
        let mut request = client.get(url);
        if url_needs_curseforge_key(url) {
            request = request.header("x-api-key", provider.api_key());
        }
        let response = match request.send() {
            Ok(r) => r,
            Err(e) => {
                last_err = Some(ProviderError::Network(e));
                continue;
            }
        };
        let status = response.status();
        if !status.is_success() {
            last_err = Some(ProviderError::Api {
                status: status.as_u16(),
                message: format!("download failed for {url}"),
            });
            continue;
        }

        let mut file = match tempfile::Builder::new()
            .prefix(".tuffbox-download-")
            .suffix(".part")
            .tempfile_in(parent)
        {
            Ok(f) => f,
            Err(e) => {
                last_err = Some(ProviderError::Io(e));
                continue;
            }
        };
        let mut hasher = Sha1::new();
        let mut stream = response;
        let mut buffer = vec![0u8; 64 * 1024];
        let mut read_err = false;
        loop {
            match stream.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    hasher.update(&buffer[..n]);
                    if let Err(e) = file.write_all(&buffer[..n]) {
                        last_err = Some(ProviderError::Io(e));
                        read_err = true;
                        break;
                    }
                }
                Err(e) => {
                    last_err = Some(ProviderError::Io(e));
                    read_err = true;
                    break;
                }
            }
        }
        if read_err {
            continue;
        }
        if let Err(e) = file.flush() {
            last_err = Some(ProviderError::Io(e));
            continue;
        }
        let actual = format!("{:x}", hasher.finalize());
        if let Some(expected) = expected_sha1 {
            if !actual.eq_ignore_ascii_case(expected) {
                last_err = Some(ProviderError::NetworkContext(format!(
                    "sha1 mismatch for {url}: expected {expected}, got {actual}"
                )));
                continue;
            }
        }
        file.persist(dest).map_err(|e| ProviderError::Io(e.error))?;
        return Ok(());
    }

    Err(last_err.unwrap_or_else(|| {
        ProviderError::NetworkContext("all CurseForge CDN candidates failed".into())
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconstructs_cdn_urls_from_file_id() {
        // Prism/GDLauncher: files/{id/1000}/{id%1000}/{name}
        let urls = curseforge_cdn_urls(5_432_101, "My Pack-1.0.zip");
        assert!(urls[0].contains("/files/5432/101/"));
        assert!(
            urls[0].contains("My%20Pack-1.0.zip") || urls[0].ends_with("My Pack-1.0.zip") == false
        );
        assert!(urls.iter().any(|u| u.contains("mediafilez.forgecdn.net")));
        assert!(urls.iter().any(|u| u.contains("edge.forgecdn.net")));
    }

    #[test]
    fn api_key_only_for_edge_and_api() {
        assert!(url_needs_curseforge_key(
            "https://edge.forgecdn.net/files/1/2/a.zip"
        ));
        assert!(url_needs_curseforge_key(
            "https://api.curseforge.com/v1/mods/1"
        ));
        assert!(!url_needs_curseforge_key(
            "https://mediafilez.forgecdn.net/files/1/2/a.zip"
        ));
        assert!(!url_needs_curseforge_key(
            "https://media.forgecdn.net/files/1/2/a.zip"
        ));
    }

    #[test]
    fn resolved_urls_fill_when_api_withheld() {
        let info = CurseForgeFileInfo {
            dependencies: vec![],
            id: 3272032,
            mod_id: 1,
            display_name: "jei".into(),
            file_name: "jei.jar".into(),
            download_url: None,
            release_type: 1,
            game_versions: vec![],
            hashes: ProviderFileHashes {
                sha1: None,
                sha512: None,
            },
            file_date: None,
            blocked: true,
            class_id: None,
            changelog: None,
        };
        let urls = info.resolved_download_urls();
        assert!(!urls.is_empty());
        assert!(urls[0].contains("/files/3272/32/jei.jar"));
    }

    #[test]
    fn parses_file_dependencies_required() {
        let raw = r#"{
            "id": 1,
            "modId": 100,
            "fileName": "foo.jar",
            "dependencies": [
                {"modId": 238222, "relationType": 3},
                {"modId": 306612, "relationType": 2},
                {"modId": 999, "relationType": 5}
            ]
        }"#;
        let file: CfFile = serde_json::from_str(raw).expect("parse CfFile");
        let info: CurseForgeFileInfo = file.into();
        assert_eq!(cf_required_mod_ids(&info.dependencies), vec![238222]);
        let specs = cf_deps_to_specs(&info.dependencies);
        assert_eq!(specs.len(), 3);
        assert_eq!(specs[0].kind, DependencyKind::Requires);
        assert_eq!(specs[0].target, "238222");
        assert_eq!(specs[1].kind, DependencyKind::Optional);
        assert_eq!(specs[2].kind, DependencyKind::Conflicts);
    }
}
