//! Persistent Modrinth hash → project index for local jar identification.
//!
//! First `sync_mods_folder` / hash lookup writes an entry under
//! `.tuffbox/mod-hash-index.json`. Later loads reuse it instead of calling
//! Modrinth again. Entries are removed when the user deletes the mod.

use crate::manifest::{ContentType, FileHashes, ModSource, ModSpec, Side, SourceKind};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const CACHE_REL: &str = ".tuffbox/mod-hash-index.json";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModHashIndex {
    #[serde(default)]
    pub entries: HashMap<String, ModHashIndexEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModHashIndexEntry {
    /// `"modrinth"` when identified, `"miss"` when Modrinth has no match.
    pub status: String,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub file_id: Option<String>,
    #[serde(default)]
    pub icon_url: Option<String>,
    #[serde(default)]
    pub download_url: Option<String>,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub sha512: Option<String>,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub side: Option<String>,
    /// Raw Modrinth `client_side` value (`required` / `optional` / `unsupported`).
    /// Present only after a real Modrinth project lookup — used to avoid
    /// re-querying and to distinguish "defaulted to both" from a real both.
    #[serde(default)]
    pub client_side: Option<String>,
    /// Raw Modrinth `server_side` value.
    #[serde(default)]
    pub server_side: Option<String>,
}

impl ModHashIndex {
    pub fn path_for_instance(instance_dir: &Path) -> PathBuf {
        instance_dir.join(CACHE_REL)
    }

    pub fn load(instance_dir: &Path) -> Self {
        let path = Self::path_for_instance(instance_dir);
        let Ok(raw) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        serde_json::from_str(&raw).unwrap_or_default()
    }

    pub fn save(&self, instance_dir: &Path) -> std::io::Result<()> {
        let path = Self::path_for_instance(instance_dir);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let raw = serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into());
        std::fs::write(path, raw)
    }

    pub fn get(&self, sha1: &str) -> Option<&ModHashIndexEntry> {
        let key = sha1.to_ascii_lowercase();
        self.entries.get(&key).or_else(|| self.entries.get(sha1))
    }

    pub fn put_miss(&mut self, sha1: &str) {
        let key = sha1.to_ascii_lowercase();
        self.entries.insert(
            key,
            ModHashIndexEntry {
                status: "miss".into(),
                id: None,
                name: None,
                version: None,
                project_id: None,
                file_id: None,
                icon_url: None,
                download_url: None,
                sha1: Some(sha1.to_ascii_lowercase()),
                sha512: None,
                content_type: None,
                side: None,
                client_side: None,
                server_side: None,
            },
        );
    }

    pub fn put_modrinth(
        &mut self,
        sha1: &str,
        spec: &ModSpec,
        client_side: Option<&str>,
        server_side: Option<&str>,
    ) {
        let key = sha1.to_ascii_lowercase();
        let side = if client_side.is_some() || server_side.is_some() {
            Side::from_modrinth(client_side, server_side)
        } else {
            spec.side
        };
        self.entries.insert(
            key,
            ModHashIndexEntry {
                status: "modrinth".into(),
                id: Some(spec.id.clone()),
                name: Some(spec.name.clone()),
                version: Some(spec.version.clone()),
                project_id: spec.source.project_id.clone(),
                file_id: spec.source.file_id.clone(),
                icon_url: spec.source.icon_url.clone(),
                download_url: spec.source.url.clone(),
                sha1: Some(sha1.to_ascii_lowercase()),
                sha512: spec.hashes.as_ref().and_then(|h| h.sha512.clone()),
                content_type: Some(match spec.content_type {
                    ContentType::Mod => "mod".into(),
                    ContentType::Resourcepack => "resourcepack".into(),
                    ContentType::Shaderpack => "shader".into(),
                    ContentType::Datapack => "datapack".into(),
                }),
                side: Some(side.as_str().to_string()),
                client_side: client_side.map(|s| s.to_string()),
                server_side: server_side.map(|s| s.to_string()),
            },
        );
    }

    /// Updates only Modrinth environment fields on an existing (or new stub) entry.
    pub fn put_sides(&mut self, sha1: &str, client_side: Option<&str>, server_side: Option<&str>) {
        let key = sha1.to_ascii_lowercase();
        let side = Side::from_modrinth(client_side, server_side);
        let entry = self
            .entries
            .entry(key.clone())
            .or_insert_with(|| ModHashIndexEntry {
                status: "modrinth".into(),
                id: None,
                name: None,
                version: None,
                project_id: None,
                file_id: None,
                icon_url: None,
                download_url: None,
                sha1: Some(key),
                sha512: None,
                content_type: None,
                side: None,
                client_side: None,
                server_side: None,
            });
        entry.client_side = client_side.map(|s| s.to_string());
        entry.server_side = server_side.map(|s| s.to_string());
        entry.side = Some(side.as_str().to_string());
    }

    pub fn remove_sha1(&mut self, sha1: &str) {
        let key = sha1.to_ascii_lowercase();
        self.entries.remove(&key);
        self.entries.remove(sha1);
    }

    pub fn remove_project(&mut self, project_id: &str) {
        self.entries
            .retain(|_, e| e.project_id.as_deref() != Some(project_id));
    }

    pub fn remove_id(&mut self, id: &str) {
        self.entries.retain(|_, e| e.id.as_deref() != Some(id));
    }
}

impl ModHashIndexEntry {
    pub fn to_mod_spec(&self, file_name: String, fallback_side: Side) -> Option<ModSpec> {
        if self.status != "modrinth" {
            return None;
        }
        let id = self.id.clone().filter(|s| !s.is_empty())?;
        let name = self.name.clone().unwrap_or_else(|| id.clone());
        let content_type = match self.content_type.as_deref() {
            Some("resourcepack") => ContentType::Resourcepack,
            Some("shader") => ContentType::Shaderpack,
            Some("datapack") => ContentType::Datapack,
            _ => ContentType::Mod,
        };
        // Prefer raw Modrinth environment fields when present (post-fix cache).
        let side = if self.client_side.is_some() || self.server_side.is_some() {
            Side::from_modrinth(self.client_side.as_deref(), self.server_side.as_deref())
        } else {
            match self.side.as_deref() {
                Some("client") => Side::Client,
                Some("server") => Side::Server,
                Some("optional") => Side::Optional,
                Some("unknown") => Side::Unknown,
                Some("both") => Side::Both,
                _ => fallback_side,
            }
        };
        Some(ModSpec {
            id,
            name,
            version: self.version.clone().unwrap_or_else(|| "unknown".into()),
            side,
            source: ModSource {
                kind: SourceKind::Modrinth,
                project_id: self.project_id.clone(),
                file_id: self.file_id.clone(),
                url: self.download_url.clone(),
                path: None,
                icon_url: self.icon_url.clone(),
                categories: Vec::new(),
            },
            file_name: Some(file_name),
            hashes: Some(FileHashes {
                sha1: self.sha1.clone(),
                sha512: self.sha512.clone(),
            }),
            dependencies: vec![],
            status: vec!["ok".into()],
            content_type,
            authors: Vec::new(),
            option: None,
        })
    }
}
