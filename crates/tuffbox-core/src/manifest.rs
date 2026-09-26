use serde::{Deserialize, Serialize};
use std::{fs, path::Path};
use thiserror::Error;

pub const CURRENT_PROJECT_SCHEMA_VERSION: &str = "0.1.0";
pub const SUPPORTED_PROJECT_SCHEMA_VERSIONS: &[&str] = &["0.1.0", "0.1"];

#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("failed to read manifest {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse manifest {path}: {source}")]
    Parse {
        path: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("unsupported project schema version {version}; supported versions: {supported}")]
    UnsupportedSchemaVersion { version: String, supported: String },
    #[error("project has no profiles")]
    NoProfiles,
    #[error("project has no client or both profile")]
    NoClientProfile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectManifest {
    pub schema_version: String,
    pub project: ProjectMetadata,
    pub minecraft: MinecraftSpec,
    pub loader: LoaderSpec,
    #[serde(default)]
    pub brief: Option<PackBrief>,
    #[serde(default)]
    pub listing: Option<ProjectListing>,
    #[serde(default)]
    pub java: Option<JavaSpec>,
    #[serde(default)]
    pub profiles: Vec<ProfileSpec>,
    #[serde(default)]
    pub mods: Vec<ModSpec>,
    #[serde(default)]
    pub overrides: Option<OverridesSpec>,
}

impl ProjectManifest {
    pub fn migrate_to_current_schema(&mut self) {
        self.schema_version = CURRENT_PROJECT_SCHEMA_VERSION.to_string();
    }

    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self, ManifestError> {
        let path_ref = path.as_ref();
        let path_string = path_ref.display().to_string();
        let raw = fs::read_to_string(path_ref).map_err(|source| ManifestError::Read {
            path: path_string.clone(),
            source,
        })?;
        let mut value: serde_json::Value =
            serde_json::from_str(&raw).map_err(|source| ManifestError::Parse {
                path: path_string.clone(),
                source,
            })?;
        migrate_project_manifest_value(&mut value)?;
        let manifest = serde_json::from_value(value).map_err(|source| ManifestError::Parse {
            path: path_string,
            source,
        })?;
        Ok(manifest)
    }

    pub fn validate_basic(&self) -> Result<(), ManifestError> {
        if self.profiles.is_empty() {
            return Err(ManifestError::NoProfiles);
        }

        let has_client = self
            .profiles
            .iter()
            .any(|profile| matches!(profile.side, Side::Client | Side::Both));

        if !has_client {
            return Err(ManifestError::NoClientProfile);
        }

        Ok(())
    }
}

