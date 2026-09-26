//! Resumable multi-download engine (patterns inspired by GDLauncher carbon_net; original code).
//!
//! - Stable `{name}.tuffbox.part` beside destination for HTTP Range resume
//! - SHA-1 / MD5 / SHA-256 verify before rename
//! - Stall timeout between successful reads
//! - Bounded concurrency for batch jobs

use crate::http::{host_from_url, CircuitBreakerOpen};
use md5::{Digest, Md5};
use reqwest::StatusCode;
use sha1::Sha1;
use sha2::Sha256;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const DEFAULT_STALL: Duration = Duration::from_secs(120);
const PART_SUFFIX: &str = ".tuffbox.part";
static CONFIGURED_CONCURRENCY: AtomicUsize = AtomicUsize::new(8);

/// Global concurrency used by Minecraft asset/library batch installs.
pub fn set_configured_concurrency(n: usize) {
    CONFIGURED_CONCURRENCY.store(n.clamp(1, 64), Ordering::Relaxed);
}

pub fn configured_concurrency() -> usize {
    CONFIGURED_CONCURRENCY.load(Ordering::Relaxed).max(1)
}

#[derive(Debug, thiserror::Error)]
pub enum DownloadEngineError {
    #[error("network error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },
    #[error("download stalled: no bytes for {secs}s")]
    Stalled { secs: u64 },
    #[error("circuit breaker open for {host}: retry in {retry_after_secs}s")]
    CircuitOpen { host: String, retry_after_secs: u64 },
    #[error("{0}")]
    Batch(String),
}

impl From<CircuitBreakerOpen> for DownloadEngineError {
    fn from(e: CircuitBreakerOpen) -> Self {
        Self::CircuitOpen {
            host: e.host,
            retry_after_secs: e.retry_after_secs,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChecksumKind {
    Sha1,
    Md5,
    Sha256,
}

impl ChecksumKind {
    pub fn hash_file(self, path: &Path) -> Result<String, DownloadEngineError> {
        let mut file = std::fs::File::open(path)?;
        let mut buf = vec![0u8; 64 * 1024];
        match self {
            ChecksumKind::Sha1 => {
                use sha1::Digest;
                let mut h = Sha1::new();
                loop {
                    let n = file.read(&mut buf)?;
                    if n == 0 {
                        break;
                    }
                    h.update(&buf[..n]);
                }
                Ok(format!("{:x}", h.finalize()))
            }
            ChecksumKind::Md5 => {
                let mut h = Md5::new();
                loop {
                    let n = file.read(&mut buf)?;
                    if n == 0 {
                        break;
                    }
                    h.update(&buf[..n]);
                }
                Ok(format!("{:x}", h.finalize()))
            }
            ChecksumKind::Sha256 => {
                use sha2::Digest;
                let mut h = Sha256::new();
                loop {
                    let n = file.read(&mut buf)?;
                    if n == 0 {
                        break;
                    }
                    h.update(&buf[..n]);
                }
                Ok(format!("{:x}", h.finalize()))
            }
        }
    }
}

pub fn part_path_for(dest: &Path) -> PathBuf {
    let name = dest
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("download");
    dest.with_file_name(format!("{name}{PART_SUFFIX}"))
}

fn download_client() -> Result<&'static reqwest::blocking::Client, DownloadEngineError> {
    use std::sync::OnceLock;
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    Ok(CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            // Large jars: allow long transfers; stall logic covers hung sockets.
            .timeout(Duration::from_secs(600))
            .connect_timeout(Duration::from_secs(20))
            .tcp_keepalive(Duration::from_secs(15))
            .tcp_nodelay(true)
            // Modrinth CDN (and most mod jars) come from a handful of hosts —
            // raise per-host connection pool so parallel streams don't queue.
            .pool_max_idle_per_host(32)
            .user_agent(concat!("TuffBox/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("download client")
    }))
}

/// Resume-friendly download to `dest` via `{dest}.tuffbox.part`.
/// Wraps the single-attempt transfer with bounded retries (Modrinth-style):
/// transient HTTP/stall failures resume from the `.part` bytes instead of
/// failing the whole file. Checksum mismatches are NOT retried here — the
/// caller has a hash-lookup redirect fallback for that.
pub fn download_resumable(
    url: &str,
    dest: &Path,
    expected: Option<(&str, ChecksumKind)>,
    progress: Option<Box<dyn FnMut(u64, u64) + Send>>,
    stall_timeout: Option<Duration>,
) -> Result<(), DownloadEngineError> {
    const MAX_ATTEMPTS: u32 = 3;
    let mut progress = progress;
    let mut attempt: u32 = 0;
    loop {
        attempt += 1;
        match download_resumable_once(url, dest, expected, &mut progress, stall_timeout) {
            Ok(()) => return Ok(()),
            // Transient failures: the .part file survives, so the next attempt
            // resumes from what already landed (Range request).
            Err(e @ (DownloadEngineError::Http(_) | DownloadEngineError::Stalled { .. }))
                if attempt < MAX_ATTEMPTS =>
            {
                std::thread::sleep(Duration::from_millis(400 * (1 << (attempt - 1))));
                let _ = e;
            }
            Err(e) => return Err(e),
        }
    }
}

fn download_resumable_once(
    url: &str,
    dest: &Path,
    expected: Option<(&str, ChecksumKind)>,
    progress: &mut Option<Box<dyn FnMut(u64, u64) + Send>>,
    stall_timeout: Option<Duration>,
) -> Result<(), DownloadEngineError> {
    let stall = stall_timeout.unwrap_or(DEFAULT_STALL);
    let host = host_from_url(url);
    crate::http::circuit_check(host)?;

    if dest.exists() {
        if let Some((exp, kind)) = expected {
            let actual = kind.hash_file(dest)?;
            if actual.eq_ignore_ascii_case(exp) {
                return Ok(());
            }
        } else {
            return Ok(());
        }
    }

    let parent = dest.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let part = part_path_for(dest);
    let mut existing = if part.is_file() {
        std::fs::metadata(&part)?.len()
    } else {
        0
    };

    let client = download_client()?;
    let mut attempt_url = url.to_string();
    let mut redirects = 0u8;

    loop {
        let mut req = client.get(&attempt_url);
        if existing > 0 {
            req = req.header(reqwest::header::RANGE, format!("bytes={existing}-"));
        }
        let response = req.send()?;
        let status = response.status();

        if status.is_redirection() {
            if let Some(loc) = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
            {
                redirects += 1;
                if redirects > 8 {
                    return Err(DownloadEngineError::Batch("too many redirects".into()));
                }
                attempt_url = if loc.starts_with("http") {
                    loc.to_string()
                } else {
                    // relative — rare for CDNs
                    loc.to_string()
                };
                continue;
            }
        }

        if status == StatusCode::RANGE_NOT_SATISFIABLE {
            let _ = std::fs::remove_file(&part);
            existing = 0;
            continue;
        }

        if existing > 0 && status == StatusCode::OK {
            let _ = std::fs::remove_file(&part);
            existing = 0;
            // Restart without Range on this same URL.
            continue;
        }

        if !status.is_success() && status != StatusCode::PARTIAL_CONTENT {
            crate::http::circuit_record_failure(host);
            return Err(DownloadEngineError::Http(
                response.error_for_status().unwrap_err(),
            ));
        }

        crate::http::circuit_record_success(host);
        return write_body(
            response,
            dest,
            &part,
            existing,
            expected,
            progress.as_mut(),
            stall,
            host,
        );
    }
}

fn write_body(
    response: reqwest::blocking::Response,
    dest: &Path,
    part: &Path,
    existing: u64,
    expected: Option<(&str, ChecksumKind)>,
    mut progress: Option<&mut Box<dyn FnMut(u64, u64) + Send>>,
    stall: Duration,
    host: &str,
) -> Result<(), DownloadEngineError> {
    let content_len = response.content_length().unwrap_or(0);
    let total = if existing > 0 && content_len > 0 {
        existing + content_len
    } else {
        content_len
    };

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(existing > 0)
        .write(true)
        .truncate(existing == 0)
        .open(part)?;

    let mut stream = response;
    let mut buffer = vec![0u8; 64 * 1024];
    let mut received = existing;
    let mut last_byte_at = Instant::now();

    loop {
        if last_byte_at.elapsed() > stall {
            crate::http::circuit_record_failure(host);
            return Err(DownloadEngineError::Stalled {
                secs: stall.as_secs(),
            });
        }
        let n = match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e)
                if e.kind() == std::io::ErrorKind::TimedOut
                    || e.kind() == std::io::ErrorKind::WouldBlock =>
            {
                crate::http::circuit_record_failure(host);
                return Err(DownloadEngineError::Stalled {
                    secs: stall.as_secs(),
                });
            }
            Err(e) => {
                crate::http::circuit_record_failure(host);
                return Err(DownloadEngineError::Io(e));
            }
        };
        file.write_all(&buffer[..n])?;
        received += n as u64;
        last_byte_at = Instant::now();
        if let Some(cb) = progress.as_mut() {
            cb(received, total);
        }
    }
    file.flush()?;
    file.sync_all()?;
    drop(file);

