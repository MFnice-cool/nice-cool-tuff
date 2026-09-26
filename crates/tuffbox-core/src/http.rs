use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use reqwest::StatusCode;

// ---------------------------------------------------------------------------
// Circuit Breaker — prevents hammering an API that is down
// ---------------------------------------------------------------------------
//
// Inspired by theseus/fetch.rs.  After `OPEN_THRESHOLD` consecutive failures
// to the same host within `WINDOW` seconds the circuit *opens* for
// `OPEN_DURATION`.  During the open window every request to that host
// immediately returns `CircuitBreakerOpen` without touching the network.
// After the window expires the next request is allowed through
// (*half-open*) — if it succeeds the circuit resets, if it fails the
// breaker opens again.

const OPEN_THRESHOLD: u32 = 5;
const WINDOW: Duration = Duration::from_secs(60);
const OPEN_DURATION: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CircuitState {
    Closed,
    Open { opened_at: Instant },
}

#[derive(Debug)]
struct HostBreaker {
    failures: Vec<Instant>,
    state: CircuitState,
}

struct CircuitBreakerState {
    hosts: HashMap<String, HostBreaker>,
}

static CIRCUIT: LazyLock<Mutex<CircuitBreakerState>> = LazyLock::new(|| {
    Mutex::new(CircuitBreakerState {
        hosts: HashMap::new(),
    })
});

/// Returns `Ok(())` if the request may proceed, or `Err` if the
/// circuit breaker has tripped for this host.
pub(crate) fn circuit_check(host: &str) -> Result<(), CircuitBreakerOpen> {
    let mut cb = CIRCUIT.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    let entry = cb
        .hosts
        .entry(host.to_string())
        .or_insert_with(|| HostBreaker {
            failures: Vec::new(),
            state: CircuitState::Closed,
        });

    match entry.state {
        CircuitState::Open { opened_at } => {
            if now.duration_since(opened_at) < OPEN_DURATION {
                return Err(CircuitBreakerOpen {
                    host: host.to_string(),
                    retry_after_secs: (OPEN_DURATION - now.duration_since(opened_at)).as_secs(),
                });
            }
            // Half-open: allow the request through, reset failure window.
            entry.state = CircuitState::Closed;
            entry.failures.clear();
        }
        CircuitState::Closed => {
            // Prune old failures outside the tracking window.
            entry.failures.retain(|t| now.duration_since(*t) < WINDOW);
        }
    }
    Ok(())
}

/// Must be called after a *successful* request to the host.
pub(crate) fn circuit_record_success(host: &str) {
    let mut cb = CIRCUIT.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(entry) = cb.hosts.get_mut(host) {
        entry.failures.clear();
        entry.state = CircuitState::Closed;
    }
}

/// Must be called after a *failed* request to the host.
pub(crate) fn circuit_record_failure(host: &str) {
    let mut cb = CIRCUIT.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    let entry = cb
        .hosts
        .entry(host.to_string())
        .or_insert_with(|| HostBreaker {
            failures: Vec::new(),
            state: CircuitState::Closed,
        });

    entry.failures.push(now);
    // Prune stale failures.
    entry.failures.retain(|t| now.duration_since(*t) < WINDOW);

    if entry.failures.len() as u32 >= OPEN_THRESHOLD {
        entry.state = CircuitState::Open { opened_at: now };
    }
}

#[derive(Debug)]
pub struct CircuitBreakerOpen {
    pub host: String,
    pub retry_after_secs: u64,
}

impl std::fmt::Display for CircuitBreakerOpen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "circuit breaker open for {}: retry in {}s",
            self.host, self.retry_after_secs
        )
    }
}

impl std::error::Error for CircuitBreakerOpen {}

/// Extracts the hostname from a URL for circuit-breaker tracking.
pub(crate) fn host_from_url(url: &str) -> &str {
    url.split("://")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .and_then(|rest| rest.split(':').next())
        .unwrap_or(url)
}

// ---------------------------------------------------------------------------
// HTTP Client
// ---------------------------------------------------------------------------

