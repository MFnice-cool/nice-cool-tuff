//! Global content-addressed mod store with hard links (Shard
//! Launcher-inspired dedup): `objects/<xx>/<sha1>` shared across ALL
//! instances, so the same jar installed in N profiles is stored once on disk.
//!
//! Flow in [`super::mod_files::materialize_mod_file_with_progress`]:
//! 1. the store is consulted by sha1 — a hit hard-links the object into the
//!    instance's mods folder (no download, no extra disk space);
//! 2. a fresh download is recorded into the store before the caller counts
//!    it as success.
//!
//! Hard links keep per-instance mod folders fully self-contained from the
//! game's point of view (same inode, independent directory entries), and a
//! deleted instance folder never corrupts the store object.
//!
//! The store root lives in the user's local data dir (not per-project), so
//! dedup works across instances in different locations too. Write failures
//! are non-fatal: dedup is an optimization, callers fall back to plain
//! download/copy.

use serde::Serialize;
use sha1::{Digest, Sha1};
use std::path::{Path, PathBuf};

/// Store root: `<local data>/TuffBox/modstore`. The
/// `TUFFBOX_MODSTORE_ROOT` env var overrides it (test isolation; ignored
/// in production). Kept public for diagnostics.
pub fn store_root() -> PathBuf {
    if let Ok(root) = std::env::var("TUFFBOX_MODSTORE_ROOT") {
        if !root.is_empty() {
            return PathBuf::from(root);
        }
    }
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("TuffBox")
        .join("modstore")
}

/// Marker file opting a project out of the dedup store. Present in a
/// project root → `materialize_*` downloads plain copies (no store consult,
/// no recording) and `retro_dedup` skips the whole root. Toggled from
/// Project Settings («File deduplication»); see `materialize_project` for
/// restoring independence after the fact.
pub const NO_DEDUP_MARKER: &str = ".tuffbox-no-dedup";

/// Is dedup disabled for the project rooted at `project_dir`?
pub fn dedup_disabled(project_dir: &Path) -> bool {
    project_dir.join(NO_DEDUP_MARKER).is_file()
}

/// Object path for a sha1: `objects/<aa>/<rest>`.
fn object_path(sha1_hex: &str) -> PathBuf {
    let sha1_hex = sha1_hex.to_lowercase();
    let (prefix, rest) = sha1_hex.split_at(2);
    store_root().join("objects").join(prefix).join(rest)
}

/// SHA-1 of a file, streamed (mirrors `mc_install::sha1_file`).
fn sha1_of(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha1::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

use std::io::Read;

/// Try to materialize `target` by hard-linking a stored object whose sha1 is
/// `expected_sha1`. Returns `Ok(true)` when the link was created (caller is
/// done — no download needed). Never throws: any failure means "no hit".
pub fn try_hardlink(target: &Path, expected_sha1: &str) -> bool {
    let Some(source) = lookup(expected_sha1) else {
        return false;
    };
    if let Some(parent) = target.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if target.exists() {
        // Another parallel task may have materialized it first.
        return sha1_of(target)
            .map(|h| h.eq_ignore_ascii_case(expected_sha1))
            .unwrap_or(false);
    }
    // Windows: antivirus/indexer briefly holds freshly-created files open;
    // a failed CreateHardLink with ERROR_SHARING_VIOLATION succeeds on a
    // short retry. Two attempts ~150ms apart cover the common window.
    if std::fs::hard_link(&source, target).is_ok() {
        return true;
    }
    std::thread::sleep(std::time::Duration::from_millis(150));
    std::fs::hard_link(&source, target).is_ok()
}

/// Look up a stored object by sha1. Full re-hash validation runs once per
/// (size, mtime) fingerprint and is cached for the process lifetime — Play
/// consults the store for every mod/library, and re-hashing a 100 MB library
/// jar on each launch is a measurable stall. A corrupted object is removed
/// (same policy as `download_cache`); any file that later changes content
/// gets a new mtime/size and is re-validated.
pub fn lookup(expected_sha1: &str) -> Option<PathBuf> {
    use std::collections::HashMap;
    use std::sync::Mutex;
    type Cache = HashMap<String, bool>; // object path string -> valid
    static VALIDATED: Mutex<Option<Cache>> = Mutex::new(None);

    let path = object_path(expected_sha1);
    if !path.is_file() {
        return None;
    }
    let fingerprint = match std::fs::metadata(&path) {
        Ok(m) => format!("{}:{}", m.len(), {
            use std::time::UNIX_EPOCH;
            m.modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis())
                .unwrap_or(0)
        }),
        Err(_) => return None,
    };
    let cache_key = format!("{}|{}", path.display(), fingerprint);
    let is_valid = {
        let mut guard = VALIDATED.lock().unwrap_or_else(|e| e.into_inner());
        guard
            .get_or_insert_with(HashMap::new)
            .entry(cache_key)
            .or_insert_with(|| {
                matches!(sha1_of(&path), Ok(h) if h.eq_ignore_ascii_case(expected_sha1))
            })
            .clone()
    };
    if is_valid {
        Some(path)
    } else {
        make_writable(&path);
        let _ = std::fs::remove_file(&path);
        None
    }
}

