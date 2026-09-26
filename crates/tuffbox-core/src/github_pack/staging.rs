use crate::github_pack::integrity::{sign_payload, Ed25519KeyPair};
use crate::github_pack::types::{
    ReleaseAssetRef, RepoTransportMeta, REPO_TRANSPORT_FILE, TRANSPORT_SCHEMA_VERSION,
};
use crate::lockfile::TuffboxLockfile;
use crate::manifest::ProjectManifest;
use crate::packwiz::{export_packwiz_pack, PackwizExportError};
use sha2::{Digest, Sha256, Sha512};
use std::{
    fs,
    path::{Path, PathBuf},
};
use thiserror::Error;

const CUSTOM_JAR_GIT_LIMIT: u64 = 50 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum StageError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("packwiz export: {0}")]
    Packwiz(#[from] PackwizExportError),
    #[error("lockfile: {0}")]
    Lockfile(#[from] crate::lockfile::LockfileError),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("manifest has no parent directory")]
    NoParent,
    #[error("symlink rejected: {0}")]
    Symlink(String),
}

#[derive(Debug, Clone, Default)]
pub struct StageOptions {
    pub release_tag: Option<String>,
    pub status: Option<String>,
    pub release_assets: Vec<ReleaseAssetRef>,
    pub owner: Option<String>,
    pub repo: Option<String>,
    pub signer: Option<Ed25519KeyPair>,
    /// Override the 50 MiB git blob limit (tests use a tiny value).
    pub custom_jar_git_limit: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct StagedFile {
    pub relative_path: String,
    pub sha512: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PendingReleaseAsset {
    pub mod_id: String,
    pub file_name: String,
    pub relative_path: String,
    pub abs_path: PathBuf,
    pub sha512: String,
    pub size: u64,
}

#[derive(Debug, Clone)]
pub struct StagedRepo {
    pub root: PathBuf,
    pub files: Vec<StagedFile>,
    pub transport: RepoTransportMeta,
    pub pending_release_assets: Vec<PendingReleaseAsset>,
}

pub fn stage_repo_tree(
    manifest: &ProjectManifest,
    manifest_path: &Path,
    staging_root: &Path,
    lockfile: Option<&TuffboxLockfile>,
    opts: StageOptions,
) -> Result<StagedRepo, StageError> {
    let _project_dir = manifest_path.parent().ok_or(StageError::NoParent)?;
    if staging_root.exists() {
        fs::remove_dir_all(staging_root)?;
    }
    fs::create_dir_all(staging_root)?;
    export_packwiz_pack(manifest, manifest_path, staging_root)?;

    let manifest_file = format!("{}.tuffbox.json", manifest.project.id);
    let lockfile_file = format!("{}.tuffbox.lock.json", manifest.project.id);
    let manifest_bytes = serde_json::to_vec_pretty(manifest)?;
    fs::write(staging_root.join(&manifest_file), &manifest_bytes)?;
    if let Some(lock) = lockfile {
        lock.save_to_path(staging_root.join(&lockfile_file))?;
    } else {
        let graph = crate::graph::DependencyGraph::from_manifest(manifest);
        TuffboxLockfile::from_manifest_and_graph(manifest, &graph)
            .save_to_path(staging_root.join(&lockfile_file))?;
    }

    let tag = opts
        .release_tag
        .clone()
        .unwrap_or_else(|| format!("v{}", manifest.project.version));
    let owner = opts.owner.clone().unwrap_or_else(|| "owner".into());
    let repo = opts
        .repo
        .clone()
        .unwrap_or_else(|| manifest.project.id.clone());
    let readme = format!(
        "# {}\n\nInstall in TuffBox: import GitHub repository `{owner}/{repo}`.\n\nPrism / packwiz users: this repo is a packwiz pack (`pack.toml`).\n",
        manifest.project.name
    );
    fs::write(staging_root.join("README.md"), readme)?;

    let limit = opts.custom_jar_git_limit.unwrap_or(CUSTOM_JAR_GIT_LIMIT);
    let pending = extract_oversized_jars(manifest, staging_root, limit, &owner, &repo, &tag)?;
    if !pending.is_empty() {
        rebuild_packwiz_index(staging_root)?;
    }

    let mut files = collect_files(staging_root, staging_root)?;
    files.retain(|f| f.relative_path != REPO_TRANSPORT_FILE.replace('\\', "/"));
    files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    let digest = content_digest(staging_root, &files)?;
    let managed_files: Vec<String> = files.iter().map(|f| f.relative_path.clone()).collect();

    let mut release_assets = opts.release_assets.clone();
    for asset in &pending {
        if !release_assets
            .iter()
            .any(|a| a.file_name == asset.file_name)
        {
            release_assets.push(ReleaseAssetRef {
                mod_id: asset.mod_id.clone(),
                file_name: asset.file_name.clone(),
                relative_path: asset.relative_path.clone(),
                sha512: asset.sha512.clone(),
                size: asset.size,
            });
        }
    }

    let has_external = !pending.is_empty() || !release_assets.is_empty();
    let status = opts.status.unwrap_or_else(|| {
        if has_external {
            "publishing".into()
        } else {
            "ready".into()
        }
    });

    let mut transport = RepoTransportMeta {
        schema_version: TRANSPORT_SCHEMA_VERSION,
        manifest_file,
        lockfile_file,
        pack_version: manifest.project.version.clone(),
        release_tag: Some(tag),
        status,
        release_assets,
        managed_files,
        content_digest: digest,
        signer_public_key: opts.signer.as_ref().map(Ed25519KeyPair::public_key_b64),
        signature: None,
    };
    for asset in &transport.release_assets {
        if !transport
            .managed_files
            .iter()
            .any(|path| path == &asset.relative_path)
        {
            transport.managed_files.push(asset.relative_path.clone());
        }
    }
    transport
        .managed_files
        .push(REPO_TRANSPORT_FILE.replace('\\', "/"));
    if let Some(key) = &opts.signer {
        let mut signed_transport = transport.clone();
        if !pending.is_empty() && signed_transport.status == "publishing" {
            signed_transport.status = "ready".into();
        }
        let payload = signed_transport.signing_payload(&manifest_bytes)?;
        transport.signature = Some(sign_payload(key, &payload));
    }
    let transport_path = staging_root.join(REPO_TRANSPORT_FILE);
    if let Some(parent) = transport_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&transport_path, serde_json::to_vec_pretty(&transport)?)?;
    files.push(StagedFile {
        relative_path: REPO_TRANSPORT_FILE.replace('\\', "/"),
        sha512: None,
    });

    Ok(StagedRepo {
        root: staging_root.to_path_buf(),
        files,
        transport,
        pending_release_assets: pending,
    })
}

pub fn content_digest(root: &Path, files: &[StagedFile]) -> Result<String, StageError> {
    let mut hasher = Sha256::new();
    for file in files {
        if file.relative_path == REPO_TRANSPORT_FILE.replace('\\', "/") {
            continue;
        }
        hasher.update(file.relative_path.as_bytes());
        hasher.update([0u8]);
        let bytes = fs::read(root.join(&file.relative_path))?;
        hasher.update(Sha256::digest(&bytes));
        hasher.update([b'\n']);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn extract_oversized_jars(
    manifest: &ProjectManifest,
    staging_root: &Path,
    limit: u64,
    owner: &str,
    repo: &str,
    tag: &str,
) -> Result<Vec<PendingReleaseAsset>, StageError> {
    let mut pending = Vec::new();
    let jars = collect_files(staging_root, staging_root)?;
    for file in jars {
        if !file.relative_path.ends_with(".jar") {
            continue;
        }
        let abs = staging_root.join(&file.relative_path);
        let size = abs.metadata()?.len();
        if size < limit {
            continue;
        }
        let bytes = fs::read(&abs)?;
        let sha512 = hex::encode(Sha512::digest(&bytes));
        let file_name = Path::new(&file.relative_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("asset.jar")
            .to_string();
        let mod_id = manifest
            .mods
            .iter()
            .find(|module| module.file_name.as_deref() == Some(file_name.as_str()))
            .map(|module| module.id.clone())
            .unwrap_or_else(|| file_name.clone());
        let safe_mod_id: String = mod_id
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                    ch
                } else {
                    '-'
                }
            })
            .collect();
        let release_file_name = format!("{safe_mod_id}-{file_name}");
        let sidecar = abs.with_extension("pw.toml");
        let body = format!(
            "name = \"{file_name}\"\nfilename = \"{file_name}\"\nside = \"both\"\n\n[download]\nurl = \"https://github.com/{owner}/{repo}/releases/download/{tag}/{release_file_name}\"\nhash-format = \"sha512\"\nhash = \"{sha512}\"\n"
        );
        fs::write(&sidecar, body)?;
        let dest = staging_root
            .join(".tuffbox")
            .join("release-assets")
            .join(&release_file_name);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(&abs, &dest)?;
        pending.push(PendingReleaseAsset {
            mod_id,
            file_name: release_file_name,
            relative_path: file.relative_path,
            abs_path: dest,
            sha512,
            size,
        });
    }
    Ok(pending)
}

fn rebuild_packwiz_index(staging_root: &Path) -> Result<(), StageError> {
    let mut entries: Vec<(String, String, bool)> = Vec::new();
    collect_index_entries(staging_root, staging_root, &mut entries)?;
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut index_toml = String::from("hash-format = \"sha256\"\n\n");
    for (file, hash, metafile) in &entries {
        index_toml.push_str("[[files]]\n");
        index_toml.push_str(&format!("file = \"{}\"\n", file.replace('"', "\\\"")));
        index_toml.push_str(&format!("hash = \"{hash}\"\n"));
        if *metafile {
            index_toml.push_str("metafile = true\n");
        }
        index_toml.push('\n');
    }
    fs::write(staging_root.join("index.toml"), &index_toml)?;
    let index_hash = hex::encode(Sha256::digest(index_toml.as_bytes()));
    let pack_path = staging_root.join("pack.toml");
    if pack_path.is_file() {
        let mut pack = fs::read_to_string(&pack_path)?;
        if let Some(start) = pack.find("hash = \"") {
            let from = start + 8;
            if let Some(end) = pack[from..].find('"') {
                pack.replace_range(from..from + end, &index_hash);
                fs::write(pack_path, pack)?;
            }
        }
    }
    Ok(())
}

fn collect_index_entries(
    root: &Path,
    dir: &Path,
    out: &mut Vec<(String, String, bool)>,
) -> Result<(), StageError> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if rel == "pack.toml"
            || rel == "index.toml"
            || rel == "README.md"
            || rel.starts_with(".tuffbox/")
            || rel.ends_with(".tuffbox.json")
            || rel.ends_with(".tuffbox.lock.json")
        {
            if path.is_dir() && rel.starts_with(".tuffbox/") {
                continue;
            }
            if path.is_file() {
                continue;
            }
        }
        if path.is_dir() {
            if rel == ".tuffbox" {
                continue;
            }
            collect_index_entries(root, &path, out)?;
        } else if path.is_file() {
            let bytes = fs::read(&path)?;
            let hash = hex::encode(Sha256::digest(&bytes));
            let metafile = rel.ends_with(".pw.toml");
            out.push((rel, hash, metafile));
        }
    }
    Ok(())
}

