//! GitHub Pack Transport — repo-native distribution of TuffBox packs.
//!
//! Isolated from TuffSwarm. Public repos only in v1.

mod apply;
mod client;
mod import;
mod publish;
mod source;
mod staging;
mod types;
mod update;

pub mod integrity;

pub use apply::{
    apply_managed_files, managed_from_transport, materialize_release_assets,
    remove_obsolete_content_files, verify_jar_hashes, verify_manifest_local_hashes, ApplyError,
    ReleaseAssetError,
};
pub use client::{
    commit_sha, inspect_public_pack, GitHubApi, GitHubError, LiveGitHubApi, MockGitHub,
};
pub use import::{extract_github_tarball, import_repo_tree, safe_relative_path, ImportError};
pub use publish::{publish_staged_tree, PublishError, PublishResult};
pub use source::{parse_github_source, validate_github_ref, GitHubSource, GitHubSourceError};
pub use staging::{
    content_digest, stage_repo_tree, PendingReleaseAsset, StageError, StageOptions, StagedFile,
    StagedRepo,
};
pub use types::{
    LocalTransportMeta, ReleaseAssetRef, RepoTransportMeta, TransportKind, TransportMetaError,
    TRANSPORT_SCHEMA_VERSION,
};
pub use update::{diff_manifests, update_available, PackChange, PackDiff, TrustOrigin};

pub use integrity::{
    pin_or_check_signer, sign_payload, verify_payload, Ed25519KeyPair, SignatureError,
};