static HTTP: LazyLock<reqwest::blocking::Client> = LazyLock::new(|| {
    // Release hardening: a builder failure (exotic TLS/proxy misconfig) must
    // degrade to a plain client instead of panicking the whole process on
    // first network use.
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(60))
        .connect_timeout(Duration::from_secs(15))
        .tcp_keepalive(Duration::from_secs(10))
        .tcp_nodelay(true)
        .user_agent("TuffBox-IDE/0.1.0")
        .build()
        .unwrap_or_else(|_| reqwest::blocking::Client::new())
});

static HTTP_ASYNC: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .connect_timeout(Duration::from_secs(15))
        .tcp_keepalive(Duration::from_secs(10))
        .user_agent("TuffBox-IDE/0.1.0")
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
});

/// Shared blocking HTTP client with connection pooling.
pub(crate) fn http() -> &'static reqwest::blocking::Client {
    &HTTP
}

/// Shared async HTTP client with connection pooling.
pub(crate) fn http_async() -> &'static reqwest::Client {
    &HTTP_ASYNC
}

const MAX_RETRIES: u32 = 3;
const MAX_RATE_LIMIT_RETRIES: u32 = 4;
const MAX_REDIRECTS: u8 = 5;
/// Cap on how long we'll wait for a Retry-After backoff before giving up.
const MAX_RATE_LIMIT_WAIT_SECS: u64 = 60;

fn retryable(err: &reqwest::Error) -> bool {
    err.is_timeout() || err.is_connect() || err.is_request()
}

/// Parses the `Retry-After` header from a 429 response.
/// Supports both delta-seconds (e.g. "5") and HTTP-date formats (e.g. "Wed, 21 Oct 2026 07:28:00 GMT").
/// Returns the number of seconds to wait.
fn parse_retry_after(value: &str) -> Option<u64> {
    let trimmed = value.trim();
    if let Ok(secs) = trimmed.parse::<u64>() {
        return Some(secs);
    }
    // Try HTTP-date format
    if let Ok(date) = chrono::DateTime::parse_from_rfc2822(trimmed) {
        let now = chrono::Utc::now();
        let diff = (date.with_timezone(&chrono::Utc) - now).num_seconds();
        if diff > 0 {
            return Some(diff as u64);
        }
    }
    None
}

/// Follows HTTP 3xx redirects manually, with workarounds for malformed `Location` headers
/// (e.g. protocol-relative `//cdn.example.com/path` or path-relative `/new-path`).
/// Respects a maximum redirect depth to prevent infinite loops.
///
/// Returns the final response. If the redirect chain exceeds `MAX_REDIRECTS`,
/// the last 3xx response is returned (its status code will be visible to
/// the caller, who can then surface it as an error).
fn follow_redirects(
    mut current_url: String,
) -> Result<reqwest::blocking::Response, reqwest::Error> {
    let mut last_response: Option<reqwest::blocking::Response> = None;

    for _ in 0..=MAX_REDIRECTS {
        let resp = HTTP.get(&current_url).send()?;
        let status = resp.status();
        last_response = Some(resp);

        if !status.is_redirection() {
            return Ok(last_response.unwrap());
        }

        // Extract Location header, handling both parsed-URL and raw-string forms
        let location = last_response
            .as_ref()
            .unwrap()
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let Some(loc) = location else {
            // No Location header — treat as final response (server bug, but we honour it)
            return Ok(last_response.unwrap());
        };

        // Resolve relative URLs
        let next_url = if loc.starts_with("//") {
            // Protocol-relative — prepend scheme from current URL
            if let Some(scheme_end) = current_url.find("://") {
                format!("{}{}", &current_url[..scheme_end + 1], loc)
            } else {
                format!("https:{}", loc)
            }
        } else if loc.starts_with('/') {
            // Path-relative — replace path on current URL
            if let Some(scheme_end) = current_url.find("://") {
                let after_scheme = &current_url[scheme_end + 3..];
                if let Some(path_start) = after_scheme.find('/') {
                    let host = &after_scheme[..path_start];
                    format!("{}://{}{}", &current_url[..scheme_end], host, loc)
                } else {
                    format!("{}{}", current_url, loc)
                }
            } else {
                loc.clone()
            }
        } else if loc.starts_with("http://") || loc.starts_with("https://") {
            loc.clone()
        } else {
            // Relative path — resolve against current URL's directory
            if let Some(last_slash) = current_url.rfind('/') {
                format!("{}/{}", &current_url[..last_slash], loc)
            } else {
                loc.clone()
            }
        };

        current_url = next_url;
    }

    // Exceeded MAX_REDIRECTS — return the last 3xx response so the caller
    // sees the actual status code.
    Ok(last_response.expect("loop ran at least once"))
}

