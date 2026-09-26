use sha2::{Digest, Sha512};
use std::fs;
use tuffbox_core::github_pack::{
    materialize_release_assets, GitHubApi, MockGitHub, RepoTransportMeta,
};

fn transport_meta(signer_public_key: Option<&str>, signature: Option<&str>) -> RepoTransportMeta {
    serde_json::from_value(serde_json::json!({
        "schemaVersion": 2,
        "manifestFile": "demo.tuffbox.json",
        "lockfileFile": "demo.tuffbox.lock.json",
        "packVersion": "1.0.0",
        "releaseTag": "v1.0.0",
        "status": "ready",
        "releaseAssets": [],
        "managedFiles": [],
        "contentDigest": "",
        "signerPublicKey": signer_public_key,
        "signature": signature,
    }))
    .unwrap()
}

fn transport_meta_with_asset(bytes: &[u8]) -> RepoTransportMeta {
    serde_json::from_value(serde_json::json!({
        "schemaVersion": 2,
        "manifestFile": "demo.tuffbox.json",
        "lockfileFile": "demo.tuffbox.lock.json",
        "packVersion": "1.0.0",
        "releaseTag": "v1.0.0",
        "status": "ready",
        "releaseAssets": [{
            "modId": "custom-lib",
            "fileName": "custom-lib.jar",
            "relativePath": "mods/custom-lib.jar",
            "sha512": hex::encode(Sha512::digest(bytes)),
            "size": bytes.len(),
        }],
        "managedFiles": [],
        "contentDigest": "",
    }))
    .unwrap()
}

#[test]
fn transport_validation_rejects_signer_without_signature() {
    let meta = transport_meta(Some("public-key"), None);

    assert!(meta.validate().is_err());
}

#[test]
fn transport_validation_rejects_signature_without_signer() {
    let meta = transport_meta(None, Some("signature"));

    assert!(meta.validate().is_err());
}

#[test]
fn transport_validation_rejects_paths_outside_project() {
    let mut meta = transport_meta(None, None);
    meta.managed_files = vec!["../outside.txt".into()];

    assert!(meta.validate().is_err());
}

#[test]
fn transport_validation_rejects_absolute_manifest_path() {
    let mut meta = transport_meta(None, None);
    meta.manifest_file = "C:/outside/demo.tuffbox.json".into();

    assert!(meta.validate().is_err());
}

#[test]
fn consumer_materializes_uploaded_release_asset_at_relative_path() {
    let api = MockGitHub::new("acme", "demo");
    let destination = tempfile::tempdir().unwrap();
    let bytes = b"trusted custom jar";
    let meta = transport_meta_with_asset(bytes);
    api.upload_release_asset("https://uploads.example/1/assets", "custom-lib.jar", bytes)
        .unwrap();

    materialize_release_assets(&api, "acme", "demo", destination.path(), &meta).unwrap();

    assert_eq!(
        fs::read(destination.path().join("mods/custom-lib.jar")).unwrap(),
        bytes
    );
}

#[test]
fn consumer_rejects_missing_release_asset() {
    let api = MockGitHub::new("acme", "demo");
    let destination = tempfile::tempdir().unwrap();
    let meta = transport_meta_with_asset(b"expected bytes");

    let result = materialize_release_assets(&api, "acme", "demo", destination.path(), &meta);

    assert!(result.is_err(), "missing release asset must fail");
}

#[test]
fn consumer_rejects_tampered_release_asset() {
    let api = MockGitHub::new("acme", "demo");
    let destination = tempfile::tempdir().unwrap();
    let meta = transport_meta_with_asset(b"expected bytes");
    api.upload_release_asset(
        "https://uploads.example/1/assets",
        "custom-lib.jar",
        b"tampered bytes",
    )
    .unwrap();

    let result = materialize_release_assets(&api, "acme", "demo", destination.path(), &meta);

    assert!(result.is_err(), "tampered release asset must fail");
    assert!(!destination.path().join("mods/custom-lib.jar").exists());
}