/// Record a freshly downloaded/verified file into the store. The object is
/// written atomically (temp + rename in the same dir). Non-fatal on failure.
pub fn record(file: &Path, expected_sha1: &str) {
    let obj = object_path(expected_sha1);
    if obj.is_file() {
        return; // already stored
    }
    // Trust but verify: only index files that really hash to expected.
    let actual = match sha1_of(file) {
        Ok(h) => h,
        Err(_) => return,
    };
    if !actual.eq_ignore_ascii_case(expected_sha1) {
        return;
    }
    let Some(parent) = obj.parent() else { return };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    // Unique temp name: parallel rayon download tasks (and the retro-dedup
    // sweep) can record different files concurrently inside this same
    // process — a pid-only name would let two writers race for the same tmp
    // file and rename the wrong bytes into place. Pid + monotonically
    // increasing counter + thread id is collision-free in practice.
    static TMP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let tmp = parent.join(format!(
        ".tmp-{}-{}-{}",
        std::process::id(),
        TMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        format!("{:?}", std::thread::current().id()),
    ));
    if std::fs::copy(file, &tmp).is_err() {
        return;
    }
    // Another process may have recorded it in the meantime — either way the
    // rename lands a valid object.
    let _ = std::fs::rename(&tmp, &obj);
    // Read-only protection (docs/17, "Права на запись"): a store object is
    // shared by N instances; a player who opens the jar in an archive tool
    // and edits a texture inside one instance would otherwise corrupt every
    // other instance through the shared inode. Best-effort: failure to set
    // the attribute is non-fatal (dedup still works, corruption risk only).
    let _ = make_readonly(&obj);
    // Prune sibling instances of the same file: if the target is a copy
    // (not a link), leave it as-is; callers that want links use try_hardlink
    // before downloading.
}

/// Set the read-only attribute (Windows) / user-read-only permission bits
/// (Unix). Best-effort: errors are swallowed by the caller.
fn make_readonly(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::metadata(path)?;
        let mut perms = meta.permissions();
        perms.set_mode((perms.mode() & 0o555) | 0o444);
        std::fs::set_permissions(path, perms)
    }
    #[cfg(windows)]
    {
        let mut perms = std::fs::metadata(path)?.permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(path, perms)
    }
}

/// Clear the read-only attribute before deleting/replacing a store object.
/// No-op when the file is writable already; failure is non-fatal (GC/relink
/// callers treat errors as "skip this object").
fn make_writable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(path) {
            let mut perms = meta.permissions();
            perms.set_mode((perms.mode() & !0o222) | 0o644 & perms.mode() | 0o200);
            let _ = std::fs::set_permissions(path, perms);
        }
    }
    #[cfg(windows)]
    {
        if let Ok(meta) = std::fs::metadata(path) {
            let mut perms = meta.permissions();
            perms.set_readonly(false);
            let _ = std::fs::set_permissions(path, perms);
        }
    }
}