fn fetch_with_redirects(url: &str) -> Result<reqwest::blocking::Response, reqwest::Error> {
    follow_redirects(url.to_string())
}

fn fetch(url: &str) -> Result<reqwest::blocking::Response, reqwest::Error> {
    let host = host_from_url(url);
    let mut last_err: Option<reqwest::Error> = None;
    let mut rate_limit_retries: u32 = 0;

    for attempt in 0..=MAX_RETRIES {
        if attempt > 0 {
            std::thread::sleep(Duration::from_secs(2u64.pow(attempt)));
        }
        match fetch_with_redirects(url) {
            Ok(resp) => {
                let status = resp.status();

                // HTTP 429 Too Many Requests — respect Retry-After
                if status == StatusCode::TOO_MANY_REQUESTS {
                    rate_limit_retries += 1;
                    if rate_limit_retries > MAX_RATE_LIMIT_RETRIES {
                        circuit_record_failure(host);
                        return resp.error_for_status();
                    }
                    let delay_secs = resp
                        .headers()
                        .get(reqwest::header::RETRY_AFTER)
                        .and_then(|v| v.to_str().ok())
                        .and_then(parse_retry_after)
                        .unwrap_or_else(|| {
                            // Exponential backoff: 10 * 2^retryCount, capped at MAX_RATE_LIMIT_WAIT_SECS
                            (10u64 * 2u64.pow(rate_limit_retries)).min(MAX_RATE_LIMIT_WAIT_SECS)
                        });
                    let capped_delay = delay_secs.min(MAX_RATE_LIMIT_WAIT_SECS);
                    last_err = Some(resp.error_for_status().unwrap_err());
                    circuit_record_failure(host);
                    std::thread::sleep(Duration::from_secs(capped_delay));
                    continue;
                }

                if status.is_server_error() {
                    if attempt < MAX_RETRIES {
                        last_err = Some(resp.error_for_status().unwrap_err());
                        circuit_record_failure(host);
                        continue;
                    }
                    circuit_record_failure(host);
                    return resp.error_for_status();
                }
                circuit_record_success(host);
                return Ok(resp);
            }
            Err(e) if retryable(&e) && attempt < MAX_RETRIES => {
                circuit_record_failure(host);
                last_err = Some(e);
            }
            Err(e) => {
                circuit_record_failure(host);
                return Err(e);
            }
        }
    }
    Err(last_err.expect("retries exhausted with last_err set"))
}

pub fn get_json<T: serde::de::DeserializeOwned>(url: &str) -> Result<T, reqwest::Error> {
    fetch(url)?.json()
}

pub fn get_json_with_context<T: serde::de::DeserializeOwned>(url: &str) -> Result<T, String> {
    let host = host_from_url(url);
    circuit_check(host).map_err(|e| e.to_string())?;
    let response = fetch(url).map_err(|e| format!("HTTP request failed: {}", e))?;
    let status = response.status();
    let body = response
        .text()
        .map_err(|e| format!("Failed to read response body: {}", e))?;

    serde_json::from_str(&body).map_err(|e| {
        let preview = if body.len() > 300 {
            format!("{}... ({} bytes total)", &body[..300], body.len())
        } else {
            body.clone()
        };
        format!(
            "JSON decode error for {} (status {}): {}. Response: {}",
            url, status, e, preview
        )
    })
}

/// Like [`get_json`], but treats a `404 Not Found` response as `Ok(None)`
/// instead of an error, which is the normal "no match" response for
/// lookup-style endpoints (e.g. Modrinth's hash lookup).
pub fn get_json_optional<T: serde::de::DeserializeOwned>(
    url: &str,
) -> Result<Option<T>, reqwest::Error> {
    let response = fetch(url)?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let response = response.error_for_status()?;
    Ok(Some(response.json()?))
}

