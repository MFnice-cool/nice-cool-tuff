pub mod curseforge;
pub mod local;
pub mod modrinth;

pub use curseforge::{normalize_mod_category, CurseForgeProvider};
pub use local::LocalJarProvider;
pub use modrinth::{markdown_to_html, ModrinthCategory, ModrinthProvider};

use crate::manifest::{DependencyKind, ModDependencySpec};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("network request failed: {0}")]
    Network(#[from] reqwest::Error),
    #[error("{0}")]
    NetworkContext(String),
    #[error("failed to parse response: {0}")]
    Parse(#[source] serde_json::Error),
    #[error("provider returned status {status}: {message}")]
    Api { status: u16, message: String },
    #[error("project not found: {0}")]
    ProjectNotFound(String),
    #[error("version not found: {0}")]
    VersionNotFound(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("unsupported operation for provider")]
    Unsupported,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSearchQuery {
    pub query: Option<String>,
    pub minecraft_version: Option<String>,
    pub loader: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub environment: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub sort: Option<String>,
    #[serde(default)]
    pub limit: Option<u32>,
    /// Modrinth project type to restrict the search to: `mod`, `resourcepack`,
    /// `datapack`, `shader`, `plugin`, `modpack`. Defaults to `mod` when unset
    /// by the caller-side UI, but is left optional here so other providers
    /// that don't distinguish content types can ignore it.
    #[serde(default)]
    pub project_type: Option<String>,
    /// Zero-based offset for server-side pagination (Modrinth + CurseForge).
    #[serde(default)]
    pub offset: Option<u32>,
}

/// Paginated search result. `total` is the backend's reported match count
/// (not just the length of `results`), enabling real paged browsing.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchPage {
    pub results: Vec<ProjectInfo>,
    pub total: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInfo {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub description: String,
    pub project_type: String,
    pub icon_url: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub downloads: Option<u64>,
    #[serde(default)]
    pub follows: Option<u64>,
    #[serde(default)]
    pub date_modified: Option<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub client_side: Option<String>,
    #[serde(default)]
    pub server_side: Option<String>,
    #[serde(default)]
    pub issues_url: Option<String>,
    #[serde(default)]
    pub source_url: Option<String>,
    /// Project creation timestamp (`published` on Modrinth GET /project,
    /// `dateCreated` on CurseForge). Absent for search hits / local projects.
    #[serde(default)]
    pub date_created: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionInfo {
    pub id: String,
    pub project_id: String,
    pub version_number: String,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub files: Vec<ProviderFileInfo>,
    pub dependencies: Vec<ProviderDependency>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub changelog: Option<String>,
    #[serde(default)]
    pub date_published: Option<String>,
    /// Modrinth release channel: `release`, `beta`, or `alpha`.
    #[serde(default)]
    pub version_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderFileInfo {
    pub url: String,
    pub filename: String,
    pub primary: bool,
    pub hashes: ProviderFileHashes,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderFileHashes {
    pub sha1: Option<String>,
    pub sha512: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDependency {
    pub project_id: Option<String>,
    pub version_id: Option<String>,
    pub dependency_type: String,
}

pub trait ContentProvider {
    fn search(&self, query: &ProviderSearchQuery) -> Result<SearchPage, ProviderError>;
    fn get_project(&self, id: &str) -> Result<ProjectInfo, ProviderError>;
    fn get_versions(
        &self,
        id: &str,
        query: &ProviderSearchQuery,
    ) -> Result<Vec<VersionInfo>, ProviderError>;
    fn get_version(&self, version_id: &str) -> Result<VersionInfo, ProviderError>;
    fn get_file(&self, version_id: &str, filename: &str)
        -> Result<ProviderFileInfo, ProviderError>;
    fn resolve_dependencies(
        &self,
        version_id: &str,
    ) -> Result<Vec<ModDependencySpec>, ProviderError>;
}

impl ProviderFileInfo {
    pub fn primary_file(version: &VersionInfo) -> Option<&ProviderFileInfo> {
        version
            .files
            .iter()
            .find(|f| f.primary)
            .or(version.files.first())
    }

    /// Picks the file that actually matches the target loader, mirroring how
    /// PrismLauncher's Modrinth integration (`ModIndex::getVersionFile`)
    /// selects a jar instead of blindly trusting the `primary` flag. Multi-loader
    /// mods often ship several jars in one version (e.g. a Forge jar
    /// and a Fabric jar); the `primary` one is not guaranteed to be the
    /// right loader. We first narrow to files whose name hints at the
    /// loader, then prefer the `primary` one among those, falling back to
    /// the plain primary file when nothing loader-specific matches.
    pub fn select_file_for_loader<'a>(
        version: &'a VersionInfo,
        loader: &str,
    ) -> Option<&'a ProviderFileInfo> {
        if version.files.is_empty() {
            return None;
        }

        // Check if this version actually supports the requested loader
        // (accounting for aliases like neoforge↔neo, paper↔bukkit).
        let version_matches_loader = version.loaders.iter().any(|vl| loaders_match(vl, loader));

        // A version declaring a single loader only has one kind of file.
        let single_loader = version.loaders.len() <= 1;
        let keyword = loader_keyword(loader);
        let mut candidates: Vec<&ProviderFileInfo> = version
            .files
            .iter()
            .filter(|f| {
                if single_loader {
                    return true;
                }
                // If the version explicitly lists the target loader (or an
                // alias), prefer files whose name matches the loader keyword.
                if version_matches_loader {
                    if let Some(kw) = &keyword {
                        return f.filename.to_lowercase().contains(kw);
                    }
                }
                // Loaders without a name keyword (e.g. vanilla) accept any file.
                true
            })
            .collect();
        if candidates.is_empty() {
            candidates = version.files.iter().collect();
        }
        let primary = candidates.iter().find(|f| f.primary).copied();
        primary.or_else(|| candidates.first().copied())
    }
}

/// Maps a loader slug to the substring a Modrinth filename typically
/// uses to mark that loader's artifact. `None` means "no specific
/// keyword" — accept any file (used for vanilla/resourcepack/datapack/shader).
fn loader_keyword(loader: &str) -> Option<&'static str> {
    match loader.to_lowercase().as_str() {
        "fabric" => Some("fabric"),
        "quilt" => Some("quilt"),
        "forge" => Some("forge"),
        "neoforge" => Some("neoforge"),
        _ => None,
    }
}

/// Returns loader aliases — alternative names that refer to the same
/// loader family.  Ported from `modrinth-content-management` so the
/// dependency resolver treats e.g. NeoForge ↔ Neo, or
/// Paper/Purpur/Spigot/Bukkit as compatible.
///
/// A loader with no known aliases returns `&[]`.
fn loader_aliases(loader: &str) -> &'static [&'static str] {
    match loader.to_lowercase().as_str() {
        "neoforge" => &["neo"],
        "neo" => &["neoforge"],
        "paper" | "purpur" | "spigot" | "bukkit" => &["paper", "purpur", "spigot", "bukkit"],
        _ => &[],
    }
}

/// Checks whether two loader strings refer to the same loader family,
/// accounting for aliases (e.g. `neoforge` == `neo`, `paper` == `bukkit`).
///
/// Comparison is case-insensitive.
pub fn loaders_match(expected: &str, candidate: &str) -> bool {
    let expected = expected.to_lowercase();
    let candidate = candidate.to_lowercase();

    expected == candidate
        || loader_aliases(&expected).contains(&candidate.as_str())
        || loader_aliases(&candidate).contains(&expected.as_str())
}

/// Returns the list of loaders that are considered equivalent to
/// `loader` (including itself).  Useful for building query parameters
/// that should cover all variants (e.g. searching Modrinth for a
/// project that lists `bukkit` when the user selected `paper`).
pub fn equivalent_loaders(loader: &str) -> Vec<String> {
    let lower = loader.to_lowercase();
    let mut result: Vec<String> = vec![lower.clone()];
    for alias in loader_aliases(&lower) {
        result.push(alias.to_string());
    }
    // Deduplicate (e.g. "paper" in both the input and the alias list).
    result.dedup();
    result
}

pub fn provider_dependency_to_spec(dep: ProviderDependency) -> Option<ModDependencySpec> {
    // Mirror modrinth-extras graph types: required/optional/embedded enter the
    // graph; incompatible becomes a conflict edge. Embedded is treated as
    // optional so bundled deps don't show up as hard MISSING_DEPENDENCY.
    let kind = match dep.dependency_type.as_str() {
        "required" => DependencyKind::Requires,
        "optional" | "embedded" => DependencyKind::Optional,
        "incompatible" => DependencyKind::Conflicts,
        _ => return None,
    };
    let target = dep.project_id?;
    Some(ModDependencySpec {
        kind,
        target,
        version_constraint: None,
        reason: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // loader_aliases / loaders_match
    // -----------------------------------------------------------------------

    #[test]
    fn loaders_match_exact() {
        assert!(loaders_match("fabric", "fabric"));
        assert!(loaders_match("forge", "forge"));
        assert!(loaders_match("neoforge", "neoforge"));
    }

    #[test]
    fn loaders_match_case_insensitive() {
        assert!(loaders_match("Fabric", "fabric"));
        assert!(loaders_match("FORGE", "forge"));
    }

    #[test]
    fn loaders_match_aliases() {
        assert!(loaders_match("neoforge", "neo"));
        assert!(loaders_match("neo", "neoforge"));
        // Bukkit-family are all equivalent.
        assert!(loaders_match("paper", "bukkit"));
        assert!(loaders_match("spigot", "purpur"));
        assert!(loaders_match("bukkit", "paper"));
    }

    #[test]
    fn loaders_match_different_families_are_not_equal() {
        assert!(!loaders_match("fabric", "forge"));
        assert!(!loaders_match("neoforge", "fabric"));
        assert!(!loaders_match("paper", "fabric"));
    }

    #[test]
    fn equivalent_loaders_includes_self() {
        let loaders = equivalent_loaders("paper");
        assert!(loaders.contains(&"paper".to_string()));
    }

    #[test]
    fn equivalent_loaders_includes_aliases() {
        let loaders = equivalent_loaders("paper");
        assert!(loaders.contains(&"bukkit".to_string()));
        assert!(loaders.contains(&"spigot".to_string()));
        assert!(loaders.contains(&"purpur".to_string()));
    }

    #[test]
    fn equivalent_loaders_no_aliases_for_unknown() {
        let loaders = equivalent_loaders("sodium");
        assert_eq!(loaders, vec!["sodium".to_string()]);
    }

    // -----------------------------------------------------------------------
    // loader_keyword
    // -----------------------------------------------------------------------

    #[test]
    fn provider_dependency_to_spec_maps_required() {
        let dep = ProviderDependency {
            project_id: Some("sodium".to_string()),
            version_id: None,
            dependency_type: "required".to_string(),
        };
        let spec = provider_dependency_to_spec(dep).unwrap();
        assert_eq!(spec.kind, DependencyKind::Requires);
        assert_eq!(spec.target, "sodium");
    }

    #[test]
    fn provider_dependency_to_spec_maps_embedded_as_optional() {
        let dep = ProviderDependency {
            project_id: Some("foo".to_string()),
            version_id: None,
            dependency_type: "embedded".to_string(),
        };
        let spec = provider_dependency_to_spec(dep).unwrap();
        assert_eq!(spec.kind, DependencyKind::Optional);
        assert_eq!(spec.target, "foo");
    }

    #[test]
    fn provider_dependency_to_spec_ignores_unknown() {
        let dep = ProviderDependency {
            project_id: Some("foo".to_string()),
            version_id: None,
            dependency_type: "unknown-kind".to_string(),
        };
        assert!(provider_dependency_to_spec(dep).is_none());
    }

    #[test]
    fn provider_dependency_to_spec_requires_project_id() {
        let dep = ProviderDependency {
            project_id: None,
            version_id: Some("v1".to_string()),
            dependency_type: "required".to_string(),
        };
        assert!(provider_dependency_to_spec(dep).is_none());
    }
}
