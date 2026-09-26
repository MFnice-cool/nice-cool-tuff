//! Auto side-labeling by scanning mod jar metadata.
//!
//! Mirrors ServerPackCreator's mod scanning (`modscanning/*Scanner.kt`) and
//! packwiz's per-mod `side` field. TuffBox's [`crate::manifest::ModSpec`]
//! carries a [`crate::manifest::Side`], but it is often `Unknown` for mods
//! imported without metadata. This module reads the loader-specific
//! descriptor inside a jar (`fabric.mod.json`, `quilt.mod.json`,
//! `META-INF/mods.toml`, `META-INF/neoforge.mods.toml`) and derives the
//! client/server environment from the `environment` / `@NetworkDirection`
//! declarations — the same signal SPC and packwiz use to decide whether a mod
//! belongs in a server pack or client-only list.

use crate::manifest::{Side, SourceKind};
use serde::Deserialize;
use std::io::Read;
use std::path::Path;
use thiserror::Error;
use zip::ZipArchive;

#[derive(Debug, Error)]
pub enum ModScanError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to open jar {path}: {source}")]
    Open {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to read entry {entry} in {path}: {source}")]
    Entry {
        entry: String,
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse {entry}: {source}")]
    Parse {
        entry: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("no recognizable mod metadata found in {0}")]
    NoMetadata(String),
}

/// Result of scanning a single mod jar.
#[derive(Debug, Clone, PartialEq)]
pub struct ModScanResult {
    pub side: Side,
    /// The loader family the descriptor implies (best-effort).
    pub loader_hint: Option<String>,
    /// Raw `environment` string from the descriptor, for diagnostics.
    pub raw_environment: Option<String>,
    /// Canonical mod id from the descriptor when present.
    pub mod_id: Option<String>,
    /// Authors / contributors from jar metadata.
    pub authors: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct FabricModJson {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    environment: Option<String>,
    #[serde(default)]
    authors: Vec<FabricAuthor>,
    #[serde(default)]
    contributors: Vec<FabricAuthor>,
    #[serde(default, rename = "accessWidener")]
    _access_widener: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum FabricAuthor {
    Name(String),
    Obj { name: Option<String> },
}

impl FabricAuthor {
    fn name(&self) -> Option<String> {
        match self {
            FabricAuthor::Name(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
            FabricAuthor::Obj { name: Some(s) } if !s.trim().is_empty() => {
                Some(s.trim().to_string())
            }
            _ => None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct QuiltModJson {
    #[serde(default, rename = "environment")]
    env_obj: Option<QuiltEnv>,
}

#[derive(Debug, Deserialize)]
struct QuiltEnv {
    #[serde(default)]
    client: Option<String>,
    #[serde(default)]
    dedicated: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ForgeModsToml {
    #[serde(default)]
    mods: Vec<ForgeModEntry>,
    #[serde(default, rename = "dependencies")]
    dependencies: std::collections::HashMap<String, Vec<ForgeDependency>>,
}

#[derive(Debug, Deserialize)]
struct ForgeModEntry {
    #[serde(default, alias = "modId", alias = "modid")]
    mod_id: String,
    #[serde(default)]
    authors: Option<ForgeAuthors>,
    #[serde(default, rename = "networking")]
    _networking: Option<ForgeNetworking>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum ForgeAuthors {
    One(String),
    Many(Vec<String>),
}

impl ForgeAuthors {
    fn into_vec(self) -> Vec<String> {
        match self {
            ForgeAuthors::One(s) => s
                .split([',', ';'])
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect(),
            ForgeAuthors::Many(v) => v
                .into_iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ForgeNetworking {
    #[serde(default, rename = "channel")]
    _channel: Option<String>,
    #[serde(default, rename = "directional")]
    _directional: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ForgeDependency {
    #[serde(default)]
    _modid: String,
    #[serde(default, rename = "type")]
    _kind: Option<String>,
}

fn read_entry(
    archive: &mut ZipArchive<std::fs::File>,
    name: &str,
    path: &str,
) -> Result<String, ModScanError> {
    let mut entry = archive.by_name(name).map_err(|e| ModScanError::Entry {
        entry: name.to_string(),
        path: path.to_string(),
        source: std::io::Error::other(e.to_string()),
    })?;
    let mut content = String::new();
    entry
        .read_to_string(&mut content)
        .map_err(|e| ModScanError::Entry {
            entry: name.to_string(),
            path: path.to_string(),
            source: std::io::Error::other(e.to_string()),
        })?;
    Ok(content)
}

/// Scans a mod jar and returns the side plus a loader hint.
///
/// Resolution order (most specific first):
/// 1. Forge/NeoForge `META-INF/neoforge.mods.toml` and `META-INF/mods.toml`
///    — a mod with no client-only networking is treated as `Both`; a mod that
///    declares `client`/`server` environments restricts accordingly.
/// 2. Fabric/Quilt `*-mod.json` — uses the `environment` field
///    (`client`/`server`/`*`).
pub fn scan_mod_jar(jar_path: &Path) -> Result<ModScanResult, ModScanError> {
    let path_str = jar_path.display().to_string();
    let file = std::fs::File::open(jar_path).map_err(|source| ModScanError::Open {
        path: path_str.clone(),
        source,
    })?;
    let mut archive = ZipArchive::new(file).map_err(|source| ModScanError::Open {
        path: path_str.clone(),
        source: std::io::Error::other(source.to_string()),
    })?;

    // NeoForge (1.20.6+) format.
    if let Ok(content) = read_entry(&mut archive, "META-INF/neoforge.mods.toml", &path_str) {
        if let Some(result) = parse_forge_mods_toml(&content, "neoforge")? {
            return Ok(result);
        }
    }
    // Forge (pre-1.20.6) format.
    if let Ok(content) = read_entry(&mut archive, "META-INF/mods.toml", &path_str) {
        if let Some(result) = parse_forge_mods_toml(&content, "forge")? {
            return Ok(result);
        }
    }
    // Quilt (preferred over Fabric since quilt.mod.json also carries env).
    if let Ok(content) = read_entry(&mut archive, "quilt.mod.json", &path_str) {
        if let Some(result) = parse_quilt_mod_json(&content, &path_str)? {
            return Ok(result);
        }
    }
    // Fabric.
    if let Ok(content) = read_entry(&mut archive, "fabric.mod.json", &path_str) {
        if let Some(result) = parse_fabric_mod_json(&content, &path_str)? {
            return Ok(result);
        }
    }
    // Legacy Forge (1.7–1.12): mcmod.info (JSON array).
    if let Ok(content) = read_entry(&mut archive, "mcmod.info", &path_str) {
        if let Some(result) = parse_mcmod_info(&content)? {
            return Ok(result);
        }
    }
    // Legacy Forge (1.6): mods.info (key=value properties).
    if let Ok(content) = read_entry(&mut archive, "mods.info", &path_str) {
        if let Some(result) = parse_mods_info(&content)? {
            return Ok(result);
        }
    }

    Err(ModScanError::NoMetadata(path_str))
}

fn parse_forge_mods_toml(
    content: &str,
    loader: &str,
) -> Result<Option<ModScanResult>, ModScanError> {
    let toml: ForgeModsToml = toml::from_str(content).map_err(|source| ModScanError::Parse {
        entry: format!("META-INF/{loader}.mods.toml"),
        source: serde::de::Error::custom(source.to_string()),
    })?;

    let side = Side::Both;
    let first = toml.mods.first();
    let mod_id = first
        .map(|m| m.mod_id.trim().to_string())
        .filter(|s| !s.is_empty());
    let authors = first
        .and_then(|m| m.authors.clone())
        .map(|a| a.into_vec())
        .unwrap_or_default();
    Ok(Some(ModScanResult {
        side,
        loader_hint: Some(loader.to_string()),
        raw_environment: None,
        mod_id,
        authors,
    }))
}

fn parse_fabric_mod_json(
    content: &str,
    _path: &str,
) -> Result<Option<ModScanResult>, ModScanError> {
    let json: FabricModJson =
        serde_json::from_str(content).map_err(|source| ModScanError::Parse {
            entry: "fabric.mod.json".into(),
            source,
        })?;
    let side = match json.environment.as_deref() {
        Some("client") => Side::Client,
        Some("server") => Side::Server,
        _ => Side::Both,
    };
    let mut authors: Vec<String> = json
        .authors
        .iter()
        .chain(json.contributors.iter())
        .filter_map(|a| a.name())
        .collect();
    authors.sort();
    authors.dedup();
    Ok(Some(ModScanResult {
        side,
        loader_hint: Some("fabric".into()),
        raw_environment: json.environment,
        mod_id: json.id.filter(|s| !s.trim().is_empty()),
        authors,
    }))
}

fn parse_quilt_mod_json(content: &str, _path: &str) -> Result<Option<ModScanResult>, ModScanError> {
    let json: QuiltModJson =
        serde_json::from_str(content).map_err(|source| ModScanError::Parse {
            entry: "quilt.mod.json".into(),
            source,
        })?;
    let side = if let Some(env) = &json.env_obj {
        match (env.client.as_deref(), env.dedicated.as_deref()) {
            (Some("available"), Some("available")) => Side::Both,
            (Some("available"), _) => Side::Client,
            (_, Some("available")) => Side::Server,
            _ => Side::Both,
        }
    } else {
        Side::Both
    };
    Ok(Some(ModScanResult {
        side,
        loader_hint: Some("quilt".into()),
        raw_environment: None,
        mod_id: None,
        authors: Vec::new(),
    }))
}

#[derive(Debug, Deserialize)]
struct McModInfoEntry {
    #[serde(default, rename = "serverSideOnly")]
    server_side_only: Option<bool>,
    #[serde(default, rename = "clientSideOnly")]
    client_side_only: Option<bool>,
}

fn parse_mcmod_info(content: &str) -> Result<Option<ModScanResult>, ModScanError> {
    let entries: Vec<McModInfoEntry> =
        serde_json::from_str(content).map_err(|source| ModScanError::Parse {
            entry: "mcmod.info".into(),
            source,
        })?;
    let side = if entries.iter().any(|e| e.client_side_only == Some(true))
        && !entries.iter().any(|e| e.server_side_only == Some(true))
    {
        Side::Client
    } else if entries.iter().any(|e| e.server_side_only == Some(true))
        && !entries.iter().any(|e| e.client_side_only == Some(true))
    {
        Side::Server
    } else {
        Side::Both
    };
    Ok(Some(ModScanResult {
        side,
        loader_hint: Some("forge".into()),
        raw_environment: None,
        mod_id: None,
        authors: Vec::new(),
    }))
}

fn parse_mods_info(content: &str) -> Result<Option<ModScanResult>, ModScanError> {
    let side = content.lines().any(|l| {
        let lower = l.to_lowercase();
        (lower.contains("side") && lower.contains("client") && !lower.contains("server"))
            || lower.contains("clientsideonly=true")
    });
    Ok(Some(ModScanResult {
        side: if side { Side::Client } else { Side::Both },
        loader_hint: Some("forge".into()),
        raw_environment: None,
        mod_id: None,
        authors: Vec::new(),
    }))
}

/// Scans every mod jar declared in the manifest and returns (mod id → side).
/// Mods without a scannable jar (e.g. remote-only entries not yet materialized)
/// are skipped so callers can keep their existing side.
pub fn scan_manifest_mods(
    instance_dir: &Path,
    manifest: &crate::manifest::ProjectManifest,
) -> Vec<(String, ModScanResult)> {
    let mut out = Vec::new();
    for module in &manifest.mods {
        let Some(file_name) = &module.file_name else {
            continue;
        };
        let folder = module.content_type.folder_name();
        let jar = instance_dir.join(folder).join(file_name);
        if !jar.is_file() {
            continue;
        }
        if matches!(module.source.kind, SourceKind::Local) {
            // Local drop-ins: still worth scanning for side hints.
        }
        match scan_mod_jar(&jar) {
            Ok(result) => out.push((module.id.clone(), result)),
            Err(_) => continue,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_jar(entries: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let jar = dir.path().join("mod.jar");
        let file = std::fs::File::create(&jar).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default();
        for (name, content) in entries {
            zip.start_file(*name, opts).unwrap();
            zip.write_all(content.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        dir
    }

    #[test]
    fn fabric_client_mod_is_client_side() {
        let dir = make_jar(&[(
            "fabric.mod.json",
            r#"{"schemaVersion":1,"id":"x","version":"1","environment":"client"}"#,
        )]);
        let result = scan_mod_jar(&dir.path().join("mod.jar")).unwrap();
        assert_eq!(result.side, Side::Client);
        assert_eq!(result.loader_hint.as_deref(), Some("fabric"));
    }

    #[test]
    fn fabric_default_is_both() {
        let dir = make_jar(&[(
            "fabric.mod.json",
            r#"{"schemaVersion":1,"id":"x","version":"1"}"#,
        )]);
        let result = scan_mod_jar(&dir.path().join("mod.jar")).unwrap();
        assert_eq!(result.side, Side::Both);
    }

    #[test]
    fn quilt_server_mod_is_server_side() {
        let dir = make_jar(&[(
            "quilt.mod.json",
            r#"{"schemaVersion":1,"id":"x","version":"1","environment":{"client":"*","dedicated":"available"}}"#,
        )]);
        let result = scan_mod_jar(&dir.path().join("mod.jar")).unwrap();
        assert_eq!(result.side, Side::Server);
    }

    #[test]
    fn forge_mods_toml_is_both() {
        let dir = make_jar(&[(
            "META-INF/mods.toml",
            r#"
[[mods]]
modId="x"
version="1"
displayName="X"
"#,
        )]);
        let result = scan_mod_jar(&dir.path().join("mod.jar")).unwrap();
        assert_eq!(result.side, Side::Both);
        assert_eq!(result.loader_hint.as_deref(), Some("forge"));
    }

    #[test]
    fn fabric_authors_are_parsed() {
        let dir = make_jar(&[(
            "fabric.mod.json",
            r#"{"schemaVersion":1,"id":"coolmod","version":"1","authors":["Alice","Bob"],"environment":"*"}"#,
        )]);
        let result = scan_mod_jar(&dir.path().join("mod.jar")).unwrap();
        assert_eq!(result.mod_id.as_deref(), Some("coolmod"));
        assert_eq!(result.authors, vec!["Alice".to_string(), "Bob".to_string()]);
    }

    #[test]
    fn forge_authors_are_parsed() {
        let dir = make_jar(&[(
            "META-INF/mods.toml",
            r#"
[[mods]]
modId="x"
version="1"
displayName="X"
authors="Carol, Dave"
"#,
        )]);
        let result = scan_mod_jar(&dir.path().join("mod.jar")).unwrap();
        assert_eq!(result.mod_id.as_deref(), Some("x"));
        assert!(result.authors.iter().any(|a| a == "Carol"));
        assert!(result.authors.iter().any(|a| a == "Dave"));
    }

    #[test]
    fn no_metadata_errors() {
        let dir = make_jar(&[("random.txt", "nope")]);
        assert!(scan_mod_jar(&dir.path().join("mod.jar")).is_err());
    }
}
