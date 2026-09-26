//! Stage → validate → apply helpers for anonymous GitHub pack consume.

use crate::github_pack::staging::StagedFile;
use crate::github_pack::types::{RepoTransportMeta, TransportMetaError};
use sha2::{Digest, Sha512};
use std::{
    fs,
    path::{Path, PathBuf},
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ApplyError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("hash mismatch for {path}: expected {expected}, got {actual}")]
    HashMismatch {
        path: String,
        expected: String,
        actual: String,
    },
    #[error("required content file is missing: {0}")]
    MissingFile(String),
    #[error("unsafe managed path: {0}")]
    UnsafePath(String),
}

#[derive(Debug, Error)]
pub enum ReleaseAssetError {
    #[error("transport metadata: {0}")]
    InvalidTransport(#[from] TransportMetaError),
    #[error("release tag is missing")]
    MissingReleaseTag,
    #[error("unsafe release asset path: {0}")]
    UnsafePath(String),
    #[error("release asset {path} has size {actual}, expected {expected}")]
    SizeMismatch {
        path: String,
        expected: u64,
        actual: u64,
    },
    #[error("release asset hash mismatch for {path}: expected {expected}, got {actual}")]
    HashMismatch {
        path: String,
        expected: String,
        actual: String,
    },
    #[error("GitHub: {0}")]
    GitHub(#[from] crate::github_pack::client::GitHubError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Verify SHA-512 of staged jar files against the extracted tree.
pub fn verify_jar_hashes(root: &Path, files: &[StagedFile]) -> Result<(), ApplyError> {
    for file in files {
        let Some(expected) = &file.sha512 else {
            continue;
        };
        let path = root.join(&file.relative_path);
        if !path.is_file() {
            continue;
        }
        let actual = hex::encode(Sha512::digest(fs::read(&path)?));
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(ApplyError::HashMismatch {
                path: file.relative_path.clone(),
                expected: expected.clone(),
                actual,
            });
        }
    }
    Ok(())
}

/// Copy `new_managed` files from `from` into `to`, then delete previously
/// managed files that are no longer present. Leaves unrelated user files alone.
pub fn apply_managed_files(
    from: &Path,
    to: &Path,
    previous_managed: &[String],
    new_managed: &[String],
) -> Result<Vec<PathBuf>, ApplyError> {
    fs::create_dir_all(to)?;
    let mut copied = Vec::new();
    for rel in new_managed {
        let safe = crate::github_pack::import::safe_relative_path(rel)
            .map_err(|_| ApplyError::UnsafePath(rel.clone()))?;
        let src = from.join(&safe);
        if !src.is_file() {
            continue;
        }
        let dest = to.join(&safe);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&src, &dest)?;
        copied.push(PathBuf::from(rel));
    }
    for rel in previous_managed {
        let safe = crate::github_pack::import::safe_relative_path(rel)
            .map_err(|_| ApplyError::UnsafePath(rel.clone()))?;
        if new_managed.iter().any(|n| n == rel) {
            continue;
        }
        let dest = to.join(safe);
        if dest.is_file() {
            fs::remove_file(dest)?;
        }
    }
    Ok(copied)
}

pub fn verify_manifest_local_hashes(
    project_dir: &Path,
    manifest: &crate::manifest::ProjectManifest,
) -> Result<(), ApplyError> {
    for module in &manifest.mods {
        let Some(expected) = module
            .hashes
            .as_ref()
            .and_then(|h| h.sha512.as_deref().or(h.sha1.as_deref()))
        else {
            continue;
        };
        let Some(name) = &module.file_name else {
            continue;
        };
        let path = project_dir
            .join(module.content_type.folder_name())
            .join(name);
        if !path.is_file() {
            if matches!(
                module.source.kind,
                crate::manifest::SourceKind::Local | crate::manifest::SourceKind::Github
            ) {
                return Err(ApplyError::MissingFile(path.to_string_lossy().into_owned()));
            }
            continue;
        }
        let bytes = fs::read(&path)?;
        let actual = if expected.len() == 40 {
            {
                use sha1::Digest;
                hex::encode(sha1::Sha1::digest(&bytes))
            }
        } else {
            hex::encode(Sha512::digest(&bytes))
        };
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(ApplyError::HashMismatch {
                path: name.clone(),
                expected: expected.to_string(),
                actual,
            });
        }
    }
    Ok(())
}

