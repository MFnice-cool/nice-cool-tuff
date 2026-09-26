use crate::github_pack::types::{RepoTransportMeta, TransportMetaError, REPO_TRANSPORT_FILE};
use crate::manifest::ProjectManifest;
use crate::packwiz::{import_packwiz_pack, PackwizImportError};
use std::{fs, path::Path};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ImportError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("manifest: {0}")]
    Manifest(#[from] crate::manifest::ManifestError),
    #[error("packwiz: {0}")]
    Packwiz(#[from] PackwizImportError),
    #[error("path escapes destination: {0}")]
    UnsafePath(String),
    #[error("transport is not ready (status={0})")]
    NotReady(String),
    #[error("signature: {0}")]
    BadSignature(String),
    #[error("transport metadata: {0}")]
    InvalidTransport(#[from] TransportMetaError),
    #[error("signed manifest sidecar missing: {0}")]
    MissingSignedManifest(String),
    #[error("unsupported tar entry type: {0}")]
    UnsupportedEntry(String),
    #[error("managed file is missing: {0}")]
    MissingManagedFile(String),
    #[error("content digest mismatch: expected {expected}, got {actual}")]
    ContentDigestMismatch { expected: String, actual: String },
}

pub fn import_repo_tree(
    root: &Path,
) -> Result<(ProjectManifest, Option<RepoTransportMeta>), ImportError> {
    let transport = load_repo_transport(root)?;
    if let Some(meta) = &transport {
        meta.validate()?;
        if !meta.is_ready() {
            return Err(ImportError::NotReady(meta.status.clone()));
        }
        let sidecar = root.join(&meta.manifest_file);
        if !sidecar.is_file() && meta.signer_public_key.is_some() {
            return Err(ImportError::MissingSignedManifest(
                meta.manifest_file.clone(),
            ));
        }
        verify_transport_content(root, meta)?;
        if sidecar.is_file() {
            let bytes = fs::read(&sidecar)?;
            if let (Some(pk), Some(sig)) = (&meta.signer_public_key, &meta.signature) {
                let payload = meta.signing_payload(&bytes)?;
                crate::github_pack::integrity::verify_payload(pk, sig, &payload)
                    .map_err(|e| ImportError::BadSignature(e.to_string()))?;
            }
            return Ok((ProjectManifest::load_from_path(sidecar)?, transport));
        }
    }
    if let Some(path) = find_sidecar_manifest(root) {
        return Ok((ProjectManifest::load_from_path(path)?, transport));
    }
    Ok((import_packwiz_pack(root)?, transport))
}

fn verify_transport_content(root: &Path, meta: &RepoTransportMeta) -> Result<(), ImportError> {
    if meta.content_digest.is_empty() {
        if meta.signer_public_key.is_some() {
            return Err(ImportError::ContentDigestMismatch {
                expected: "non-empty signed content digest".into(),
                actual: String::new(),
            });
        }
        return Ok(());
    }
    let transport_path = REPO_TRANSPORT_FILE.replace('\\', "/");
    let release_paths: std::collections::HashSet<&str> = meta
        .release_assets
        .iter()
        .map(|asset| asset.relative_path.as_str())
        .collect();
    let files = meta
        .managed_files
        .iter()
        .filter(|path| path.as_str() != transport_path && !release_paths.contains(path.as_str()))
        .map(|path| {
            if !root.join(path).is_file() {
                return Err(ImportError::MissingManagedFile(path.clone()));
            }
            Ok(crate::github_pack::staging::StagedFile {
                relative_path: path.clone(),
                sha512: None,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let actual = crate::github_pack::staging::content_digest(root, &files)
        .map_err(|error| ImportError::Io(std::io::Error::other(error.to_string())))?;
    if !actual.eq_ignore_ascii_case(&meta.content_digest) {
        return Err(ImportError::ContentDigestMismatch {
            expected: meta.content_digest.clone(),
            actual,
        });
    }
    Ok(())
}

fn load_repo_transport(root: &Path) -> Result<Option<RepoTransportMeta>, ImportError> {
    let path = root.join(REPO_TRANSPORT_FILE);
    if !path.is_file() {
        return Ok(None);
    }
    let raw = fs::read_to_string(path)?;
    Ok(Some(serde_json::from_str(&raw)?))
}

fn find_sidecar_manifest(root: &Path) -> Option<std::path::PathBuf> {
    fs::read_dir(root)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.ends_with(".tuffbox.json"))
                .unwrap_or(false)
        })
}

/// Reject absolute paths, `..`, and Windows prefixes. Returns a safe relative path.
pub fn safe_relative_path(name: &str) -> Result<String, ImportError> {
    let normalized = name.replace('\\', "/");
    if normalized.starts_with('/')
        || normalized.contains(':')
        || normalized.split('/').any(|seg| seg == ".." || seg == ".")
    {
        return Err(ImportError::UnsafePath(name.to_string()));
    }
    if normalized.is_empty() {
        return Err(ImportError::UnsafePath(name.to_string()));
    }
    Ok(normalized)
}

const MAX_EXTRACTED_BYTES: u64 = 512 * 1024 * 1024;
const MAX_EXTRACTED_FILES: usize = 20_000;

/// Extract a GitHub repository tarball (gzip) into `dest`, stripping the
/// leading `{owner}-{repo}-{sha}/` prefix and rejecting path traversal.
pub fn extract_github_tarball(archive: &[u8], dest: &Path) -> Result<(), ImportError> {
    fs::create_dir_all(dest)?;
    let decoder = flate2::read::GzDecoder::new(std::io::Cursor::new(archive));
    let mut archive = tar::Archive::new(decoder);
    let mut files = 0usize;
    let mut bytes = 0u64;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        let mut comps = path.components();
        let _strip_github_prefix = comps.next();
        let rel = comps.as_path();
        if rel.as_os_str().is_empty() {
            continue;
        }
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        let safe = safe_relative_path(&rel_str)?;
        files += 1;
        if files > MAX_EXTRACTED_FILES {
            return Err(ImportError::UnsafePath("too many files".into()));
        }
        let out = dest.join(&safe);
        if entry.header().entry_type().is_dir() {
            fs::create_dir_all(&out)?;
            continue;
        }
        if !entry.header().entry_type().is_file() {
            return Err(ImportError::UnsupportedEntry(rel_str));
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent)?;
        }
        bytes += entry.size();
        if bytes > MAX_EXTRACTED_BYTES {
            return Err(ImportError::UnsafePath("archive too large".into()));
        }
        let mut file = fs::File::create(&out)?;
        std::io::copy(&mut entry, &mut file)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github_pack::staging::{stage_repo_tree, StageOptions};
    use crate::manifest::{
        ContentType, FileHashes, LoaderKind, LoaderSpec, MinecraftSpec, ModSource, ModSpec,
        ProjectManifest, ProjectMetadata, Side, SourceKind,
    };

    #[test]
    fn prefers_sidecar_manifest_over_packwiz_name() {
        let project = tempfile::tempdir().unwrap();
        let staging = tempfile::tempdir().unwrap();
        let mut manifest = ProjectManifest {
            schema_version: crate::manifest::CURRENT_PROJECT_SCHEMA_VERSION.into(),
            project: ProjectMetadata {
                id: "demo".into(),
                name: "Sidecar Name".into(),
                version: "9.9.9".into(),
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
            mods: vec![ModSpec {
                id: "sodium".into(),
                name: "Sodium".into(),
                source: ModSource {
                    kind: SourceKind::Modrinth,
                    project_id: Some("AANobbMI".into()),
                    file_id: Some("ver1".into()),
                    url: Some("https://example.com/sodium.jar".into()),
                    path: None,
                    icon_url: None,
                    categories: vec![],
                },
                version: "0.6.0".into(),
                file_name: Some("sodium.jar".into()),
                hashes: Some(FileHashes {
                    sha1: Some("deadbeef".into()),
                    sha512: None,
                }),
                side: Side::Client,
                dependencies: vec![],
                status: vec![],
                content_type: ContentType::Mod,
                authors: vec![],
                option: None,
            }],
            overrides: None,
        };
        let manifest_path = project.path().join("demo.tuffbox.json");
        fs::write(&manifest_path, "{}").unwrap();
        stage_repo_tree(
            &manifest,
            &manifest_path,
            staging.path(),
            None,
            StageOptions::default(),
        )
        .unwrap();
        // pack.toml still says Demo Pack-style name from export; sidecar must win.
        manifest.project.name = "Sidecar Name".into();
        fs::write(
            staging.path().join("demo.tuffbox.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let (loaded, meta) = import_repo_tree(staging.path()).unwrap();
        assert_eq!(loaded.project.name, "Sidecar Name");
        assert_eq!(loaded.project.version, "9.9.9");
        assert!(meta.unwrap().is_ready());
    }

    #[test]
    fn rejects_path_traversal() {
        assert!(safe_relative_path("../etc/passwd").is_err());
        assert!(safe_relative_path("/abs").is_err());
        assert_eq!(safe_relative_path("mods/foo.jar").unwrap(), "mods/foo.jar");
    }

    #[test]
    fn extract_strips_github_prefix_and_blocks_escape() {
        let dest = tempfile::tempdir().unwrap();
        let mut tar_buf = Vec::new();
        {
            let enc = flate2::write::GzEncoder::new(&mut tar_buf, flate2::Compression::default());
            let mut builder = tar::Builder::new(enc);
            let mut header = tar::Header::new_gnu();
            header.set_size(3);
            header.set_cksum();
            builder
                .append_data(&mut header, "acme-pack-abc123/pack.toml", b"ok\n".as_ref())
                .unwrap();
            builder.finish().unwrap();
        }
        extract_github_tarball(&tar_buf, dest.path()).unwrap();
        assert_eq!(
            fs::read_to_string(dest.path().join("pack.toml")).unwrap(),
            "ok\n"
        );
    }
}
