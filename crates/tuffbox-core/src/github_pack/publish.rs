//! Publish a staged pack tree through GitHub Git Data API (one visible commit).

use crate::github_pack::client::{
    blob_bytes, create_blob, create_commit, create_tree, default_branch, git_blob_sha, patch_json,
    post_json, recursive_tree, ref_sha, update_ref, GitHubApi, GitHubError, TreeEntry,
};
use crate::github_pack::staging::StagedRepo;
use crate::github_pack::types::{RepoTransportMeta, REPO_TRANSPORT_FILE};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PublishError {
    #[error("{0}")]
    GitHub(#[from] GitHubError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("publish is a no-op (content digest unchanged)")]
    NoOp,
    #[error("branch update conflict: {0}")]
    Conflict(String),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishResult {
    pub commit_sha: String,
    pub branch: String,
    pub noop: bool,
    pub share_url: String,
    pub release_url: Option<String>,
    pub two_phase: bool,
}

/// Locally resolved view of a commit's tree. Cached so a single
/// `publish_staged_tree` call performs at most one `recursive_tree` request:
/// the same snapshot drives no-op detection, incremental blob skipping and the
/// chained "mark ready" commit of the two-phase flow.
#[derive(Debug, Clone)]
struct CommitTreeState {
    /// SHA of the commit itself (used as `create_commit` parent).
    commit_sha: String,
    /// Root tree SHA of the commit (`create_tree` base for child commits).
    root_tree_sha: String,
    /// Flat path -> blob SHA for every file under the commit tree.
    blobs: HashMap<String, String>,
    /// Managed-file list recorded in `.tuffbox/repo-transport.json`.
    managed_files: Vec<String>,
}

pub fn publish_staged_tree(
    api: &dyn GitHubApi,
    owner: &str,
    repo: &str,
    branch: Option<&str>,
    staged: &StagedRepo,
    message: &str,
) -> Result<PublishResult, PublishError> {
    let branch = match branch {
        Some(b) if !b.is_empty() => b.to_string(),
        _ => default_branch(api, owner, repo)?,
    };
    let git_ref = format!("heads/{branch}");
    let current_sha = ref_sha(api, owner, repo, &git_ref)?;

    // Fetch the parent commit's tree exactly once per publish; the snapshot
    // feeds no-op detection and every commit below. `recursive_tree` fails
    // loudly on truncated responses instead of silently dropping entries.
    let parent_state = match &current_sha {
        Some(sha) => {
            let tree = recursive_tree(api, owner, repo, sha)?;
            if remote_digest_matches(api, owner, repo, &tree, &staged.transport)? {
                return Err(PublishError::NoOp);
            }
            Some(resolve_commit_state(api, owner, repo, sha, &tree)?)
        }
        None => None,
    };

    let two_phase = !staged.pending_release_assets.is_empty();
    let (commit_sha, committed_state) = commit_tree(
        api,
        owner,
        repo,
        &git_ref,
        parent_state.as_ref(),
        staged,
        message,
    )
    .map_err(map_conflict)?;

    let mut release_url = None;
    if two_phase {
        let tag = staged
            .transport
            .release_tag
            .clone()
            .unwrap_or_else(|| format!("v{}", staged.transport.pack_version));
        let created = post_json(
            api,
            &format!("/repos/{owner}/{repo}/releases"),
            &json!({
                "tag_name": tag,
                "name": tag,
                "draft": true,
                "target_commitish": commit_sha,
            }),
        )?;
        let upload_url = created
            .get("upload_url")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        for asset in &staged.pending_release_assets {
            let bytes = fs::read(&asset.abs_path)?;
            api.upload_release_asset(upload_url, &asset.file_name, &bytes)?;
        }
        mark_transport_ready(&staged.root, &staged.transport)?;
        let restaged_files = reread_files_for_ready_marker(staged)?;
        commit_files(
            api,
            owner,
            repo,
            &git_ref,
            Some(&committed_state),
            &staged.root,
            &restaged_files,
            &format!("{message} (mark ready)"),
        )
        .map_err(map_conflict)?;
        if let Some(id) = created.get("id").and_then(|v| v.as_u64()) {
            let published = patch_json(
                api,
                &format!("/repos/{owner}/{repo}/releases/{id}"),
                &json!({ "draft": false }),
            )?;
            release_url = published
                .get("html_url")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
        }
    } else if let Some(tag) = &staged.transport.release_tag {
        match post_json(
            api,
            &format!("/repos/{owner}/{repo}/git/refs"),
            &json!({ "ref": format!("refs/tags/{tag}"), "sha": commit_sha }),
        ) {
            Ok(_) => {}
            Err(GitHubError::Conflict(msg)) => {
                return Err(PublishError::Conflict(format!("tag {tag}: {msg}")));
            }
            Err(e) => return Err(e.into()),
        }
    }

    let final_sha = ref_sha(api, owner, repo, &git_ref)?.unwrap_or(commit_sha);
    Ok(PublishResult {
        share_url: format!("https://github.com/{owner}/{repo}"),
        commit_sha: final_sha,
        branch,
        noop: false,
        release_url,
        two_phase,
    })
}

fn map_conflict(err: GitHubError) -> PublishError {
    match err {
        GitHubError::Conflict(msg) => PublishError::Conflict(msg),
        other => PublishError::GitHub(other),
    }
}

fn remote_digest_matches(
    api: &dyn GitHubApi,
    owner: &str,
    repo: &str,
    tree: &[TreeEntry],
    local: &RepoTransportMeta,
) -> Result<bool, PublishError> {
    if local.content_digest.is_empty() {
        return Ok(false);
    }
    let Some(entry) = tree.iter().find(|e| e.path == REPO_TRANSPORT_FILE) else {
        return Ok(false);
    };
    let bytes = blob_bytes(api, owner, repo, &entry.sha)?;
    let remote: RepoTransportMeta = serde_json::from_slice(&bytes)?;
    Ok(remote.content_digest == local.content_digest && remote.status == local.status)
}

/// Flattens a freshly fetched recursive tree into reusable commit state:
/// root tree SHA, per-path blob SHAs and the recorded managed-file list.
fn resolve_commit_state(
    api: &dyn GitHubApi,
    owner: &str,
    repo: &str,
    commit_sha: &str,
    tree: &[TreeEntry],
) -> Result<CommitTreeState, GitHubError> {
    let mut root_tree_sha = commit_sha.to_string();
    let mut blobs = HashMap::new();
    for entry in tree {
        if entry.kind == "tree" {
            if entry.path.is_empty() {
                root_tree_sha = entry.sha.clone();
            }
            continue;
        }
        blobs.insert(entry.path.clone(), entry.sha.clone());
    }
    let managed_files = previous_managed_files(api, owner, repo, tree)?;
    Ok(CommitTreeState {
        commit_sha: commit_sha.to_string(),
        root_tree_sha,
        blobs,
        managed_files,
    })
}

fn commit_tree(
    api: &dyn GitHubApi,
    owner: &str,
    repo: &str,
    git_ref: &str,
    parent_state: Option<&CommitTreeState>,
    staged: &StagedRepo,
    message: &str,
) -> Result<(String, CommitTreeState), GitHubError> {
    let paths: Vec<String> = staged
        .files
        .iter()
        .map(|f| f.relative_path.clone())
        .collect();
    commit_files(
        api,
        owner,
        repo,
        git_ref,
        parent_state,
        &staged.root,
        &paths,
        message,
    )
}

fn reread_files_for_ready_marker(staged: &StagedRepo) -> Result<Vec<String>, PublishError> {
    Ok(staged
        .files
        .iter()
        .map(|f| f.relative_path.clone())
        .collect())
}

fn mark_transport_ready(
    root: &std::path::Path,
    current: &RepoTransportMeta,
) -> Result<(), PublishError> {
    let path = root.join(REPO_TRANSPORT_FILE);
    let mut meta: RepoTransportMeta = if path.is_file() {
        serde_json::from_slice(&fs::read(&path)?)?
    } else {
        current.clone()
    };
    meta.status = "ready".into();
    fs::write(path, serde_json::to_vec_pretty(&meta)?)?;
    Ok(())
}

/// Commits `relative_paths` (read from `root`) on top of `parent_state`.
///
/// Blobs are content-addressed, so files whose local git blob SHA-1 already
/// appears in the parent tree are not uploaded again; obsolete managed files
/// are dropped from the tree via `sha: null` entries. Returns the new commit
/// SHA plus the resolved state of that commit so callers chain further commits
/// without additional `recursive_tree` requests.
// GitHub Data API calls carry owner/repo/ref plus tree inputs; splitting them
// into a params struct would not reduce coupling, only move the arguments.
#[allow(clippy::too_many_arguments)]
fn commit_files(
    api: &dyn GitHubApi,
    owner: &str,
    repo: &str,
    git_ref: &str,
    parent_state: Option<&CommitTreeState>,
    root: &std::path::Path,
    relative_paths: &[String],
    message: &str,
) -> Result<(String, CommitTreeState), GitHubError> {
    let mut blobs = parent_state.map(|s| s.blobs.clone()).unwrap_or_default();
    let mut tree_entries: Vec<Value> = Vec::new();
    for rel in relative_paths {
        let bytes = fs::read(root.join(rel)).map_err(|e| GitHubError::Request(e.to_string()))?;
        let local_sha = git_blob_sha(&bytes);
        if blobs.get(rel) == Some(&local_sha) {
            continue;
        }
        let sha = create_blob(api, owner, repo, &bytes)?;
        blobs.insert(rel.clone(), sha.clone());
        tree_entries.push(json!({
            "path": rel,
            "mode": "100644",
            "type": "blob",
            "sha": sha,
        }));
    }

    let base_tree = parent_state.map(|s| s.root_tree_sha.clone());
    let mut managed_files = parent_state
        .map(|s| s.managed_files.clone())
        .unwrap_or_default();
    if let Some(state) = parent_state {
        for old in &state.managed_files {
            if relative_paths.iter().any(|p| p == old) {
                continue;
            }
            tree_entries.push(json!({
                "path": old,
                "mode": "100644",
                "type": "blob",
                "sha": Value::Null,
            }));
            blobs.remove(old);
            managed_files.retain(|p| p != old);
        }
    }

    let tree_sha = create_tree(api, owner, repo, base_tree.as_deref(), &tree_entries)?;
    let parents: Vec<String> = parent_state
        .map(|s| vec![s.commit_sha.clone()])
        .unwrap_or_default();
    let commit_sha = create_commit(api, owner, repo, message, &tree_sha, &parents)?;
    update_ref(
        api,
        owner,
        repo,
        git_ref,
        &commit_sha,
        parent_state.is_none(),
    )?;
    for rel in relative_paths {
        if !managed_files.contains(rel) {
            managed_files.push(rel.clone());
        }
    }
    Ok((
        commit_sha.clone(),
        CommitTreeState {
            commit_sha,
            root_tree_sha: tree_sha,
            blobs,
            managed_files,
        },
    ))
}

fn previous_managed_files(
    api: &dyn GitHubApi,
    owner: &str,
    repo: &str,
    tree: &[TreeEntry],
) -> Result<Vec<String>, GitHubError> {
    let Some(entry) = tree.iter().find(|e| e.path == REPO_TRANSPORT_FILE) else {
        return Ok(Vec::new());
    };
    let bytes = blob_bytes(api, owner, repo, &entry.sha)?;
    let meta: RepoTransportMeta =
        serde_json::from_slice(&bytes).map_err(|e| GitHubError::Request(e.to_string()))?;
    Ok(meta.managed_files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github_pack::client::MockGitHub;
    use crate::github_pack::staging::{stage_repo_tree, StageOptions};
    use crate::github_pack::types::TRANSPORT_SCHEMA_VERSION;
    use crate::manifest::{
        ContentType, FileHashes, LoaderKind, LoaderSpec, MinecraftSpec, ModSource, ModSpec,
        ProjectManifest, ProjectMetadata, Side, SourceKind,
    };
    use std::fs;

    fn sample_manifest(version: &str, extra: Vec<ModSpec>) -> ProjectManifest {
        let mut mods = vec![ModSpec {
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
        }];
        mods.extend(extra);
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

    fn custom_jar_mod() -> ModSpec {
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
            version: "1.0.0".into(),
            file_name: Some("custom-lib.jar".into()),
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

    #[test]
    fn publish_creates_one_commit_and_noop_on_repeat() {
        let api = MockGitHub::new("acme", "demo");
        let project = tempfile::tempdir().unwrap();
        let staging = tempfile::tempdir().unwrap();
        let manifest = sample_manifest("1.0.0", vec![]);
        let manifest_path = project.path().join("demo.tuffbox.json");
        fs::write(&manifest_path, "{}").unwrap();
        let staged = stage_repo_tree(
            &manifest,
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
        let first = publish_staged_tree(&api, "acme", "demo", None, &staged, "v1").unwrap();
        assert!(!first.noop);
        assert!(!first.commit_sha.is_empty());
        let inspected = crate::github_pack::inspect_public_pack(&api, "acme", "demo").unwrap();
        assert_eq!(inspected["fullName"], "acme/demo");
        assert_eq!(inspected["ready"], true);
        let files = api.head_files();
        assert!(files.contains_key("pack.toml"));
        assert!(files.contains_key("demo.tuffbox.json"));
        let again = publish_staged_tree(&api, "acme", "demo", None, &staged, "v1-again");
        assert!(matches!(again, Err(PublishError::NoOp)));
    }

    #[test]
    fn publish_rejects_non_fast_forward() {
        let api = MockGitHub::new("acme", "demo");
        let project = tempfile::tempdir().unwrap();
        let staging = tempfile::tempdir().unwrap();
        let manifest = sample_manifest("1.0.0", vec![]);
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
        publish_staged_tree(&api, "acme", "demo", None, &staged, "v1").unwrap();
        api.fail_next_ref_update();
        let staging2 = tempfile::tempdir().unwrap();
        let manifest2 = sample_manifest("1.1.0", vec![]);
        let staged2 = stage_repo_tree(
            &manifest2,
            &manifest_path,
            staging2.path(),
            None,
            StageOptions::default(),
        )
        .unwrap();
        let err = publish_staged_tree(&api, "acme", "demo", None, &staged2, "v2").unwrap_err();
        assert!(matches!(err, PublishError::Conflict(_)));
    }

    #[test]
    fn two_phase_publish_uploads_oversized_jar() {
        let api = MockGitHub::new("acme", "demo");
        let project = tempfile::tempdir().unwrap();
        let staging = tempfile::tempdir().unwrap();
        fs::create_dir_all(project.path().join("mods")).unwrap();
        fs::write(
            project.path().join("mods/custom-lib.jar"),
            b"oversized-bytes",
        )
        .unwrap();
        let manifest = sample_manifest("2.0.0", vec![custom_jar_mod()]);
        let manifest_path = project.path().join("demo.tuffbox.json");
        fs::write(&manifest_path, "{}").unwrap();
        let staged = stage_repo_tree(
            &manifest,
            &manifest_path,
            staging.path(),
            None,
            StageOptions {
                owner: Some("acme".into()),
                repo: Some("demo".into()),
                custom_jar_git_limit: Some(4),
                ..StageOptions::default()
            },
        )
        .unwrap();
        assert!(!staged.pending_release_assets.is_empty());
        assert_eq!(staged.transport.status, "publishing");
        let result = publish_staged_tree(&api, "acme", "demo", None, &staged, "v2").unwrap();
        assert!(result.two_phase);
        assert!(api
            .uploaded_assets()
            .iter()
            .any(|n| n == "custom-lib-custom-lib.jar"));
        let files = api.head_files();
        let meta: RepoTransportMeta = serde_json::from_slice(&files[REPO_TRANSPORT_FILE]).unwrap();
        assert_eq!(meta.status, "ready");
        assert_eq!(meta.schema_version, TRANSPORT_SCHEMA_VERSION);
        assert!(!files.contains_key("mods/custom-lib.jar"));
    }

    #[test]
    fn identical_recommit_uploads_zero_new_blobs() {
        let api = MockGitHub::new("acme", "demo");
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("dir")).unwrap();
        fs::write(root.path().join("a.txt"), b"alpha").unwrap();
        fs::write(root.path().join("dir/b.txt"), b"beta").unwrap();
        let paths = vec!["a.txt".to_string(), "dir/b.txt".to_string()];

        let (_, state) = commit_files(
            &api,
            "acme",
            "demo",
            "heads/main",
            None,
            root.path(),
            &paths,
            "init",
        )
        .unwrap();
        assert_eq!(api.created_blob_count(), 2);

        // Same bytes on top of the previous commit: content-addressed dedup
        // must skip every upload.
        commit_files(
            &api,
            "acme",
            "demo",
            "heads/main",
            Some(&state),
            root.path(),
            &paths,
            "no changes",
        )
        .unwrap();
        assert_eq!(
            api.created_blob_count(),
            2,
            "identical recommit must not upload new blobs"
        );
        let head = api.head_files();
        assert_eq!(head.get("a.txt").unwrap(), &b"alpha".to_vec());
        assert_eq!(head.get("dir/b.txt").unwrap(), &b"beta".to_vec());
    }

    #[test]
    fn single_changed_file_uploads_exactly_one_new_blob() {
        let api = MockGitHub::new("acme", "demo");
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("dir")).unwrap();
        fs::write(root.path().join("a.txt"), b"alpha").unwrap();
        fs::write(root.path().join("dir/b.txt"), b"beta").unwrap();
        let paths = vec!["a.txt".to_string(), "dir/b.txt".to_string()];

        let (_, state) = commit_files(
            &api,
            "acme",
            "demo",
            "heads/main",
            None,
            root.path(),
            &paths,
            "init",
        )
        .unwrap();
        assert_eq!(api.created_blob_count(), 2);

        fs::write(root.path().join("dir/b.txt"), b"beta-v2").unwrap();
        commit_files(
            &api,
            "acme",
            "demo",
            "heads/main",
            Some(&state),
            root.path(),
            &paths,
            "one change",
        )
        .unwrap();
        assert_eq!(
            api.created_blob_count(),
            3,
            "exactly one changed file must produce exactly one new blob"
        );
        let head = api.head_files();
        assert_eq!(head.get("a.txt").unwrap(), &b"alpha".to_vec());
        assert_eq!(head.get("dir/b.txt").unwrap(), &b"beta-v2".to_vec());
    }

    #[test]
    fn obsolete_managed_files_are_deleted_via_null_entries() {
        let api = MockGitHub::new("acme", "demo");
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("dir")).unwrap();
        fs::write(root.path().join("a.txt"), b"alpha").unwrap();
        fs::write(root.path().join("dir/b.txt"), b"beta").unwrap();

        let (_, mut state) = commit_files(
            &api,
            "acme",
            "demo",
            "heads/main",
            None,
            root.path(),
            &["a.txt".to_string(), "dir/b.txt".to_string()],
            "init",
        )
        .unwrap();
        state.managed_files = vec!["a.txt".into(), "dir/b.txt".into()];

        commit_files(
            &api,
            "acme",
            "demo",
            "heads/main",
            Some(&state),
            root.path(),
            &["a.txt".to_string()],
            "drop b",
        )
        .unwrap();
        let head = api.head_files();
        assert!(head.contains_key("a.txt"));
        assert!(!head.contains_key("dir/b.txt"));
    }

    #[test]
    fn republish_of_identical_pack_is_noop_without_uploads() {
        let api = MockGitHub::new("acme", "demo");
        let project = tempfile::tempdir().unwrap();
        let manifest_path = project.path().join("demo.tuffbox.json");
        fs::write(&manifest_path, "{}").unwrap();
        let manifest = sample_manifest("1.0.0", vec![]);
        let staging = tempfile::tempdir().unwrap();
        let staged = stage_repo_tree(
            &manifest,
            &manifest_path,
            staging.path(),
            None,
            StageOptions::default(),
        )
        .unwrap();
        publish_staged_tree(&api, "acme", "demo", None, &staged, "v1").unwrap();
        let uploads_after_first = api.created_blob_count();
        assert!(uploads_after_first > 0);

        let staging2 = tempfile::tempdir().unwrap();
        let staged_again = stage_repo_tree(
            &manifest,
            &manifest_path,
            staging2.path(),
            None,
            StageOptions::default(),
        )
        .unwrap();
        let again = publish_staged_tree(&api, "acme", "demo", None, &staged_again, "v1-again");
        assert!(matches!(again, Err(PublishError::NoOp)));
        assert_eq!(
            api.created_blob_count(),
            uploads_after_first,
            "identical republish must not upload any blob"
        );
    }

    #[test]
    fn publish_performs_single_recursive_tree_request_per_call() {
        let api = MockGitHub::new("acme", "demo");
        let project = tempfile::tempdir().unwrap();
        let manifest_path = project.path().join("demo.tuffbox.json");
        fs::write(&manifest_path, "{}").unwrap();
        let staging = tempfile::tempdir().unwrap();
        let staged = stage_repo_tree(
            &sample_manifest("1.0.0", vec![]),
            &manifest_path,
            staging.path(),
            None,
            StageOptions::default(),
        )
        .unwrap();
        assert_eq!(api.tree_request_count(), 0);
        publish_staged_tree(&api, "acme", "demo", None, &staged, "v1").unwrap();
        assert_eq!(
            api.tree_request_count(),
            0,
            "fresh publish has no parent commit, so no recursive tree walk"
        );

        let staging2 = tempfile::tempdir().unwrap();
        let staged2 = stage_repo_tree(
            &sample_manifest("1.1.0", vec![]),
            &manifest_path,
            staging2.path(),
            None,
            StageOptions::default(),
        )
        .unwrap();
        publish_staged_tree(&api, "acme", "demo", None, &staged2, "v2").unwrap();
        assert_eq!(
            api.tree_request_count(),
            1,
            "update publish must reuse one parent-tree snapshot for no-op check and commit"
        );
    }

    #[test]
    fn two_phase_publish_also_performs_single_recursive_tree_request() {
        let api = MockGitHub::new("acme", "demo");
        let project = tempfile::tempdir().unwrap();
        let staging = tempfile::tempdir().unwrap();
        fs::create_dir_all(project.path().join("mods")).unwrap();
        fs::write(
            project.path().join("mods/custom-lib.jar"),
            b"oversized-bytes",
        )
        .unwrap();
        let manifest = sample_manifest("2.0.0", vec![custom_jar_mod()]);
        let manifest_path = project.path().join("demo.tuffbox.json");
        fs::write(&manifest_path, "{}").unwrap();
        let staged = stage_repo_tree(
            &manifest,
            &manifest_path,
            staging.path(),
            None,
            StageOptions {
                owner: Some("acme".into()),
                repo: Some("demo".into()),
                custom_jar_git_limit: Some(4),
                ..StageOptions::default()
            },
        )
        .unwrap();
        assert_eq!(api.tree_request_count(), 0);
        publish_staged_tree(&api, "acme", "demo", None, &staged, "v2").unwrap();
        assert_eq!(
            api.tree_request_count(),
            0,
            "fresh two-phase publish walks no remote tree; the mark-ready \
             commit chains locally off the first commit's resolved state"
        );
    }

    #[test]
    fn truncated_tree_response_fails_loudly_on_republish() {
        let api = MockGitHub::new("acme", "demo");
        let project = tempfile::tempdir().unwrap();
        let manifest_path = project.path().join("demo.tuffbox.json");
        fs::write(&manifest_path, "{}").unwrap();
        let staging = tempfile::tempdir().unwrap();
        let staged = stage_repo_tree(
            &sample_manifest("1.0.0", vec![]),
            &manifest_path,
            staging.path(),
            None,
            StageOptions::default(),
        )
        .unwrap();
        publish_staged_tree(&api, "acme", "demo", None, &staged, "v1").unwrap();

        api.emulate_truncated_tree();
        let staging2 = tempfile::tempdir().unwrap();
        let staged2 = stage_repo_tree(
            &sample_manifest("1.1.0", vec![]),
            &manifest_path,
            staging2.path(),
            None,
            StageOptions::default(),
        )
        .unwrap();
        let err = publish_staged_tree(&api, "acme", "demo", None, &staged2, "v2").unwrap_err();
        assert!(
            matches!(err, PublishError::GitHub(GitHubError::TruncatedTree { .. })),
            "truncated tree must surface as a loud error, not silent data loss"
        );
        // Nothing was committed on top of the partial read.
        let meta: RepoTransportMeta =
            serde_json::from_slice(&api.head_files()[REPO_TRANSPORT_FILE]).unwrap();
        assert_eq!(meta.pack_version, "1.0.0");
    }
}