pub fn migrate_project_manifest_value(value: &mut serde_json::Value) -> Result<(), ManifestError> {
    let version = value
        .get("schemaVersion")
        .and_then(|v| v.as_str())
        .unwrap_or("0.1")
        .to_string();

    let normalized = if version == "0.1" {
        "0.1.0".to_string()
    } else {
        version.clone()
    };

    if !SUPPORTED_PROJECT_SCHEMA_VERSIONS.contains(&normalized.as_str()) {
        return Err(ManifestError::UnsupportedSchemaVersion {
            version,
            supported: SUPPORTED_PROJECT_SCHEMA_VERSIONS.join(", "),
        });
    }

    if let Some(object) = value.as_object_mut() {
        object.insert(
            "schemaVersion".to_string(),
            serde_json::Value::String(CURRENT_PROJECT_SCHEMA_VERSION.to_string()),
        );
        object
            .entry("profiles".to_string())
            .or_insert_with(|| serde_json::Value::Array(Vec::new()));
        object
            .entry("mods".to_string())
            .or_insert_with(|| serde_json::Value::Array(Vec::new()));
    }

    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMetadata {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub authors: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackBrief {
    #[serde(default)]
    pub goal: String,
    #[serde(default)]
    pub target_audience: String,
    #[serde(default)]
    pub gameplay_pillars: Vec<String>,
    #[serde(default)]
    pub constraints: Vec<String>,
    #[serde(default)]
    pub release_targets: Vec<String>,
    #[serde(default)]
    pub notes: String,
}

/// Storefront listing (Modrinth/CurseForge card + long description).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListingGalleryItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectListing {
    #[serde(default)]
    pub name: String,
    /// Card blurb — Modrinth soft limit ~256 chars.
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub body_markdown: String,
    /// Project-relative path, e.g. `.tuffbox/listing/icon.png`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_path: Option<String>,
    #[serde(default)]
    pub gallery: Vec<ListingGalleryItem>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub authors: Vec<String>,
}

impl ProjectListing {
    pub const SUMMARY_SOFT_LIMIT: usize = 256;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinecraftSpec {
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoaderSpec {
    #[serde(rename = "type")]
    pub kind: LoaderKind,
    pub version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LoaderKind {
    Vanilla,
    Fabric,
    Forge,
    Neoforge,
    Quilt,
}

impl LoaderKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            LoaderKind::Vanilla => "vanilla",
            LoaderKind::Fabric => "fabric",
            LoaderKind::Forge => "forge",
            LoaderKind::Neoforge => "neoforge",
            LoaderKind::Quilt => "quilt",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JavaSpec {
    #[serde(default)]
    pub major: Option<u16>,
    #[serde(default)]
    pub distribution: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileSpec {
    pub id: String,
    pub name: String,
    pub side: Side,
    #[serde(default)]
    pub include_optional_mods: bool,
    #[serde(default)]
    pub include_shaders: bool,
    #[serde(default)]
    pub memory_mb: Option<u32>,
    #[serde(default)]
    pub jvm_args: Vec<String>,
    #[serde(default)]
    pub include_mods: Vec<String>,
    /// Offline/test-launch player name. Defaults to "Player" if unset.
    /// Used to derive a stable offline UUID so the same name always maps
    /// to the same in-game identity across test runs (matching vanilla
    /// offline-mode behavior), instead of every launch using a fixed
    /// all-zero UUID for every player.
    #[serde(default)]
    pub player_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Client,
    Server,
    Both,
    Optional,
    Unknown,
}

impl Side {
    pub fn as_str(&self) -> &'static str {
        match self {
            Side::Client => "client",
            Side::Server => "server",
            Side::Both => "both",
            Side::Optional => "optional",
            Side::Unknown => "unknown",
        }
    }

    /// Maps Modrinth `client_side` / `server_side` environment strings to [`Side`].
    ///
    /// Modrinth values are `required`, `optional`, `unsupported`, or `unknown`.
    pub fn from_modrinth(client_side: Option<&str>, server_side: Option<&str>) -> Self {
        let client = client_side.unwrap_or("unknown");
        let server = server_side.unwrap_or("unknown");
        match (client, server) {
            ("required" | "optional", "unsupported") => Side::Client,
            ("unsupported", "required" | "optional") => Side::Server,
            ("required" | "optional", "required" | "optional") => Side::Both,
            _ => Side::Unknown,
        }
    }

    pub fn is_compatible_with_profile(self, profile_side: Side) -> bool {
        match (self, profile_side) {
            (Side::Both, _) => true,
            (_, Side::Both) => true,
            (Side::Client, Side::Client) => true,
            (Side::Server, Side::Server) => true,
            (Side::Optional, _) => true,
            (Side::Unknown, _) => false,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModSpec {
    pub id: String,
    pub name: String,
    pub source: ModSource,
    pub version: String,
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub hashes: Option<FileHashes>,
    pub side: Side,
    #[serde(default)]
    pub dependencies: Vec<ModDependencySpec>,
    #[serde(default)]
    pub status: Vec<String>,
    /// What kind of content this entry is (mod jar, resourcepack, datapack,
    /// shaderpack). Defaults to `Mod` for backward compatibility with
    /// manifests written before this field existed.
    ///
    /// This matters because each content type lives in a different folder
    /// inside the instance (`mods/`, `resourcepacks/`, `shaderpacks/`,
    /// `datapacks/` under a world save) — treating everything as a mod jar
    /// meant resourcepacks/shaders installed from Modrinth were written
    /// into `mods/`, where loaders either ignore them or, worse, try to
    /// load them as a mod and fail.
    #[serde(default)]
    pub content_type: ContentType,
    /// Mod authors / creators (Modrinth/CF / jar metadata). Empty when unknown.
    #[serde(default)]
    pub authors: Vec<String>,
    /// Packwiz-style optional-mod metadata (`option{}` in `.pw.toml`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub option: Option<ModOption>,
}

/// Optional-mod presentation / default-enabled flag (packwiz `option` table).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModOption {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// When `true`, optional mod is enabled by default on install.
    #[serde(default = "default_true")]
    pub default: bool,
}

fn default_true() -> bool {
    true
}

impl ModSpec {
    /// A mod is "pinned" (frozen against updates, mirroring packwiz's per-mod
    /// `pin` flag in `.pw.toml`) when its `status` list carries `"pinned"`.
    /// The IDE adds/removes that token through the config editor.
    pub fn pinned(&self) -> bool {
        self.status.iter().any(|s| s.eq_ignore_ascii_case("pinned"))
    }

    /// Whether this mod should be installed for the given profile.
    ///
    /// Optional mods are skipped when `profile.include_optional_mods` is false,
    /// unless the mod id is explicitly listed in `profile.include_mods`.
    pub fn included_in_profile(&self, profile: &ProfileSpec) -> bool {
        if !self.side.is_compatible_with_profile(profile.side) {
            return false;
        }
        if self.side == Side::Optional && !profile.include_optional_mods {
            return profile.include_mods.iter().any(|id| id == &self.id);
        }
        if self.side == Side::Optional {
            // Respect packwiz `option.default = false` when the profile includes
            // optionals generically (no whitelist): skip until explicitly listed.
            if let Some(opt) = &self.option {
                if !opt.default && profile.include_mods.is_empty() {
                    return false;
                }
            }
        }
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ContentType {
    #[default]
    Mod,
    Resourcepack,
    Shaderpack,
    Datapack,
}

impl ContentType {
    /// Parses a Modrinth `project_type` string (`mod`, `resourcepack`,
    /// `shader`, `datapack`, ...) into a [`ContentType`]. Unknown/loader
    /// project types (`modpack`, `plugin`) fall back to `Mod` since they're
    /// still delivered as a jar the loader should see.
    pub fn from_modrinth_project_type(project_type: &str) -> Self {
        match project_type {
            "resourcepack" => ContentType::Resourcepack,
            "shader" => ContentType::Shaderpack,
            "datapack" => ContentType::Datapack,
            _ => ContentType::Mod,
        }
    }

    /// Guess content type from a download / file name when folder or provider
    /// metadata is missing (packwiz metafiles, local zip drops, etc.).
    ///
    /// Recognizes player-built VanillaTweaks packs from vanillatweaks.net
    /// (`VanillaTweaks_r….zip` / any `VanillaTweaks_….zip`) as resource packs.
    pub fn from_filename(name: &str) -> Self {
        let lower = name.to_ascii_lowercase();
        // Extract the basename manually (split on both separators) so a full
        // Windows path like `C:\Downloads\foo.zip` is detected the same on any
        // host OS — Path::file_name() only splits on '\' when built for Windows.
        let base = lower.rsplit(['/', '\\']).next().unwrap_or(lower.as_str());
        if base.starts_with("vanillatweaks_")
            || lower.contains("resourcepack")
            || (lower.ends_with(".zip") && lower.contains("resource"))
        {
            ContentType::Resourcepack
        } else if lower.contains("shader") {
            ContentType::Shaderpack
        } else if lower.contains("datapack") {
            ContentType::Datapack
        } else {
            ContentType::Mod
        }
    }

    /// The folder this content type lives in, relative to the instance
    /// root (or, for datapacks, relative to a world save directory).
    pub fn folder_name(self) -> &'static str {
        match self {
            ContentType::Mod => "mods",
            ContentType::Resourcepack => "resourcepacks",
            ContentType::Shaderpack => "shaderpacks",
            ContentType::Datapack => "datapacks",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModSource {
    #[serde(rename = "type")]
    pub kind: SourceKind,
    #[serde(default, rename = "projectId")]
    pub project_id: Option<String>,
    #[serde(default, rename = "fileId")]
    pub file_id: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    /// Cached Modrinth icon URL (only meaningful for Modrinth-sourced mods).
    /// Lets the UI show the real icon without an extra network round-trip per
    /// mod on every list load.
    #[serde(default, rename = "iconUrl")]
    pub icon_url: Option<String>,
    /// Cached provider categories/tags (e.g. Modrinth `optimization`,
    /// `worldgen`, `magic`). Used to cluster the dependency graph by the real
    /// site taxonomy instead of fragile keyword guessing.
    #[serde(default)]
    pub categories: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Modrinth,
    Curseforge,
    Github,
    Local,
    Direct,
}

impl SourceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SourceKind::Modrinth => "modrinth",
            SourceKind::Curseforge => "curseforge",
            SourceKind::Github => "github",
            SourceKind::Local => "local",
            SourceKind::Direct => "direct",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileHashes {
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub sha512: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModDependencySpec {
    #[serde(rename = "type")]
    pub kind: DependencyKind,
    pub target: String,
    #[serde(default, rename = "versionConstraint")]
    pub version_constraint: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyKind {
    Requires,
    Optional,
    Conflicts,
    BreaksWith,
    Replaces,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverridesSpec {
    #[serde(default)]
    pub config: Option<String>,
    #[serde(default)]
    pub kubejs: Option<String>,
    #[serde(default)]
    pub resourcepacks: Option<String>,
    #[serde(default)]
    pub shaderpacks: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{ContentType, Side};

    #[test]
    fn modrinth_client_only_is_client() {
        assert_eq!(
            Side::from_modrinth(Some("required"), Some("unsupported")),
            Side::Client
        );
        assert_eq!(
            Side::from_modrinth(Some("optional"), Some("unsupported")),
            Side::Client
        );
    }

    #[test]
    fn modrinth_server_only_is_server() {
        assert_eq!(
            Side::from_modrinth(Some("unsupported"), Some("required")),
            Side::Server
        );
    }

    #[test]
    fn modrinth_both_required_is_both() {
        assert_eq!(
            Side::from_modrinth(Some("required"), Some("required")),
            Side::Both
        );
    }

    #[test]
    fn modrinth_missing_is_unknown() {
        assert_eq!(Side::from_modrinth(None, None), Side::Unknown);
    }

    #[test]
    fn filename_detects_vanillatweaks_custom_resource_pack() {
        assert_eq!(
            ContentType::from_filename("VanillaTweaks_r123456.zip"),
            ContentType::Resourcepack
        );
        assert_eq!(
            ContentType::from_filename("vanillatweaks_abcDEF.zip"),
            ContentType::Resourcepack
        );
        assert_eq!(
            ContentType::from_filename(r"C:\Downloads\VanillaTweaks_r999.zip"),
            ContentType::Resourcepack
        );
        // Unrelated zip stays a mod hint (folder/metadata decides later).
        assert_eq!(
            ContentType::from_filename("cool-shaders.zip"),
            ContentType::Shaderpack
        );
        assert_eq!(ContentType::from_filename("sodium.jar"), ContentType::Mod);
    }
}