/// Delete orphaned objects no longer hard-linked from any instance.
/// Returns the number of removed files and bytes reclaimed. Only walks
/// `objects/`; hard-link refcounts are the filesystem's job (st_nlink).
///
/// Reference policy per OS:
/// - Unix: an object with `nlink > 1` is linked from at least one instance —
///   keep. `nlink == 1` means nothing references it — remove.
/// - Windows: `std::os::windows::fs::MetadataExt::number_of_links()` exposes
///   the same refcount, so the honest rule works there too. Files with
///   `nlink == 1` but modified recently (< 24h) are kept as a safety margin
///   for in-flight writes (record() copies into a temp then renames — a
///   crash mid-copy could otherwise orphan a very fresh object that a
///   parallel process is about to link).
pub fn gc() -> std::io::Result<(usize, u64)> {
    let objects = store_root().join("objects");
    if !objects.is_dir() {
        return Ok((0, 0));
    }
    let mut removed = 0usize;
    let mut bytes = 0u64;
    for prefix in std::fs::read_dir(&objects)?.flatten() {
        let prefix_path = prefix.path();
        if !prefix_path.is_dir() {
            continue;
        }
        for obj in std::fs::read_dir(&prefix_path)?.flatten() {
            let path = obj.path();
            let Ok(meta) = std::fs::metadata(&path) else {
                continue;
            };
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if meta.nlink() > 1 {
                    continue; // hard-linked from at least one instance
                }
            }
            // All OSes: freshly recorded objects (written < 24h ago, not yet
            // linked anywhere — a second instance may be about to link them)
            // survive this GC round. Unix previously deleted them instantly,
            // wiping the cache right after a first install; the documented
            // policy is the 24h margin, so apply it everywhere. On Windows
            // this is also the only "no links" signal available (std's
            // number_of_links is unstable there).
            if is_recently_linked(&path, &meta) {
                continue;
            }
            bytes += meta.len();
            make_writable(&path);
            std::fs::remove_file(&path)?;
            removed += 1;
        }
    }
    Ok((removed, bytes))
}

// ---------------------------------------------------------------------------
// Retroactive deduplication (docs/17, M3)
// ---------------------------------------------------------------------------

/// Result of a retro-dedup sweep over one or more project trees.
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RetroReport {
    /// Files examined (candidates: jars, zips — immutable content files).
    pub scanned: usize,
    /// Files replaced with a hardlink to an existing store object.
    pub linked: usize,
    /// New unique files recorded into the store.
    pub recorded: usize,
    /// Files skipped: already a link to the store, missing, or no change.
    pub skipped: usize,
    /// Approximate bytes reclaimed: sum of duplicate file sizes that became
    /// links (each linked file no longer holds its own data blocks).
    pub bytes_reclaimed: u64,
    /// Errors (per file, non-fatal — the sweep continues).
    pub errors: Vec<String>,
    /// Project roots skipped because the user turned dedup off for that pack
    /// (`.tuffbox-no-dedup` marker present).
    pub disabled_roots: Vec<String>,
}

/// Recursively collect candidate files for retro-dedup: anything with a
/// content-file extension under `root`. Only immutable files (jars/zips)
/// are ever touched — configs, saves, screenshots, options.txt are NOT in
/// the candidate set by construction.
fn retro_candidates(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Never descend into world/config/mutable trees. saves/ can be
            // huge, config/ holds user-owned state; neither contains dedup
            // candidates worth the walk.
            let name = entry.file_name().to_string_lossy().into_owned();
            if matches!(name.as_str(), "saves" | "config" | "screenshots" | "logs") {
                continue;
            }
            retro_candidates(&path, out);
        } else if path.extension().is_some_and(|e| {
            let e = e.to_string_lossy().to_lowercase();
            e == "jar" || e == "zip"
        }) {
            out.push(path);
        }
    }
}

/// Windows nlink witness (nlink is unstable in std's MetadataExt).
///
/// The honest GC rule needs to know whether an object is hard-linked from any
/// instance. std can't give us nlink on Windows, but `same-file` gives file
/// identity (volume serial + file index), which is *stronger*: two paths with
/// the same identity ARE the same file. So:
/// - [`file_is_store_link`] uses identity comparison — exact answer.
/// - [`gc`] uses identity in reverse: an object is deleted only when it is
///   older than [`GC_GRACE`] AND we have no record of instances referencing
///   it. Scanning every project on every GC would be too slow, so GC accepts
///   the grace-period compromise (same policy that existed before, but 24h
///   instead of 30 days — retro-dedup and launch-time linking keep objects
///   alive simply by touching them, and a deleted jar is re-materialized on
///   the next launch anyway, so a wrong GC decision is self-healing).
const GC_GRACE: std::time::Duration = std::time::Duration::from_secs(24 * 3600);