    if let Some((exp, kind)) = expected {
        let actual = kind.hash_file(part)?;
        if !actual.eq_ignore_ascii_case(exp) {
            let _ = std::fs::remove_file(part);
            return Err(DownloadEngineError::ChecksumMismatch {
                expected: exp.to_string(),
                actual,
            });
        }
    }

    if dest.exists() {
        let _ = std::fs::remove_file(dest);
    }
    std::fs::rename(part, dest)?;
    Ok(())
}

/// Parallel downloads with a concurrency cap.
pub fn download_batch_limited(
    jobs: &[(String, PathBuf, Option<String>)],
    concurrency: usize,
) -> Result<(), DownloadEngineError> {
    use rayon::prelude::*;

    let limit = concurrency.max(1);
    let active = AtomicUsize::new(0);
    let errors: Mutex<Vec<String>> = Mutex::new(Vec::new());

    jobs.par_iter().for_each(|(url, path, sha1)| {
        loop {
            let cur = active.load(Ordering::SeqCst);
            if cur < limit
                && active
                    .compare_exchange(cur, cur + 1, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
            {
                break;
            }
            std::thread::yield_now();
        }
        let expected = sha1.as_deref().map(|s| (s, ChecksumKind::Sha1));
        let result = download_resumable(url, path, expected, None, None);
        if let Err(e) = result {
            if let Ok(mut g) = errors.lock() {
                g.push(format!("{url}: {e}"));
            }
        }
        active.fetch_sub(1, Ordering::SeqCst);
    });

    let errs = errors.into_inner().unwrap_or_default();
    if errs.is_empty() {
        Ok(())
    } else {
        Err(DownloadEngineError::Batch(errs.join("\n")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn part_path_is_stable() {
        let p = PathBuf::from("mods/foo.jar");
        assert_eq!(
            part_path_for(&p),
            PathBuf::from("mods/foo.jar.tuffbox.part")
        );
    }
}