pub fn get_bytes(url: &str) -> Result<Vec<u8>, reqwest::Error> {
    Ok(fetch(url)?.bytes()?.to_vec())
}

pub fn get_text(url: &str) -> Result<String, reqwest::Error> {
    fetch(url)?.text()
}

/// Streams a download from `url` directly to disk at `dest`, computing a SHA1
/// hash incrementally as bytes arrive. This avoids loading the entire file
/// into memory (important for large mods/libs that can be hundreds of MB).
///
/// If `expected_sha1` is provided and the computed hash doesn't match, the
/// partial file is deleted and an error is returned.
///
/// The `progress` callback is invoked with `(bytes_received_so_far,
/// total_bytes)` after each chunk, allowing the UI to show real-time
/// download progress without buffering the whole file.
///
/// The download writes to a temporary file (`.part`) and atomically renames
/// it to `dest` only after the hash check passes — so a failed or interrupted
/// download never leaves a half-written file at `dest`.
pub fn download_streaming(
    url: &str,
    dest: &std::path::Path,
    expected_sha1: Option<&str>,
    progress: Option<Box<dyn FnMut(u64, u64) + Send>>,
) -> Result<(), StreamingDownloadError> {
    let expected = expected_sha1.map(|s| (s, crate::download_engine::ChecksumKind::Sha1));
    crate::download_engine::download_resumable(url, dest, expected, progress, None).map_err(|e| {
        match e {
            crate::download_engine::DownloadEngineError::Http(err) => {
                StreamingDownloadError::Http(err)
            }
            crate::download_engine::DownloadEngineError::Io(err) => StreamingDownloadError::Io(err),
            crate::download_engine::DownloadEngineError::ChecksumMismatch { expected, actual } => {
                StreamingDownloadError::ChecksumMismatch { expected, actual }
            }
            other => StreamingDownloadError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                other.to_string(),
            )),
        }
    })
}

#[cfg(test)]
fn persist_replacing(
    file: tempfile::NamedTempFile,
    dest: &std::path::Path,
) -> Result<(), StreamingDownloadError> {
    file.persist(dest)
        .map(|_| ())
        .map_err(|error| StreamingDownloadError::Io(error.error))
}

#[derive(Debug, thiserror::Error)]
pub enum StreamingDownloadError {
    #[error("network error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("sha1 mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },
}

#[cfg(test)]
mod download_tests {
    use super::persist_replacing;
    use std::io::Write;

    #[test]
    fn persisted_download_replaces_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("same-name.jar");
        std::fs::write(&destination, b"old bytes").unwrap();

        let mut staged = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
        staged.write_all(b"new bytes").unwrap();
        staged.flush().unwrap();
        persist_replacing(staged, &destination).unwrap();

        assert_eq!(std::fs::read(destination).unwrap(), b"new bytes");
    }
}

pub fn post_json<B: serde::Serialize, T: serde::de::DeserializeOwned>(
    url: &str,
    body: &B,
) -> Result<T, reqwest::Error> {
    let host = host_from_url(url);

    let mut last_err = None;
    for attempt in 0..=MAX_RETRIES {
        if attempt > 0 {
            std::thread::sleep(Duration::from_secs(2u64.pow(attempt)));
        }
        let resp = HTTP
            .post(url)
            .header("Content-Type", "application/json")
            .json(body)
            .send();
        match resp {
            Ok(r) => {
                if r.status().is_server_error() {
                    if attempt < MAX_RETRIES {
                        last_err = Some(r.error_for_status().unwrap_err());
                        circuit_record_failure(host);
                        continue;
                    }
                    circuit_record_failure(host);
                    return r.error_for_status()?.json();
                }
                circuit_record_success(host);
                return r.error_for_status()?.json();
            }
            Err(e) if retryable(&e) && attempt < MAX_RETRIES => {
                circuit_record_failure(host);
                last_err = Some(e);
            }
            Err(e) => {
                circuit_record_failure(host);
                return Err(e);
            }
        }
    }
    Err(last_err.expect("retries exhausted with last_err set"))
}