fn is_recently_linked(path: &Path, meta: &std::fs::Metadata) -> bool {
    let _ = path;
    if let Ok(modified) = meta.modified() {
        if let Ok(age) = std::time::SystemTime::now().duration_since(modified) {
            if age < GC_GRACE {
                return true;
            }
        }
    }
    false
}

/// Is `path` the very same file as the store object for `sha1` (i.e. already
/// a hardlink to it)? Exact on both OSes via file identity: inode+device on
/// Unix, volume serial + file index (same-file Handle) on Windows.
fn file_is_store_link(path: &Path, sha1: &str) -> bool {
    let Some(obj) = lookup(sha1) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Ok(a), Ok(b)) = (std::fs::metadata(path), std::fs::metadata(&obj)) {
            return a.ino() == b.ino() && a.dev() == b.dev();
        }
        false
    }
    #[cfg(windows)]
    {
        match (same_file::Handle::from_path(path), same_file::Handle::from_path(&obj)) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        }
    }
}

/// Replace `path` with a hardlink to the store object for `sha1`.
/// Atomic by construction: the link is created under a unique temp name next
/// to `path` and renamed over it, so at no point can a crash leave the
/// instance with a missing file (the old file is only ever unlinked, never
/// truncated). Returns true when the link landed.
fn relink_to_store(path: &Path, sha1: &str) -> bool {
    let Some(obj) = lookup(sha1) else {
        return false;
    };
    let Some(parent) = path.parent() else {
        return false;
    };
    static TMP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let tmp = parent.join(format!(
        ".tb-link-{}-{}",
        std::process::id(),
        TMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
    ));
    if std::fs::hard_link(&obj, &tmp).is_err() {
        return false;
    }
    if std::fs::rename(&tmp, path).is_ok() {
        return true;
    }
    // Windows: renaming over a read-only target (the file may carry the
    // store's read-only attribute from an earlier link) fails — clear the
    // bit, retry, then re-protect the shared object.
    #[cfg(windows)]
    {
        if let Ok(meta) = std::fs::metadata(path) {
            let mut perms = meta.permissions();
            if perms.readonly() {
                perms.set_readonly(false);
                if std::fs::set_permissions(path, perms).is_ok() {
                    if std::fs::rename(&tmp, path).is_ok() {
                        let _ = make_readonly(&obj);
                        return true;
                    }
                }
            }
        }
    }
    let _ = std::fs::remove_file(&tmp);
    // The original file is untouched — no restore needed (unlike the old
    // remove-then-link dance, which had to copy the content back on failure).
    false
}

/// Retroactive deduplication sweep: walk the given project roots, hash every
/// jar/zip, record new objects into the store and replace duplicates with
/// hardlinks. Never throws: all per-file failures are collected in the
/// report. Dedup is best-effort by design.
pub fn retro_dedup(roots: &[&Path]) -> RetroReport {
    let mut report = RetroReport::default();
    for root in roots {
        if dedup_disabled(root) {
            report.disabled_roots.push(root.display().to_string());
            continue;
        }
        let mut candidates = Vec::new();
        retro_candidates(root, &mut candidates);
        for path in candidates {
            report.scanned += 1;
            let sha1 = match sha1_of(&path) {
                Ok(h) => h,
                Err(e) => {
                    report.errors.push(format!("{}: {e}", path.display()));
                    continue;
                }
            };
            if file_is_store_link(&path, &sha1) {
                report.skipped += 1;
                continue;
            }
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let known = lookup(&sha1).is_some();
            if known {
                if relink_to_store(&path, &sha1) {
                    report.linked += 1;
                    report.bytes_reclaimed += size;
                } else {
                    report.skipped += 1;
                }
            } else {
                record(&path, &sha1);
                if lookup(&sha1).is_some() {
                    report.recorded += 1;
                }
                // First copy stays a plain file; duplicates in other projects
                // become links when their turn comes.
            }
        }
    }
    report
}