fn collect_files(root: &Path, dir: &Path) -> Result<Vec<StagedFile>, StageError> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.symlink_metadata()?.file_type().is_symlink() {
            return Err(StageError::Symlink(
                path.strip_prefix(root)
                    .unwrap_or(&path)
                    .display()
                    .to_string(),
            ));
        }
        if path.is_dir() {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            if rel == ".tuffbox/release-assets" {
                continue;
            }
            out.extend(collect_files(root, &path)?);
        } else if path.is_file() {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            if rel.starts_with(".tuffbox/release-assets/") {
                continue;
            }
            let sha512 = if is_distributed_binary(&path) {
                let bytes = fs::read(&path)?;
                Some(hex::encode(Sha512::digest(&bytes)))
            } else {
                None
            };
            out.push(StagedFile {
                relative_path: rel,
                sha512,
            });
        }
    }
    Ok(out)
}

fn is_distributed_binary(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()).map(|s| s.to_ascii_lowercase()),
        Some(ext) if matches!(ext.as_str(), "jar" | "zip" | "litemod" | "disabled")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{
        ContentType, FileHashes, LoaderKind, LoaderSpec, MinecraftSpec, ModSource, ModSpec,
        ProjectManifest, ProjectMetadata, Side, SourceKind,
    };

    #[test]
    fn stage_writes_canonical_layout() {
        let project = tempfile::tempdir().unwrap();
        let staging = tempfile::tempdir().unwrap();
        let manifest = ProjectManifest {
            schema_version: crate::manifest::CURRENT_PROJECT_SCHEMA_VERSION.into(),
            project: ProjectMetadata {
                id: "demo".into(),
                name: "Demo Pack".into(),
                version: "1.2.3".into(),
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

        let staged = stage_repo_tree(
            &manifest,
            &manifest_path,
            staging.path(),
            None,
            StageOptions::default(),
        )
        .unwrap();

        assert!(staging.path().join("pack.toml").is_file());
        assert!(staging.path().join("index.toml").is_file());
        assert!(staging.path().join("demo.tuffbox.json").is_file());
        assert!(staging.path().join("demo.tuffbox.lock.json").is_file());
        assert!(staging.path().join("mods/sodium.pw.toml").is_file());
        assert!(staging
            .path()
            .join(".tuffbox/repo-transport.json")
            .is_file());
        assert!(staged.transport.is_ready());
        assert_eq!(staged.transport.pack_version, "1.2.3");
        assert!(staged
            .transport
            .managed_files
            .iter()
            .any(|f| f == "pack.toml"));
    }
}
