use serde::{Deserialize, Serialize};
use std::path::{Component, Path};
use thiserror::Error;

pub const TRANSPORT_SCHEMA_VERSION: u32 = 2;
pub const REPO_TRANSPORT_FILE: &str = ".tuffbox/repo-transport.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TransportKind {
    Github,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseAssetRef {
    pub mod_id: String,
    pub file_name: String,
    #[serde(default)]
    pub relative_path: String,
    #[serde(default)]
    pub sha512: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TransportMetaError {
    #[error("signer public key and signature must be present together")]
    IncompleteSignature,
    #[error("release asset {0} has incomplete metadata")]
    IncompleteReleaseAsset(String),
    #[error("legacy release asset metadata is unsupported for schema {0}")]
    LegacyReleaseAsset(u32),
    #[error("unsafe transport path: {0}")]
    UnsafePath(String),
    #[error("unsafe release tag: {0}")]
    UnsafeReleaseTag(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RepoTransportMeta {
    pub schema_version: u32,
    pub manifest_file: String,
    pub lockfile_file: String,
    pub pack_version: String,
    #[serde(default)]
    pub release_tag: Option<String>,
    /// `ready` | `publishing`
    #[serde(default = "ready_status")]
    pub status: String,
    #[serde(default)]
    pub release_assets: Vec<ReleaseAssetRef>,
    #[serde(default)]
    pub managed_files: Vec<String>,
    /// SHA-256 of sorted `path + NUL + sha256(bytes)` for every managed file
    /// except `.tuffbox/repo-transport.json`. Used for no-op publish detection.
    #[serde(default)]
    pub content_digest: String,
    #[serde(default)]
    pub signer_public_key: Option<String>,
    #[serde(default)]
    pub signature: Option<String>,
}

fn ready_status() -> String {
    "ready".into()
}

impl RepoTransportMeta {
    pub fn is_ready(&self) -> bool {
        self.status == "ready"
    }

    pub fn validate(&self) -> Result<(), TransportMetaError> {
        if self.signer_public_key.is_some() != self.signature.is_some() {
            return Err(TransportMetaError::IncompleteSignature);
        }
        validate_transport_path(&self.manifest_file)?;
        validate_transport_path(&self.lockfile_file)?;
        for path in &self.managed_files {
            validate_transport_path(path)?;
        }
        for asset in &self.release_assets {
            if self.schema_version < 2
                && (asset.relative_path.is_empty() || asset.sha512.is_empty() || asset.size == 0)
            {
                return Err(TransportMetaError::LegacyReleaseAsset(self.schema_version));
            }
            if asset.file_name.trim().is_empty()
                || asset.relative_path.trim().is_empty()
                || asset.sha512.trim().is_empty()
                || asset.size == 0
            {
                return Err(TransportMetaError::IncompleteReleaseAsset(
                    asset.file_name.clone(),
                ));
            }
            validate_transport_path(&asset.relative_path)?;
        }
        if let Some(tag) = self.release_tag.as_deref() {
            crate::github_pack::source::validate_github_ref(tag)
                .map_err(|_| TransportMetaError::UnsafeReleaseTag(tag.to_string()))?;
        }
        Ok(())
    }

    pub fn signing_payload(&self, manifest_bytes: &[u8]) -> Result<Vec<u8>, serde_json::Error> {
        let mut unsigned = self.clone();
        unsigned.signature = None;
        let metadata = serde_json::to_vec(&unsigned)?;
        let mut payload = Vec::with_capacity(
            24usize
                .saturating_add(metadata.len())
                .saturating_add(manifest_bytes.len()),
        );
        payload.extend_from_slice(b"tuffbox-transport-v2\0");
        payload.extend_from_slice(&(metadata.len() as u64).to_be_bytes());
        payload.extend_from_slice(&metadata);
        payload.extend_from_slice(manifest_bytes);
        Ok(payload)
    }
}

fn validate_transport_path(path: &str) -> Result<(), TransportMetaError> {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed.contains(':') {
        return Err(TransportMetaError::UnsafePath(path.to_string()));
    }
    let candidate = Path::new(trimmed);
    for component in candidate.components() {
        if matches!(
            component,
            Component::Prefix(_) | Component::RootDir | Component::ParentDir
        ) {
            return Err(TransportMetaError::UnsafePath(path.to_string()));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LocalTransportMeta {
    pub schema_version: u32,
    pub kind: TransportKind,
    pub repo: String,
    #[serde(default)]
    pub git_ref: Option<String>,
    pub manifest_file: String,
    pub installed_version: String,
    #[serde(default)]
    pub installed_commit_sha: Option<String>,
    #[serde(default)]
    pub installed_at: Option<String>,
    #[serde(default)]
    pub pinned_signer_public_key: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_transport_round_trip() {
        let meta = RepoTransportMeta {
            schema_version: TRANSPORT_SCHEMA_VERSION,
            manifest_file: "demo.tuffbox.json".into(),
            lockfile_file: "demo.tuffbox.lock.json".into(),
            pack_version: "1.2.3".into(),
            release_tag: Some("v1.2.3".into()),
            status: "ready".into(),
            release_assets: vec![ReleaseAssetRef {
                mod_id: "custom-lib".into(),
                file_name: "custom-lib.jar".into(),
                relative_path: "mods/custom-lib.jar".into(),
                sha512: "deadbeef".into(),
                size: 4,
            }],
            managed_files: vec!["pack.toml".into(), "index.toml".into()],
            content_digest: "abc".into(),
            signer_public_key: None,
            signature: None,
        };
        let json = serde_json::to_string_pretty(&meta).unwrap();
        let back: RepoTransportMeta = serde_json::from_str(&json).unwrap();
        assert_eq!(meta, back);
        assert!(back.is_ready());
    }
}
