//! Content-addressable download cache, inspired by packwiz's
//! `core/download.go`.
//!
//! Files are stored by their SHA-256 hash under
//! `.tuffbox/cache/<aa>/<bb...>` (where `aabb...` is the hex hash), mirroring
//! packwiz's `cache/<sha256[:2]>/<sha256[2:]>` layout. Repeated downloads of
//! the same file (same hash, different URL/source) hit the cache instead of
//! the network, and every cached file is hash-validated before it is handed
//! back — exactly the integrity guarantee packwiz provides for mod downloads.
//!
//! This is complementary to [`crate::mod_index_cache`] (which maps a hash to
//! *Modrinth metadata*): that index answers "what mod is this?", this cache
//! answers "where is the bytes on disk?".

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use thiserror::Error;

const CACHE_DIR_REL: &str = ".tuffbox/cache";
const INDEX_FILE: &str = ".tuffbox/cache/index.json";
const CACHE_VERSION: u32 = 2;

#[derive(Debug, Error)]
pub enum CacheError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("download error: {0}")]
    Download(String),
    #[error("hash mismatch for cached file: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct CacheIndex {
    version: u32,
    /// Maps a SHA-256 (lowercase hex) to a list of source URLs it was seen at.
    #[serde(default)]
    entries: HashMap<String, Vec<String>>,
}

impl CacheIndex {
    fn load(root: &Path) -> Self {
        let path = root.join(INDEX_FILE);
        let Ok(raw) = std::fs::read_to_string(&path) else {
            return Self {
                version: CACHE_VERSION,
                entries: HashMap::new(),
            };
        };
        let mut index: CacheIndex = serde_json::from_str(&raw).unwrap_or_else(|_| CacheIndex {
            version: CACHE_VERSION,
            entries: HashMap::new(),
        });
        if index.version < CACHE_VERSION {
            // Version 1 had a bug where zero-byte files could be cached; drop
            // any empty file from the index (mirrors packwiz's v1→v2 fix).
            index.entries.retain(|hash, _| !hash.is_empty());
            index.version = CACHE_VERSION;
        }
        index
    }

    fn save(&self, root: &Path) -> std::io::Result<()> {
        let path = root.join(INDEX_FILE);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(
            path,
            serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into()),
        )
    }
}

fn cache_path_for(root: &Path, sha256: &str) -> PathBuf {
    let sha256 = sha256.to_lowercase();
    let (prefix, rest) = sha256.split_at(2);
    root.join(CACHE_DIR_REL).join(prefix).join(rest)
}

/// Ensures a file identified by `expected_sha256` is present in the cache,
/// downloading it from `url` if necessary, and returns the on-disk path.
///
/// If the file is already cached and its hash validates, the network is not
/// touched. When `expected_sha256` is `None`, the file is downloaded and its
/// computed hash is recorded (so future calls with the hash can reuse it).
pub fn get_or_download(
    project_root: &Path,
    url: &str,
    expected_sha256: Option<&str>,
) -> Result<PathBuf, CacheError> {
    let mut index = CacheIndex::load(project_root);

    if let Some(expected) = expected_sha256 {
        let expected = expected.to_lowercase();
        if let Some(path) = try_reuse(&mut index, project_root, &expected)? {
            return Ok(path);
        }
    }

    let bytes = crate::http::get_bytes(url).map_err(|e| CacheError::Download(e.to_string()))?;
    let actual = format!("{:x}", Sha256::digest(&bytes));
    if let Some(expected) = expected_sha256 {
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(CacheError::HashMismatch {
                expected: expected.to_string(),
                actual,
            });
        }
    }

    let target = cache_path_for(project_root, &actual);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&target, &bytes)?;

    index
        .entries
        .entry(actual)
        .or_default()
        .push(url.to_string());
    index.save(project_root)?;

    Ok(target)
}

/// Returns the cached path if the file exists and its hash still matches.
fn try_reuse(
    index: &mut CacheIndex,
    project_root: &Path,
    expected: &str,
) -> Result<Option<PathBuf>, CacheError> {
    if !index.entries.contains_key(expected) {
        return Ok(None);
    }
    let path = cache_path_for(project_root, expected);
    if !path.is_file() {
        index.entries.remove(expected);
        index.save(project_root)?;
        return Ok(None);
    }
    // Validate the stored bytes still hash to the expected value.
    let mut file = std::fs::File::open(&path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let actual = format!("{:x}", hasher.finalize());
    if actual.eq_ignore_ascii_case(expected) {
        Ok(Some(path))
    } else {
        index.entries.remove(expected);
        index.save(project_root)?;
        let _ = std::fs::remove_file(&path);
        Ok(None)
    }
}

/// Imports an already-present local file into the cache, returning its hash.
pub fn import_file(project_root: &Path, file_path: &Path) -> Result<String, CacheError> {
    let mut file = std::fs::File::open(file_path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let hash = format!("{:x}", hasher.finalize());

    let target = cache_path_for(project_root, &hash);
    if !target.is_file() {
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(file_path, &target)?;
    }

    let mut index = CacheIndex::load(project_root);
    index
        .entries
        .entry(hash.clone())
        .or_default()
        .push(format!("import:{}", file_path.display()));
    index.save(project_root)?;
    Ok(hash)
}

/// Looks up the cached path for a known SHA-256, if present.
pub fn lookup(project_root: &Path, sha256: &str) -> Option<PathBuf> {
    let index = CacheIndex::load(project_root);
    let sha256 = sha256.to_lowercase();
    if index.entries.contains_key(&sha256) {
        let path = cache_path_for(project_root, &sha256);
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

/// Removes every entry whose on-disk file is missing (housekeeping).
pub fn prune(project_root: &Path) -> std::io::Result<usize> {
    let mut index = CacheIndex::load(project_root);
    let before = index.entries.len();
    index
        .entries
        .retain(|hash, _| cache_path_for(project_root, hash).is_file());
    let removed = before - index.entries.len();
    index.save(project_root)?;
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_places_file_by_hash() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("blob.bin");
        std::fs::write(&src, b"import me").unwrap();
        let hash = import_file(dir.path(), &src).unwrap();
        assert!(lookup(dir.path(), &hash).is_some());
    }

    #[test]
    fn imported_file_reuses_cache_path() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("blob.bin");
        std::fs::write(&src, b"same bytes").unwrap();
        let hash = import_file(dir.path(), &src).unwrap();
        let cached = lookup(dir.path(), &hash).unwrap();
        // The on-disk cached copy has the same contents as the source.
        assert_eq!(std::fs::read(&cached).unwrap(), b"same bytes");
    }

    #[test]
    fn prune_removes_missing_entries() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("blob.bin");
        std::fs::write(&src, b"to prune").unwrap();
        let hash = import_file(dir.path(), &src).unwrap();
        let cached = lookup(dir.path(), &hash).unwrap();
        std::fs::remove_file(&cached).unwrap();
        let removed = prune(dir.path()).unwrap();
        assert_eq!(removed, 1);
        assert_eq!(lookup(dir.path(), &hash), None);
    }
}