pub fn materialize_release_assets(
    api: &dyn crate::github_pack::client::GitHubApi,
    owner: &str,
    repo: &str,
    destination: &Path,
    transport: &RepoTransportMeta,
) -> Result<Vec<PathBuf>, ReleaseAssetError> {
    transport.validate()?;
    if transport.release_assets.is_empty() {
        return Ok(Vec::new());
    }
    let tag = transport
        .release_tag
        .as_deref()
        .filter(|tag| !tag.trim().is_empty())
        .ok_or(ReleaseAssetError::MissingReleaseTag)?;
    let mut verified = Vec::with_capacity(transport.release_assets.len());
    for asset in &transport.release_assets {
        let relative = crate::github_pack::import::safe_relative_path(&asset.relative_path)
            .map_err(|_| ReleaseAssetError::UnsafePath(asset.relative_path.clone()))?;
        let bytes = api.download_release_asset(owner, repo, tag, &asset.file_name)?;
        let actual_size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if actual_size != asset.size {
            return Err(ReleaseAssetError::SizeMismatch {
                path: relative,
                expected: asset.size,
                actual: actual_size,
            });
        }
        let actual_hash = hex::encode(Sha512::digest(&bytes));
        if !actual_hash.eq_ignore_ascii_case(&asset.sha512) {
            return Err(ReleaseAssetError::HashMismatch {
                path: relative,
                expected: asset.sha512.clone(),
                actual: actual_hash,
            });
        }
        verified.push((relative, bytes));
    }
    let mut written = Vec::with_capacity(verified.len());
    for (relative, bytes) in verified {
        let path = destination.join(&relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let part = path.with_extension("tuffbox-part");
        fs::write(&part, &bytes)?;
        fs::rename(&part, &path)?;
        written.push(PathBuf::from(relative));
    }
    Ok(written)
}

/// Drop provider jars from the previous pack that are not in the incoming
/// pack. Used when Minecraft or the loader changed and a full rematerialize
/// is required. Leaves extra user files that were never in the old manifest.
pub fn remove_obsolete_content_files(
    instance_dir: &Path,
    old: &crate::manifest::ProjectManifest,
    new: &crate::manifest::ProjectManifest,
) -> Result<Vec<PathBuf>, ApplyError> {
    let keep: std::collections::HashSet<String> = new
        .mods
        .iter()
        .filter_map(|m| {
            m.file_name
                .as_ref()
                .map(|name| format!("{}/{}", m.content_type.folder_name(), name))
        })
        .collect();
    let mut removed = Vec::new();
    for module in &old.mods {
        let Some(name) = &module.file_name else {
            continue;
        };
        let rel = format!("{}/{}", module.content_type.folder_name(), name);
        if keep.contains(&rel) {
            continue;
        }
        let path = instance_dir.join(&rel);
        if path.is_file() {
            fs::remove_file(&path)?;
            removed.push(PathBuf::from(rel));
        }
    }
    Ok(removed)
}

pub fn managed_from_transport(meta: Option<&RepoTransportMeta>) -> Vec<String> {
    meta.map(|m| m.managed_files.clone()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github_pack::staging::StagedFile;

    #[test]
    fn hash_mismatch_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("mods.jar"), b"actual").unwrap();
        let files = vec![StagedFile {
            relative_path: "mods.jar".into(),
            sha512: Some("deadbeef".into()),
        }];
        let err = verify_jar_hashes(dir.path(), &files).unwrap_err();
        assert!(matches!(err, ApplyError::HashMismatch { .. }));
    }

    #[test]
    fn apply_copies_and_deletes_only_managed() {
        let from = tempfile::tempdir().unwrap();
        let to = tempfile::tempdir().unwrap();
        fs::write(from.path().join("keep.txt"), b"new").unwrap();
        fs::write(from.path().join("added.txt"), b"add").unwrap();
        fs::write(to.path().join("keep.txt"), b"old").unwrap();
        fs::write(to.path().join("gone.txt"), b"bye").unwrap();
        fs::write(to.path().join("user.txt"), b"mine").unwrap();
        apply_managed_files(
            from.path(),
            to.path(),
            &["keep.txt".into(), "gone.txt".into()],
            &["keep.txt".into(), "added.txt".into()],
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(to.path().join("keep.txt")).unwrap(),
            "new"
        );
        assert_eq!(
            fs::read_to_string(to.path().join("added.txt")).unwrap(),
            "add"
        );
        assert!(!to.path().join("gone.txt").exists());
        assert_eq!(
            fs::read_to_string(to.path().join("user.txt")).unwrap(),
            "mine"
        );
    }

    #[test]
    fn publish_install_update_and_rollback_fixture() {
        use crate::github_pack::client::{GitHubApi, MockGitHub};
        use crate::github_pack::import::{extract_github_tarball, import_repo_tree};
        use crate::github_pack::publish::publish_staged_tree;
        use crate::github_pack::staging::{stage_repo_tree, StageOptions};
        use crate::github_pack::update::diff_manifests;
        use crate::manifest::{
            ContentType, FileHashes, LoaderKind, LoaderSpec, MinecraftSpec, ModSource, ModSpec,
            ProjectManifest, ProjectMetadata, Side, SourceKind,
        };
        use crate::snapshot::{SnapshotMeta, SnapshotStore};

        fn pack(version: &str, mods: Vec<ModSpec>) -> ProjectManifest {
            ProjectManifest {
                schema_version: crate::manifest::CURRENT_PROJECT_SCHEMA_VERSION.into(),
                project: ProjectMetadata {
                    id: "demo".into(),
                    name: "Demo Pack".into(),
                    version: version.into(),
                    description: None,
                    authors: vec!["Tester".into()],
                },
                minecraft: MinecraftSpec {
                    version: "1.21.1".into(),
                },
                loader: LoaderSpec {
                    kind: LoaderKind::Fabric,
                    version: "0.16.0".into(),
                },
                brief: None,
                listing: None,
                java: None,
                profiles: vec![],
                mods,
                overrides: None,
            }
        }
        fn spec(id: &str, version: &str) -> ModSpec {
            ModSpec {
                id: id.into(),
                name: id.into(),
                source: ModSource {
                    kind: SourceKind::Modrinth,
                    project_id: Some(id.into()),
                    file_id: Some(version.into()),
                    url: Some(format!("https://example.com/{id}.jar")),
                    path: None,
                    icon_url: None,
                    categories: vec![],
                },
                version: version.into(),
                file_name: Some(format!("{id}.jar")),
                hashes: Some(FileHashes {
                    sha1: Some("aa".into()),
                    sha512: None,
                }),
                side: Side::Both,
                dependencies: vec![],
                status: vec![],
                content_type: ContentType::Mod,
                authors: vec![],
                option: None,
            }
        }
        fn custom(version: &str) -> ModSpec {
            ModSpec {
                id: "custom-lib".into(),
                name: "Custom Lib".into(),
                source: ModSource {
                    kind: SourceKind::Local,
                    project_id: None,
                    file_id: None,
                    url: None,
                    path: Some("mods/custom-lib.jar".into()),
                    icon_url: None,
                    categories: vec![],
                },
                version: version.into(),
                file_name: Some("custom-lib.jar".into()),
                hashes: Some(FileHashes {
                    sha1: Some("bb".into()),
                    sha512: None,
                }),
                side: Side::Both,
                dependencies: vec![],
                status: vec![],
                content_type: ContentType::Mod,
                authors: vec![],
                option: None,
            }
        }

        let api = MockGitHub::new("acme", "demo");
        let author = tempfile::tempdir().unwrap();
        let staging = tempfile::tempdir().unwrap();
        fs::create_dir_all(author.path().join("mods")).unwrap();
        fs::write(author.path().join("mods/custom-lib.jar"), b"custom-v1").unwrap();
        let v1 = pack(
            "1.0.0",
            vec![
                spec("sodium", "0.6.0"),
                spec("extra-mod", "1.0.0"),
                custom("1.0.0"),
            ],
        );
        let manifest_path = author.path().join("demo.tuffbox.json");
        fs::write(&manifest_path, "{}").unwrap();
        let staged = stage_repo_tree(
            &v1,
            &manifest_path,
            staging.path(),
            None,
            StageOptions {
                owner: Some("acme".into()),
                repo: Some("demo".into()),
                ..StageOptions::default()
            },
        )
        .unwrap();
        publish_staged_tree(&api, "acme", "demo", None, &staged, "v1").unwrap();

        let instance = tempfile::tempdir().unwrap();
        let tarball = api.get_bytes("/repos/acme/demo/tarball/HEAD").unwrap();
        extract_github_tarball(&tarball, instance.path()).unwrap();
        let (installed, meta) = import_repo_tree(instance.path()).unwrap();
        assert_eq!(installed.project.version, "1.0.0");
        let previous = managed_from_transport(meta.as_ref());

        let staging2 = tempfile::tempdir().unwrap();
        fs::write(author.path().join("mods/custom-lib.jar"), b"custom-v2").unwrap();
        let v2 = pack(
            "2.0.0",
            vec![
                spec("sodium", "0.6.1"),
                spec("iris", "1.0.0"),
                custom("1.1.0"),
            ],
        );
        let staged2 = stage_repo_tree(
            &v2,
            &manifest_path,
            staging2.path(),
            None,
            StageOptions {
                owner: Some("acme".into()),
                repo: Some("demo".into()),
                ..StageOptions::default()
            },
        )
        .unwrap();
        publish_staged_tree(&api, "acme", "demo", None, &staged2, "v2").unwrap();

        let extract = tempfile::tempdir().unwrap();
        let tarball2 = api.get_bytes("/repos/acme/demo/tarball/HEAD").unwrap();
        extract_github_tarball(&tarball2, extract.path()).unwrap();
        let (incoming, incoming_meta) = import_repo_tree(extract.path()).unwrap();
        let diff = diff_manifests(&installed, &incoming);
        assert!(diff.changes.iter().any(|c| matches!(
            c,
            crate::github_pack::update::PackChange::ModBumped { id, .. } if id == "sodium"
        )));
        assert!(diff.changes.iter().any(|c| matches!(
            c,
            crate::github_pack::update::PackChange::ModAdded { id, .. } if id == "iris"
        )));
        assert!(diff.changes.iter().any(|c| matches!(
            c,
            crate::github_pack::update::PackChange::ModRemoved { id, .. } if id == "extra-mod"
        )));
        assert!(diff.changes.iter().any(|c| matches!(
            c,
            crate::github_pack::update::PackChange::ModBumped {
                id,
                origin: crate::github_pack::update::TrustOrigin::Custom,
                ..
            } if id == "custom-lib"
        )));

        let store = SnapshotStore::new(instance.path());
        let snap = store
            .create_with_meta(
                "before-update",
                "github_pack_update",
                instance.path().join("demo.tuffbox.json"),
                None::<&std::path::Path>,
                &[] as &[std::path::PathBuf],
                SnapshotMeta {
                    operation: "github_pack_update".into(),
                    managed_files: previous.iter().map(std::path::PathBuf::from).collect(),
                    ..Default::default()
                },
            )
            .unwrap();
        apply_managed_files(
            extract.path(),
            instance.path(),
            &previous,
            &managed_from_transport(incoming_meta.as_ref()),
        )
        .unwrap();
        let after = import_repo_tree(instance.path()).unwrap().0;
        assert_eq!(after.project.version, "2.0.0");
        assert!(after.mods.iter().any(|m| m.id == "iris"));
        assert!(!after.mods.iter().any(|m| m.id == "extra-mod"));
        assert_eq!(
            after
                .mods
                .iter()
                .find(|m| m.id == "custom-lib")
                .unwrap()
                .version,
            "1.1.0"
        );
        store.rollback(&snap.id).unwrap();
        let rolled = import_repo_tree(instance.path()).unwrap().0;
        assert_eq!(rolled.project.version, "1.0.0");
        assert!(rolled.mods.iter().any(|m| m.id == "extra-mod"));
        assert!(!rolled.mods.iter().any(|m| m.id == "iris"));
    }

    #[test]
    fn hash_mismatch_rolls_back_to_identical_files() {
        use crate::github_pack::staging::StagedFile;
        use crate::snapshot::{SnapshotMeta, SnapshotStore};

        let instance = tempfile::tempdir().unwrap();
        fs::create_dir_all(instance.path().join("mods")).unwrap();
        fs::write(instance.path().join("mods/kept.jar"), b"good").unwrap();
        let manifest = serde_json::json!({
            "schemaVersion": crate::manifest::CURRENT_PROJECT_SCHEMA_VERSION,
            "project": { "id": "demo", "name": "Demo", "version": "1.0.0", "authors": [] },
            "minecraft": { "version": "1.21.1" },
            "loader": { "kind": "fabric", "version": "0.16.0" },
            "mods": []
        });
        let manifest_path = instance.path().join("demo.tuffbox.json");
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        let store = SnapshotStore::new(instance.path());
        let snap = store
            .create_with_meta(
                "before-bad-update",
                "github_pack_update",
                &manifest_path,
                None::<&std::path::Path>,
                &[] as &[std::path::PathBuf],
                SnapshotMeta {
                    operation: "github_pack_update".into(),
                    managed_files: vec![std::path::PathBuf::from("mods/kept.jar")],
                    ..Default::default()
                },
            )
            .unwrap();
        fs::write(instance.path().join("mods/evil.jar"), b"tampered").unwrap();
        let err = verify_jar_hashes(
            instance.path(),
            &[StagedFile {
                relative_path: "mods/evil.jar".into(),
                sha512: Some("deadbeef".into()),
            }],
        )
        .unwrap_err();
        assert!(matches!(err, ApplyError::HashMismatch { .. }));
        store.rollback(&snap.id).unwrap();
        assert_eq!(
            fs::read(instance.path().join("mods/kept.jar")).unwrap(),
            b"good"
        );
        assert!(!instance.path().join("mods/evil.jar").exists());
        let restored: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(manifest_path).unwrap()).unwrap();
        assert_eq!(restored["project"]["version"], "1.0.0");
    }

    #[test]
    fn rematerialize_removes_obsolete_provider_jars_keeps_user_files() {
        use crate::manifest::{
            ContentType, LoaderKind, LoaderSpec, MinecraftSpec, ModSource, ModSpec,
            ProjectManifest, ProjectMetadata, Side, SourceKind,
        };

        fn pack(mods: Vec<ModSpec>) -> ProjectManifest {
            ProjectManifest {
                schema_version: crate::manifest::CURRENT_PROJECT_SCHEMA_VERSION.into(),
                project: ProjectMetadata {
                    id: "demo".into(),
                    name: "Demo".into(),
                    version: "1.0.0".into(),
                    description: None,
                    authors: vec![],
                },
                minecraft: MinecraftSpec {
                    version: "1.21.1".into(),
                },
                loader: LoaderSpec {
                    kind: LoaderKind::Fabric,
                    version: "0.16.0".into(),
                },
                brief: None,
                listing: None,
                java: None,
                profiles: vec![],
                mods,
                overrides: None,
            }
        }
        fn provider(id: &str, file: &str) -> ModSpec {
            ModSpec {
                id: id.into(),
                name: id.into(),
                source: ModSource {
                    kind: SourceKind::Modrinth,
                    project_id: Some(id.into()),
                    file_id: Some("1".into()),
                    url: Some(format!("https://example.com/{file}")),
                    path: None,
                    icon_url: None,
                    categories: vec![],
                },
                version: "1.0.0".into(),
                file_name: Some(file.into()),
                hashes: None,
                side: Side::Both,
                dependencies: vec![],
                status: vec![],
                content_type: ContentType::Mod,
                authors: vec![],
                option: None,
            }
        }

        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("mods")).unwrap();
        fs::write(dir.path().join("mods/old-loader.jar"), b"old").unwrap();
        fs::write(dir.path().join("mods/kept.jar"), b"keep").unwrap();
        fs::write(dir.path().join("mods/user-extra.jar"), b"mine").unwrap();
        let old = pack(vec![
            provider("sodium", "old-loader.jar"),
            provider("iris", "kept.jar"),
        ]);
        let new = pack(vec![provider("iris", "kept.jar")]);
        let removed = remove_obsolete_content_files(dir.path(), &old, &new).unwrap();
        assert!(removed.iter().any(|p| p.ends_with("old-loader.jar")));
        assert!(!dir.path().join("mods/old-loader.jar").exists());
        assert_eq!(fs::read(dir.path().join("mods/kept.jar")).unwrap(), b"keep");
        assert_eq!(
            fs::read(dir.path().join("mods/user-extra.jar")).unwrap(),
            b"mine"
        );
    }
}
