use crate::{graph::DependencyGraph, manifest::ProjectManifest};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};
use thiserror::Error;

pub const CURRENT_LOCKFILE_SCHEMA_VERSION: &str = "0.1.0";
pub const SUPPORTED_LOCKFILE_SCHEMA_VERSIONS: &[&str] = &["0.1.0", "0.1"];

#[derive(Debug, Error)]
pub enum LockfileError {
    #[error("failed to read lockfile {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse lockfile {path}: {source}")]
    Parse {
        path: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("unsupported lockfile schema version {version}; supported versions: {supported}")]
    UnsupportedSchemaVersion { version: String, supported: String },
    #[error("failed to write lockfile {path}: {source}")]
    Write {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TuffboxLockfile {
    pub schema_version: String,
    pub project_id: String,
    pub project_version: String,
    pub minecraft_version: String,
    pub loader: LockedLoader,
    #[serde(default)]
    pub java_major: Option<u16>,
    pub mods: Vec<LockedMod>,
    pub graph: LockedGraph,
    pub generated_at: String,
}

impl TuffboxLockfile {
    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self, LockfileError> {
        let path_ref = path.as_ref();
        let path_string = path_ref.display().to_string();
        let raw = fs::read_to_string(path_ref).map_err(|source| LockfileError::Read {
            path: path_string.clone(),
            source,
        })?;
        let mut value: serde_json::Value =
            serde_json::from_str(&raw).map_err(|source| LockfileError::Parse {
                path: path_string.clone(),
                source,
            })?;
        migrate_lockfile_value(&mut value)?;
        serde_json::from_value(value).map_err(|source| LockfileError::Parse {
            path: path_string,
            source,
        })
    }

    pub fn migrate_to_current_schema(&mut self) {
        self.schema_version = CURRENT_LOCKFILE_SCHEMA_VERSION.to_string();
    }

    pub fn from_manifest_and_graph(manifest: &ProjectManifest, graph: &DependencyGraph) -> Self {
        let mut mods: Vec<LockedMod> = manifest
            .mods
            .iter()
            .map(|module| LockedMod {
                id: module.id.clone(),
                name: module.name.clone(),
                version: module.version.clone(),
                source: LockedSource {
                    kind: module.source.kind.as_str().to_string(),
                    project_id: module.source.project_id.clone(),
                    file_id: module.source.file_id.clone(),
                    url: module.source.url.clone(),
                    path: module.source.path.clone(),
                },
                file_name: module.file_name.clone(),
                hashes: LockedHashes {
                    sha1: module
                        .hashes
                        .as_ref()
                        .and_then(|hashes| hashes.sha1.clone()),
                    sha512: module
                        .hashes
                        .as_ref()
                        .and_then(|hashes| hashes.sha512.clone()),
                },
                side: module.side.as_str().to_string(),
            })
            .collect();

        mods.sort_by_key(|a| a.id.clone());

        let edges = graph
            .edges
            .iter()
            .map(|edge| LockedEdge {
                from: edge.from.0.clone(),
                to: edge.to.0.clone(),
                kind: edge.kind.as_str().to_string(),
                constraint: edge.constraint.clone(),
                reason: edge.reason.clone(),
            })
            .collect();

        Self {
            schema_version: CURRENT_LOCKFILE_SCHEMA_VERSION.to_string(),
            project_id: manifest.project.id.clone(),
            project_version: manifest.project.version.clone(),
            minecraft_version: manifest.minecraft.version.clone(),
            loader: LockedLoader {
                kind: manifest.loader.kind.as_str().to_string(),
                version: manifest.loader.version.clone(),
            },
            java_major: manifest.java.as_ref().and_then(|java| java.major),
            mods,
            graph: LockedGraph {
                node_count: graph.nodes.len(),
                edge_count: graph.edges.len(),
                edges,
            },
            generated_at: rfc3339_now(),
        }
    }

    pub fn save_to_path(&self, path: impl AsRef<Path>) -> Result<(), LockfileError> {
        let path_ref = path.as_ref();
        let path_string = path_ref.display().to_string();
        if let Some(parent) = path_ref.parent() {
            fs::create_dir_all(parent).map_err(|source| LockfileError::Write {
                path: path_string.clone(),
                source,
            })?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|source| LockfileError::Parse {
            path: path_string.clone(),
            source,
        })?;
        fs::write(path_ref, json).map_err(|source| LockfileError::Write {
            path: path_string,
            source,
        })?;
        Ok(())
    }

    /// Re-hash jars on disk and drop lock entries whose files are missing
    /// (packwiz `Refresh()`-style). Returns how many mods were updated or removed.
    pub fn refresh_from_disk(&mut self, project_dir: impl AsRef<Path>) -> usize {
        use sha1::{Digest, Sha1};
        use sha2::Sha512;

        let project_dir = project_dir.as_ref();
        let mut changed = 0usize;
        let mut kept = Vec::with_capacity(self.mods.len());
        for mut module in self.mods.drain(..) {
            let path = resolve_locked_jar(project_dir, &module);
            let Some(path) = path else {
                changed += 1; // dropped missing
                continue;
            };
            let Ok(bytes) = fs::read(&path) else {
                changed += 1;
                continue;
            };
            let sha1 = format!("{:x}", Sha1::digest(&bytes));
            let sha512 = format!("{:x}", Sha512::digest(&bytes));
            if module.hashes.sha1.as_deref() != Some(sha1.as_str())
                || module.hashes.sha512.as_deref() != Some(sha512.as_str())
            {
                module.hashes.sha1 = Some(sha1);
                module.hashes.sha512 = Some(sha512);
                changed += 1;
            }
            kept.push(module);
        }
        self.mods = kept;
        self.generated_at = rfc3339_now();
        changed
    }
}

fn resolve_locked_jar(project_dir: &Path, module: &LockedMod) -> Option<std::path::PathBuf> {
    if let Some(rel) = module.source.path.as_ref() {
        let p = project_dir.join(rel);
        if p.is_file() {
            return Some(p);
        }
    }
    if let Some(name) = module.file_name.as_ref() {
        let p = project_dir.join("mods").join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

#[cfg(test)]
mod refresh_tests {
    use super::*;

    #[test]
    fn refresh_drops_missing_and_rehashes() {
        let dir = tempfile::tempdir().unwrap();
        let mods = dir.path().join("mods");
        fs::create_dir_all(&mods).unwrap();
        fs::write(mods.join("a.jar"), b"hello").unwrap();

        let mut lock = TuffboxLockfile {
            schema_version: CURRENT_LOCKFILE_SCHEMA_VERSION.into(),
            project_id: "p".into(),
            project_version: "1".into(),
            minecraft_version: "1.20.1".into(),
            loader: LockedLoader {
                kind: "fabric".into(),
                version: "0.15".into(),
            },
            java_major: None,
            mods: vec![
                LockedMod {
                    id: "a".into(),
                    name: "A".into(),
                    version: "1".into(),
                    source: LockedSource {
                        kind: "local".into(),
                        project_id: None,
                        file_id: None,
                        url: None,
                        path: None,
                    },
                    file_name: Some("a.jar".into()),
                    hashes: LockedHashes {
                        sha1: Some("bad".into()),
                        sha512: None,
                    },
                    side: "both".into(),
                },
                LockedMod {
                    id: "gone".into(),
                    name: "Gone".into(),
                    version: "1".into(),
                    source: LockedSource {
                        kind: "local".into(),
                        project_id: None,
                        file_id: None,
                        url: None,
                        path: None,
                    },
                    file_name: Some("missing.jar".into()),
                    hashes: LockedHashes {
                        sha1: None,
                        sha512: None,
                    },
                    side: "both".into(),
                },
            ],
            graph: LockedGraph {
                node_count: 0,
                edge_count: 0,
                edges: vec![],
            },
            generated_at: "t".into(),
        };

        let changed = lock.refresh_from_disk(dir.path());
        assert!(changed >= 2);
        assert_eq!(lock.mods.len(), 1);
        assert_eq!(lock.mods[0].id, "a");
        assert!(lock.mods[0].hashes.sha1.as_ref().unwrap().len() == 40);
    }

    #[test]
    fn save_to_path_writes_pretty_json_beside_manifest_stem() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("foo.tuffbox.lock.json");
        let lock = TuffboxLockfile {
            schema_version: CURRENT_LOCKFILE_SCHEMA_VERSION.into(),
            project_id: "foo".into(),
            project_version: "1.0.0".into(),
            minecraft_version: "1.21.1".into(),
            loader: LockedLoader {
                kind: "fabric".into(),
                version: "0.16.0".into(),
            },
            java_major: None,
            mods: vec![],
            graph: LockedGraph {
                node_count: 0,
                edge_count: 0,
                edges: vec![],
            },
            generated_at: "t".into(),
        };

        lock.save_to_path(&path).unwrap();
        let loaded = TuffboxLockfile::load_from_path(&path).unwrap();
        assert_eq!(loaded.project_id, "foo");
        assert_eq!(loaded.project_version, "1.0.0");
        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.contains('\n'), "lockfile should be pretty-printed");
    }
}

pub fn migrate_lockfile_value(value: &mut serde_json::Value) -> Result<(), LockfileError> {
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

    if !SUPPORTED_LOCKFILE_SCHEMA_VERSIONS.contains(&normalized.as_str()) {
        return Err(LockfileError::UnsupportedSchemaVersion {
            version,
            supported: SUPPORTED_LOCKFILE_SCHEMA_VERSIONS.join(", "),
        });
    }

    if let Some(object) = value.as_object_mut() {
        object.insert(
            "schemaVersion".to_string(),
            serde_json::Value::String(CURRENT_LOCKFILE_SCHEMA_VERSION.to_string()),
        );
        object
            .entry("mods".to_string())
            .or_insert_with(|| serde_json::Value::Array(Vec::new()));
    }

    Ok(())
}

fn rfc3339_now() -> String {
    crate::time_util::rfc3339_now()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockedLoader {
    pub kind: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockedMod {
    pub id: String,
    pub name: String,
    pub version: String,
    pub source: LockedSource,
    pub file_name: Option<String>,
    pub hashes: LockedHashes,
    pub side: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockedSource {
    pub kind: String,
    pub project_id: Option<String>,
    pub file_id: Option<String>,
    pub url: Option<String>,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockedHashes {
    pub sha1: Option<String>,
    pub sha512: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockedGraph {
    pub node_count: usize,
    pub edge_count: usize,
    pub edges: Vec<LockedEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockedEdge {
    pub from: String,
    pub to: String,
    pub kind: String,
    pub constraint: Option<String>,
    pub reason: Option<String>,
}
