pub use crate::manifest::LoaderKind;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct McVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl McVersion {
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split('.').collect();
        match parts.len() {
            2 => Some(Self::new(parts[0].parse().ok()?, parts[1].parse().ok()?, 0)),
            3 => Some(Self::new(
                parts[0].parse().ok()?,
                parts[1].parse().ok()?,
                parts[2].parse().ok()?,
            )),
            _ => None,
        }
    }

    pub fn data_epoch(&self) -> DataEpoch {
        match (self.major, self.minor) {
            (1, 0..=12) => DataEpoch::Legacy,
            (1, 13..=15) => DataEpoch::EarlyDataPack,
            (1, 16..=20) => DataEpoch::ModernDataPack,
            (1, 21..) => DataEpoch::Components,
            _ => DataEpoch::Unknown,
        }
    }

    pub fn tag_namespace(&self) -> TagNamespace {
        match (self.major, self.minor) {
            (1, 0..=15) => TagNamespace::Forge,
            (1, 16..=20) => TagNamespace::Mixed,
            (1, 21..) => TagNamespace::Common,
            _ => TagNamespace::Common,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataEpoch {
    Legacy,
    EarlyDataPack,
    ModernDataPack,
    Components,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagNamespace {
    Forge,
    Mixed,
    Common,
}

#[derive(Debug, Clone)]
pub struct ModpackEnvironment {
    pub mc_version: McVersion,
    pub loader: LoaderKind,
    pub loader_version: Option<String>,
    pub root_path: PathBuf,
    pub data_epoch: DataEpoch,
    pub tag_namespace: TagNamespace,
}

pub struct EnvironmentDetector;

impl EnvironmentDetector {
    pub fn detect(modpack_root: &Path) -> Result<ModpackEnvironment, DetectError> {
        let loader = Self::detect_loader(modpack_root)?;
        let (mc_version, loader_version) = Self::detect_versions(modpack_root, loader)?;
        let data_epoch = mc_version.data_epoch();
        let tag_namespace = Self::resolve_tag_namespace(&mc_version, loader);

        Ok(ModpackEnvironment {
            mc_version,
            loader,
            loader_version,
            root_path: modpack_root.to_path_buf(),
            data_epoch,
            tag_namespace,
        })
    }

    fn detect_loader(root: &Path) -> Result<LoaderKind, DetectError> {
        let mods_dir = root.join("mods");
        if mods_dir.exists() {
            if let Some(loader) = Self::detect_loader_from_mods(&mods_dir)? {
                return Ok(loader);
            }
        }

        if root.join("config/fabric").exists() || root.join("fabric.mod.json").exists() {
            return Ok(LoaderKind::Fabric);
        }

        if root.join("config/quilt").exists() {
            return Ok(LoaderKind::Quilt);
        }

        if root.join("config/fml.toml").exists() {
            if let Ok(entries) = std::fs::read_dir(&mods_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().map(|e| e == "jar").unwrap_or(false) {
                        if Self::jar_has_neoforge_marker(&path).unwrap_or(false) {
                            return Ok(LoaderKind::Neoforge);
                        }
                    }
                }
            }
            return Ok(LoaderKind::Forge);
        }

        Err(DetectError::UnknownLoader)
    }

    fn detect_loader_from_mods(mods_dir: &Path) -> Result<Option<LoaderKind>, DetectError> {
        for entry in std::fs::read_dir(mods_dir)?.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "jar").unwrap_or(false) {
                if let Ok(file) = std::fs::File::open(&path) {
                    if let Ok(mut archive) = zip::ZipArchive::new(file) {
                        if archive.by_name("fabric.mod.json").is_ok() {
                            return Ok(Some(LoaderKind::Fabric));
                        }
                        if archive.by_name("quilt.mod.json").is_ok() {
                            return Ok(Some(LoaderKind::Quilt));
                        }
                        if archive.by_name("META-INF/neoforge.mods.toml").is_ok() {
                            return Ok(Some(LoaderKind::Neoforge));
                        }
                        if archive.by_name("META-INF/mods.toml").is_ok() {
                            return Ok(Some(LoaderKind::Forge));
                        }
                        if archive.by_name("mcmod.info").is_ok() {
                            return Ok(Some(LoaderKind::Forge));
                        }
                    }
                }
            }
        }
        Ok(None)
    }

    fn jar_has_neoforge_marker(jar_path: &Path) -> Result<bool, DetectError> {
        let file = std::fs::File::open(jar_path)?;
        let archive = zip::ZipArchive::new(file)?;
        let has_marker = archive
            .file_names()
            .any(|name| name == "META-INF/neoforge.mods.toml");
        Ok(has_marker)
    }

    fn detect_versions(
        root: &Path,
        loader: LoaderKind,
    ) -> Result<(McVersion, Option<String>), DetectError> {
        let manifest = root.join("manifest.json");
        if manifest.exists() {
            let content = std::fs::read_to_string(&manifest)?;
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(mc) = json.get("minecraft") {
                    if let Some(version) = mc.get("version").and_then(|v| v.as_str()) {
                        let mc_ver = McVersion::parse(version).ok_or(DetectError::VersionParse)?;
                        let loader_ver = mc
                            .get("modLoaders")
                            .and_then(|v| v.as_array())
                            .and_then(|arr| arr.first())
                            .and_then(|v| v.get("id"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        return Ok((mc_ver, loader_ver));
                    }
                }
            }
        }

        let modrinth = root.join("modrinth.index.json");
        if modrinth.exists() {
            let content = std::fs::read_to_string(&modrinth)?;
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(deps) = json.get("dependencies").and_then(|v| v.as_object()) {
                    if let Some(mc_ver_str) = deps.get("minecraft").and_then(|v| v.as_str()) {
                        let mc_ver =
                            McVersion::parse(mc_ver_str).ok_or(DetectError::VersionParse)?;
                        let loader_key = match loader {
                            LoaderKind::Fabric => "fabric-loader",
                            LoaderKind::Quilt => "quilt-loader",
                            LoaderKind::Forge => "forge",
                            LoaderKind::Neoforge => "neoforge",
                            LoaderKind::Vanilla => "forge",
                        };
                        let loader_ver = deps
                            .get(loader_key)
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        return Ok((mc_ver, loader_ver));
                    }
                }
            }
        }

        Err(DetectError::VersionNotFound)
    }

    fn resolve_tag_namespace(version: &McVersion, loader: LoaderKind) -> TagNamespace {
        match (version.minor, loader) {
            (21.., _) => TagNamespace::Common,
            (_, LoaderKind::Fabric | LoaderKind::Quilt) => TagNamespace::Common,
            (16..=20, LoaderKind::Forge) => TagNamespace::Forge,
            (16..=20, LoaderKind::Neoforge) => TagNamespace::Common,
            _ => TagNamespace::Forge,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DetectError {
    #[error("IO: {0}")]
    Io(#[from] std::io::Error),
    #[error("ZIP: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("Unknown loader")]
    UnknownLoader,
    #[error("Version not found")]
    VersionNotFound,
    #[error("Version parse error")]
    VersionParse,
}