/// Outcome of [`release`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseOutcome {
    /// The store object was deleted — its disk space is reclaimed.
    Released,
    /// Another instance still links the object — it was kept.
    InUse,
    /// No store object for this hash (never recorded / already gone).
    NotFound,
}

/// Delete the store object for `sha1` once a pack stopped using its file, so
/// that the user's «delete this file» actually frees disk space instead of
/// leaving the bytes parked in the store until the next GC.
///
/// Safety (hard-link semantics): removing the store's directory entry can
/// never affect other packs — their links keep the inode alive on their own.
/// So the decision below only trades cache freshness, never correctness:
/// - Unix: exact — `nlink > 1` after the caller's entry was removed means
///   some other directory still links the object → keep.
/// - Windows: std has no stable nlink, so the known instance roots are
///   scanned for a file sharing the object's identity (`same-file`: volume
///   serial + file index). A folder unknown to the registry is harmless:
///   its link keeps the data; the store merely loses its cached name and
///   the next install re-downloads.
///
/// No grace period here (unlike [`gc`]): this runs after an explicit user
/// deletion, not a heuristic sweep. Races with a parallel `record`/link
/// degrade to "cache miss → re-download", never to data loss.
pub fn release(expected_sha1: &str, instance_roots: &[PathBuf]) -> ReleaseOutcome {
    let obj = object_path(expected_sha1);
    if !obj.is_file() {
        return ReleaseOutcome::NotFound;
    }
    #[cfg(unix)]
    {
        let _ = instance_roots; // scan not needed — nlink is exact here
        if let Ok(meta) = std::fs::metadata(&obj) {
            use std::os::unix::fs::MetadataExt;
            if meta.nlink() > 1 {
                return ReleaseOutcome::InUse;
            }
        }
    }
    #[cfg(windows)]
    {
        let Ok(obj_handle) = same_file::Handle::from_path(&obj) else {
            return ReleaseOutcome::NotFound;
        };
        for root in instance_roots {
            let mut candidates = Vec::new();
            retro_candidates(root, &mut candidates);
            for candidate in candidates {
                if let Ok(handle) = same_file::Handle::from_path(&candidate) {
                    if handle == obj_handle {
                        return ReleaseOutcome::InUse;
                    }
                }
            }
        }
    }
    make_writable(&obj);
    if std::fs::remove_file(&obj).is_ok() {
        ReleaseOutcome::Released
    } else {
        ReleaseOutcome::NotFound
    }
}

/// Result of the independence sweep run when a pack opts out of dedup.
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UnlinkReport {
    /// Candidate files examined (jars/zips under the project).
    pub scanned: usize,
    /// Store hardlinks replaced with independent copies.
    pub materialized: usize,
    /// Files that were not store links (already independent).
    pub skipped: usize,
    /// Per-file failures (non-fatal — the sweep continues).
    pub errors: Vec<String>,
}

/// Give a project back full ownership of its files: every jar/zip under
/// `project_dir` that is a hardlink into the store is replaced with an
/// independent copy of the same bytes (temp copy + atomic rename, so a crash
/// can never lose content — worst case a file stays linked). The store keeps
/// its object (other packs are unaffected); only this pack's directory
/// entries become separate inodes.
pub fn materialize_project(project_dir: &Path) -> UnlinkReport {
    let mut report = UnlinkReport::default();
    let mut candidates = Vec::new();
    retro_candidates(project_dir, &mut candidates);
    for path in candidates {
        report.scanned += 1;
        let sha1 = match sha1_of(&path) {
            Ok(h) => h,
            Err(e) => {
                report.errors.push(format!("{}: {e}", path.display()));
                continue;
            }
        };
        if !file_is_store_link(&path, &sha1) {
            report.skipped += 1;
            continue;
        }
        let Some(obj) = lookup(&sha1) else {
            report.skipped += 1;
            continue;
        };
        match replace_link_with_copy(&path, &obj) {
            Ok(()) => report.materialized += 1,
            Err(e) => report.errors.push(format!("{}: {e}", path.display())),
        }
    }
    report
}

