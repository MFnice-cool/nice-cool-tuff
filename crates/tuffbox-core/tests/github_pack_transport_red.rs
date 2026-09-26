use sha2::{Digest, Sha512};
use std::fs;
use tuffbox_core::github_pack::{
    extract_github_tarball, import_repo_tree, stage_repo_tree, verify_manifest_local_hashes,
    Ed25519KeyPair, StageOptions,
};
use tuffbox_core::manifest::{
    ContentType, FileHashes, LoaderKind, LoaderSpec, MinecraftSpec, ModSource, ModSpec,
    ProjectManifest, ProjectMetadata, Side, SourceKind,
};

fn manifest_with_mod(source_kind: SourceKind, file_name: &str, sha512: &str) -> ProjectManifest {
    ProjectManifest {
        schema_version: tuffbox_core::manifest::CURRENT_PROJECT_SCHEMA_VERSION.into(),
        project: ProjectMetadata {
            id: "demo".into(),
            name: "Demo Pack".into(),
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
        mods: vec![ModSpec {
            id: "custom-lib".into(),
            name: "Custom Lib".into(),
            source: ModSource {
                kind: source_kind,
                project_id: None,
                file_id: None,
                url: None,
                path: Some(format!("mods/{file_name}")),
                icon_url: None,
                categories: vec![],
            },
            version: "1.0.0".into(),
            file_name: Some(file_name.into()),
            hashes: Some(FileHashes {
                sha1: None,
                sha512: Some(sha512.into()),
            }),
            side: Side::Both,
            dependencies: vec![],
            status: vec![],
            content_type: ContentType::Mod,
            authors: vec![],
            option: None,
        }],
        overrides: None,
    }
}

fn github_tarball_with_link(entry_type: tar::EntryType) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let encoder = flate2::write::GzEncoder::new(&mut bytes, flate2::Compression::default());
        let mut builder = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(entry_type);
        header.set_size(0);
        header.set_mode(0o777);
        header.set_cksum();
        builder
            .append_link(
                &mut header,
                "acme-demo-deadbeef/mods/linked.jar",
                "acme-demo-deadbeef/mods/target.jar",
            )
            .unwrap();
        builder.finish().unwrap();
    }
    bytes
}

#[test]
fn import_rejects_symlink_tar_entry() {
    let dest = tempfile::tempdir().unwrap();
    let archive = github_tarball_with_link(tar::EntryType::Symlink);

    let result = extract_github_tarball(&archive, dest.path());

    assert!(result.is_err(), "symlink tar entries must be rejected");
}

#[test]
fn import_rejects_hardlink_tar_entry() {
    let dest = tempfile::tempdir().unwrap();
    let archive = github_tarball_with_link(tar::EntryType::Link);

    let result = extract_github_tarball(&archive, dest.path());

    assert!(result.is_err(), "hardlink tar entries must be rejected");
}

#[test]
fn import_rejects_missing_signed_manifest_sidecar() {
    let project = tempfile::tempdir().unwrap();
    let staging = tempfile::tempdir().unwrap();
    let manifest = manifest_with_mod(SourceKind::Local, "custom-lib.jar", "00");
    let manifest_path = project.path().join("demo.tuffbox.json");
    fs::write(&manifest_path, "{}").unwrap();
    stage_repo_tree(
        &manifest,
        &manifest_path,
        staging.path(),
        None,
        StageOptions {
            signer: Some(Ed25519KeyPair::generate()),
            ..StageOptions::default()
        },
    )
    .unwrap();
    fs::remove_file(staging.path().join("demo.tuffbox.json")).unwrap();

    let error = import_repo_tree(staging.path()).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("signed manifest sidecar missing"),
        "unexpected import error: {error}"
    );
}

#[test]
fn stage_records_release_asset_destination_hash_and_size() {
    let project = tempfile::tempdir().unwrap();
    let staging = tempfile::tempdir().unwrap();
    let asset_bytes = b"oversized custom jar";
    fs::create_dir_all(project.path().join("mods")).unwrap();
    fs::write(project.path().join("mods/custom-lib.jar"), asset_bytes).unwrap();
    let expected_sha512 = hex::encode(Sha512::digest(asset_bytes));
    let manifest = manifest_with_mod(SourceKind::Local, "custom-lib.jar", &expected_sha512);
    let manifest_path = project.path().join("demo.tuffbox.json");
    fs::write(&manifest_path, "{}").unwrap();

    let staged = stage_repo_tree(
        &manifest,
        &manifest_path,
        staging.path(),
        None,
        StageOptions {
            custom_jar_git_limit: Some(4),
            ..StageOptions::default()
        },
    )
    .unwrap();
    let metadata = serde_json::to_value(&staged.transport.release_assets[0]).unwrap();

    assert_eq!(metadata["relativePath"], "mods/custom-lib.jar");
    assert_eq!(metadata["sha512"], expected_sha512);
    assert_eq!(metadata["size"], asset_bytes.len() as u64);
    assert_eq!(metadata["modId"], "custom-lib");
    assert!(staged
        .transport
        .managed_files
        .iter()
        .any(|path| path == "mods/custom-lib.jar"));
}

#[test]
fn signed_transport_rejects_repo_file_tampering() {
    let project = tempfile::tempdir().unwrap();
    let staging = tempfile::tempdir().unwrap();
    let manifest = manifest_with_mod(SourceKind::Local, "custom-lib.jar", "00");
    let manifest_path = project.path().join("demo.tuffbox.json");
    fs::write(&manifest_path, "{}").unwrap();
    stage_repo_tree(
        &manifest,
        &manifest_path,
        staging.path(),
        None,
        StageOptions {
            signer: Some(Ed25519KeyPair::generate()),
            ..StageOptions::default()
        },
    )
    .unwrap();
    fs::write(staging.path().join("README.md"), b"tampered").unwrap();

    assert!(import_repo_tree(staging.path()).is_err());
}

#[test]
fn missing_custom_file_with_declared_hash_is_an_error() {
    let project = tempfile::tempdir().unwrap();
    let manifest = manifest_with_mod(SourceKind::Local, "missing.jar", "00");

    let result = verify_manifest_local_hashes(project.path(), &manifest);

    assert!(result.is_err(), "missing custom file with a hash must fail");
}

#[test]
fn missing_github_file_with_declared_hash_is_an_error() {
    let project = tempfile::tempdir().unwrap();
    let manifest = manifest_with_mod(SourceKind::Github, "missing.jar", "00");

    let result = verify_manifest_local_hashes(project.path(), &manifest);

    assert!(result.is_err(), "missing GitHub file with a hash must fail");
}