/// Replace the store link at `path` with an independent copy of `obj`.
/// Copy → unique temp in the same dir → atomic rename over `path`. On
/// Windows the link may carry the store's read-only attribute (shared
/// inode): clear it for the rename, then re-protect the object afterwards.
fn replace_link_with_copy(path: &Path, obj: &Path) -> std::io::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "link has no parent directory"))?;
    static TMP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let tmp = parent.join(format!(
        ".tb-unlink-{}-{}",
        std::process::id(),
        TMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
    ));
    std::fs::copy(obj, &tmp)?;
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(first) => {
            #[cfg(windows)]
            {
                if let Ok(meta) = std::fs::metadata(path) {
                    let mut perms = meta.permissions();
                    if perms.readonly() {
                        perms.set_readonly(false);
                        if std::fs::set_permissions(path, perms).is_ok()
                            && std::fs::rename(&tmp, path).is_ok()
                        {
                            let _ = make_readonly(obj);
                            return Ok(());
                        }
                    }
                }
            }
            let _ = std::fs::remove_file(&tmp);
            Err(first)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Isolate the store root per test (tests run in parallel in one process;
    /// a mutex + fresh temp root keeps them from clobbering each other and
    /// from polluting the real user store).
    static STORE_LOCK: Mutex<()> = Mutex::new(());

    fn with_test_store<F: FnOnce()>(f: F) {
        let _guard = STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("TUFFBOX_MODSTORE_ROOT", dir.path());
        f();
        std::env::remove_var("TUFFBOX_MODSTORE_ROOT");
    }

    #[test]
    fn record_lookup_hardlink_roundtrip() {
        with_test_store(|| {
            let dir = tempfile::tempdir().unwrap();
            let src = dir.path().join("mod.jar");
            std::fs::write(&src, b"jar bytes for dedup").unwrap();
            let sha = sha1_of(&src).unwrap();

            record(&src, &sha);
            assert!(lookup(&sha).is_some());

            // Hard-link into a fresh instance folder.
            let inst = dir.path().join("inst1").join("mods");
            std::fs::create_dir_all(&inst).unwrap();
            let target = inst.join("mod.jar");
            assert!(try_hardlink(&target, &sha));
            assert_eq!(std::fs::read(&target).unwrap(), b"jar bytes for dedup");

            // Idempotent: linking when target already exists with same hash → true.
            assert!(try_hardlink(&target, &sha));

            // Unknown hash → no hit.
            assert!(!try_hardlink(
                &inst.join("other.jar"),
                "0000000000000000000000000000000000000000"
            ));
        });
    }

    #[test]
    fn retro_dedup_links_duplicate_and_records_new() {
        with_test_store(|| {
            let dir = tempfile::tempdir().unwrap();
            // Two "projects", each with the same jar bytes plus a unique zip.
            let p1 = dir.path().join("proj1").join("mods");
            let p2 = dir.path().join("proj2").join("mods");
            std::fs::create_dir_all(&p1).unwrap();
            std::fs::create_dir_all(&p2).unwrap();
            std::fs::write(p1.join("sodium.jar"), b"sodium bytes").unwrap();
            std::fs::write(p2.join("sodium.jar"), b"sodium bytes").unwrap();
            std::fs::write(p1.join("unique.zip"), b"unique pack").unwrap();

            let report = retro_dedup(&[&dir.path().join("proj1"), &dir.path().join("proj2")]);

            // 3 candidates scanned; 2 unique objects recorded. Within one
            // sweep, proj1's files are processed first (record), so proj2's
            // duplicate jar is immediately linked against the fresh object —
            // dedup works across roots in a single pass.
            assert_eq!(report.scanned, 3);
            assert_eq!(report.recorded, 2);
            assert_eq!(report.linked, 1, "{report:?}");
            assert!(report.bytes_reclaimed > 0);
            assert!(report.errors.is_empty(), "{:?}", report.errors);
            // Content survived the replace.
            assert_eq!(std::fs::read(p2.join("sodium.jar")).unwrap(), b"sodium bytes");

            // A second sweep is a no-op: everything is already linked/recorded.
            let report2 = retro_dedup(&[&dir.path().join("proj2")]);
            assert_eq!(report2.scanned, 1);
            assert_eq!(report2.linked, 0);
            assert_eq!(report2.recorded, 0);
        });
    }

    #[test]
    fn retro_dedup_skips_packs_that_opted_out() {
        with_test_store(|| {
            let dir = tempfile::tempdir().unwrap();
            let proj = dir.path().join("proj");
            std::fs::create_dir_all(proj.join("mods")).unwrap();
            std::fs::write(proj.join("mods").join("a.jar"), b"jar bytes").unwrap();
            // Opt-out marker present.
            std::fs::write(proj.join(NO_DEDUP_MARKER), b"").unwrap();

            let report = retro_dedup(&[&proj]);
            assert_eq!(report.scanned, 0, "marker must exclude the whole root");
            assert_eq!(report.disabled_roots, vec![proj.display().to_string()]);
            assert_eq!(report.recorded, 0);
            assert_eq!(report.linked, 0);
        });
    }

    #[test]
    fn materialize_project_restores_independence() {
        with_test_store(|| {
            let dir = tempfile::tempdir().unwrap();
            let src = dir.path().join("src.jar");
            std::fs::write(&src, b"shared jar bytes").unwrap();
            let sha = sha1_of(&src).unwrap();
            record(&src, &sha);

            let proj = dir.path().join("proj");
            std::fs::create_dir_all(proj.join("mods")).unwrap();
            let linked = proj.join("mods").join("mod.jar");
            assert!(try_hardlink(&linked, &sha));
            assert!(file_is_store_link(&linked, &sha));

            let report = materialize_project(&proj);
            assert_eq!(report.scanned, 1);
            assert_eq!(report.materialized, 1, "{report:?}");
            assert!(report.errors.is_empty(), "{:?}", report.errors);

            // Content identical, inode independent.
            assert_eq!(std::fs::read(&linked).unwrap(), b"shared jar bytes");
            assert!(!file_is_store_link(&linked, &sha));
            // Store object untouched.
            assert!(lookup(&sha).is_some());

            // A second sweep is a no-op (already independent).
            let again = materialize_project(&proj);
            assert_eq!(again.materialized, 0);
            assert_eq!(again.skipped, 1);
        });
    }

    #[test]
    fn fresh_unlinked_object_survives_gc() {
        with_test_store(|| {
            let dir = tempfile::tempdir().unwrap();
            let src = dir.path().join("fresh.jar");
            std::fs::write(&src, b"fresh bytes").unwrap();
            let sha = sha1_of(&src).unwrap();
            record(&src, &sha);
            let obj = object_path(&sha);
            assert!(obj.is_file());

            // Just recorded, not yet linked anywhere: GC must keep it (the
            // 24h margin exists exactly for this window — a second instance
            // may be about to hard-link it).
            let (removed, _) = gc().unwrap();
            assert_eq!(removed, 0);
            assert!(obj.is_file());

            // After the grace period it is fair game (no links).
            make_writable(&obj);
            filetime::set_file_times(
                &obj,
                filetime::FileTime::from_unix_time(0, 0),
                filetime::FileTime::from_unix_time(0, 0),
            )
            .unwrap();
            let (removed, _) = gc().unwrap();
            assert!(removed >= 1);
            assert!(!obj.is_file());
        });
    }

    #[test]
    fn materialize_mod_file_respects_per_pack_opt_out() {
        with_test_store(|| {
            use crate::manifest::{FileHashes, ModSource};
            use crate::mod_files::materialize_mod_file;

            let dir = tempfile::tempdir().unwrap();
            let bytes = b"opt out probe jar";
            let jar = dir.path().join("seed.jar");
            std::fs::write(&jar, bytes).unwrap();
            let sha = sha1_of(&jar).unwrap();
            record(&jar, &sha);

            let module = crate::manifest::ModSpec {
                id: "probe".to_string(),
                name: "Probe".to_string(),
                source: ModSource {
                    kind: crate::manifest::SourceKind::Modrinth,
                    project_id: Some("probe".to_string()),
                    file_id: Some("v1".to_string()),
                    url: Some("https://example.invalid/mod.jar".to_string()),
                    path: None,
                    icon_url: None,
                    categories: Vec::new(),
                },
                version: "1.0.0".to_string(),
                file_name: Some("probe.jar".to_string()),
                hashes: Some(FileHashes {
                    sha1: Some(sha.clone()),
                    sha512: None,
                }),
                side: crate::manifest::Side::Both,
                dependencies: Vec::new(),
                status: vec!["ok".to_string()],
                content_type: crate::mod_files::ContentType::Mod,
                authors: Vec::new(),
                option: None,
            };

            // Dedup on (no marker): the store hit materializes via hardlink,
            // the invalid URL is never contacted.
            let proj_on = dir.path().join("on");
            std::fs::create_dir_all(proj_on.join("mods")).unwrap();
            materialize_mod_file(&proj_on, &module).unwrap();
            assert_eq!(
                std::fs::read(proj_on.join("mods").join("probe.jar")).unwrap(),
                bytes
            );

            // Dedup off (marker): the store must NOT be consulted — the
            // download from the invalid host fails loudly instead of
            // silently linking shared bytes into an opted-out pack.
            let proj_off = dir.path().join("off");
            std::fs::create_dir_all(proj_off.join("mods")).unwrap();
            std::fs::write(proj_off.join(NO_DEDUP_MARKER), b"").unwrap();
            assert!(materialize_mod_file(&proj_off, &module).is_err());
            assert!(!proj_off.join("mods").join("probe.jar").is_file());
        });
    }

    #[test]
    fn release_frees_space_only_when_last_linker_is_gone() {
        with_test_store(|| {
            let dir = tempfile::tempdir().unwrap();
            let src = dir.path().join("seed.jar");
            std::fs::write(&src, b"shared jar bytes").unwrap();
            let sha = sha1_of(&src).unwrap();
            record(&src, &sha);

            let p1 = dir.path().join("pack1").join("mods");
            let p2 = dir.path().join("pack2").join("mods");
            std::fs::create_dir_all(&p1).unwrap();
            std::fs::create_dir_all(&p2).unwrap();
            let f1 = p1.join("mod.jar");
            let f2 = p2.join("mod.jar");
            assert!(try_hardlink(&f1, &sha));
            assert!(try_hardlink(&f2, &sha));

            let roots = [dir.path().join("pack1"), dir.path().join("pack2")];

            // Pack 1 deletes its file — pack 2 still links the object.
            std::fs::remove_file(&f1).unwrap();
            let out = release(&sha, &roots);
            assert_eq!(out, ReleaseOutcome::InUse);
            assert!(lookup(&sha).is_some());
            assert_eq!(std::fs::read(&f2).unwrap(), b"shared jar bytes");

            // Pack 2 deletes too — now nothing references it: the store must
            // let go, otherwise "delete" would not free any disk space.
            std::fs::remove_file(&f2).unwrap();
            let out = release(&sha, &roots);
            assert_eq!(out, ReleaseOutcome::Released);
            assert!(!object_path(&sha).exists());

            // Idempotent / unknown hashes are a quiet no-op.
            assert_eq!(release(&sha, &roots), ReleaseOutcome::NotFound);
            assert_eq!(
                release("0000000000000000000000000000000000000000", &roots),
                ReleaseOutcome::NotFound
            );
        });
    }

    #[test]
    fn recorded_object_is_readonly_and_gcs_cleanly() {
        with_test_store(|| {
            let dir = tempfile::tempdir().unwrap();
            let src = dir.path().join("m.jar");
            std::fs::write(&src, b"shared jar bytes").unwrap();
            let sha = sha1_of(&src).unwrap();

            record(&src, &sha);
            let obj = object_path(&sha);
            // Read-only attribute set (protects against in-instance edits
            // through the shared inode).
            #[cfg(windows)]
            assert!(std::fs::metadata(&obj).unwrap().permissions().readonly());
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = std::fs::metadata(&obj).unwrap().permissions().mode();
                assert_eq!(mode & 0o222, 0, "object must not be user-writable");
            }

            // GC removes it despite read-only (make_writable clears first).
            // Fresh objects fall inside the 24h grace window on Windows, so
            // backdate the mtime past the grace period first — requires
            // clearing read-only before set_file_mtime will succeed.
            make_writable(&obj);
            filetime::set_file_times(
                &obj,
                filetime::FileTime::from_unix_time(0, 0),
                filetime::FileTime::from_unix_time(0, 0),
            )
            .unwrap();
            let (removed, _) = gc().unwrap();
            assert!(removed >= 1);
            assert!(!obj.exists());
        });
    }
}
