use serde::{Deserialize, Serialize};
use std::path::Path;

/// Categorizes why a launch failed or crashed, so the UI can decide whether
/// a Retry button makes sense and what kind of messaging to show.
///
/// This is the single structured error type that the launch commands return
/// (instead of a bare `String`). Previously every launch failure was written
/// only to the log file inside a `spawn_blocking` task and the command always
/// returned `Ok`, so the UI thought the game had started and the user only
/// discovered the failure by opening the log. Returning this typed error lets
/// the frontend show a clear, categorized message with a Retry action when
/// appropriate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchErrorKind {
    /// Network is down / host unreachable (incl. circuit-breaker open).
    Offline,
    /// A specific mirror/host failed but the version may resolve elsewhere.
    HostUnreachable,
    /// The Minecraft version alias (`latest`/`snapshot`/...) could not be
    /// resolved to a concrete id.
    VersionResolve,
    /// One or more mod files failed to download.
    ModDownload,
    /// No usable Java runtime was found (or the found one is wrong).
    JavaMissing,
    /// Generic install/prepare failure (missing files, bad manifest, ...).
    Install,
    /// The JVM process started but exited with a non-zero code / crash.
    LaunchCrash,
    /// Anything else.
    Unknown,
}

impl LaunchErrorKind {
    /// Stable snake_case id. This MUST match the serialized form the frontend
    /// receives (the enum uses `#[serde(rename_all = "snake_case")]` and the
    /// frontend matches on these exact strings in `launch.ts`), so we return
    /// the same spelling here for use in Rust-side logs/metrics.
    pub fn as_label(self) -> &'static str {
        match self {
            LaunchErrorKind::Offline => "offline",
            LaunchErrorKind::HostUnreachable => "host_unreachable",
            LaunchErrorKind::VersionResolve => "version_resolve",
            LaunchErrorKind::ModDownload => "mod_download",
            LaunchErrorKind::JavaMissing => "java_missing",
            LaunchErrorKind::Install => "install",
            LaunchErrorKind::LaunchCrash => "launch_crash",
            LaunchErrorKind::Unknown => "unknown",
        }
    }
}

impl std::fmt::Display for LaunchErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_label())
    }
}

impl LaunchErrorKind {
    /// Whether retrying the same launch is likely to help. Offline-style and
    /// transient failures are retryable; fundamental config errors are not.
    pub fn retryable(self) -> bool {
        matches!(
            self,
            LaunchErrorKind::Offline
                | LaunchErrorKind::HostUnreachable
                | LaunchErrorKind::VersionResolve
                | LaunchErrorKind::ModDownload
                | LaunchErrorKind::JavaMissing
                | LaunchErrorKind::Install
                | LaunchErrorKind::LaunchCrash
        )
    }
}

/// Structured launch error returned by the launch Tauri commands.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchErrorInfo {
    pub kind: LaunchErrorKind,
    pub message: String,
    /// Path to the launch log, when one was opened, so the UI can offer a
    /// "view log" action alongside the error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_path: Option<String>,
}

impl LaunchErrorInfo {
    pub fn new(kind: LaunchErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            log_path: None,
        }
    }

    /// Attach the launch-log path (used by the install/launch commands).
    pub fn with_log(mut self, path: &Path) -> Self {
        self.log_path = Some(path.to_string_lossy().into_owned());
        self
    }

    pub fn retryable(&self) -> bool {
        self.kind.retryable()
    }
}

impl std::fmt::Display for LaunchErrorInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.kind, self.message)
    }
}

impl std::error::Error for LaunchErrorInfo {}

/// True when an error message describes an offline / unreachable condition,
/// so we can map it to [`LaunchErrorKind::Offline`] even when the underlying
/// error type doesn't make that explicit (e.g. the circuit breaker or a
/// low-level OS socket error surfaced as a string).
pub fn message_looks_offline(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    m.contains("circuit")
        || m.contains("connection refused")
        || m.contains("failed to resolve")
        || m.contains("dns")
        || m.contains("timed out")
        || m.contains("timeout")
        || m.contains("no route")
        || m.contains("host unreachable")
        || m.contains("network is unreachable")
        || m.contains("os error 1006")
        || m.contains("os error 10060")
        || m.contains("os error 11001")
        || m.contains("nameresolution")
        || m.contains("name or service not known")
}

/// Map a free-form build / spawn error message to the most specific
/// [`LaunchErrorKind`] we can infer from its text.
///
/// This centralises the heuristics that were previously duplicated across the
/// launcher backend (one branch for command-building failures, another for
/// process-spawn failures) so they live in one place and can be unit-tested.
/// Order matters: an offline condition is reported as [`LaunchErrorKind::Offline`]
/// even if the message also happens to mention "java", since a retry has a
/// real chance of succeeding once the network is back.
pub fn classify_build_error_kind(msg: &str) -> LaunchErrorKind {
    let lower = msg.to_ascii_lowercase();
    if message_looks_offline(msg) {
        LaunchErrorKind::Offline
    } else if lower.contains("java") || lower.contains("not found") {
        LaunchErrorKind::JavaMissing
    } else {
        LaunchErrorKind::Install
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn retryable_kinds() {
        assert!(LaunchErrorKind::Offline.retryable());
        assert!(LaunchErrorKind::HostUnreachable.retryable());
        assert!(LaunchErrorKind::VersionResolve.retryable());
        assert!(LaunchErrorKind::ModDownload.retryable());
        assert!(LaunchErrorKind::JavaMissing.retryable());
        assert!(LaunchErrorKind::Install.retryable());
        assert!(LaunchErrorKind::LaunchCrash.retryable());
        // A fundamentally broken config is not something a retry will fix.
        assert!(!LaunchErrorKind::Unknown.retryable());
    }

    #[test]
    fn kind_labels_are_snake_case_and_stable() {
        // These labels are part of the contract the frontend keys off of
        // (see launch.ts RETRYABLE set), so assert they never drift.
        assert_eq!(LaunchErrorKind::Offline.as_label(), "offline");
        assert_eq!(
            LaunchErrorKind::HostUnreachable.as_label(),
            "host_unreachable"
        );
        assert_eq!(
            LaunchErrorKind::VersionResolve.as_label(),
            "version_resolve"
        );
        assert_eq!(LaunchErrorKind::ModDownload.as_label(), "mod_download");
        assert_eq!(LaunchErrorKind::JavaMissing.as_label(), "java_missing");
        assert_eq!(LaunchErrorKind::Install.as_label(), "install");
        assert_eq!(LaunchErrorKind::LaunchCrash.as_label(), "launch_crash");
        assert_eq!(LaunchErrorKind::Unknown.as_label(), "unknown");
    }

    #[test]
    fn with_log_attaches_path_and_serializes() {
        let info = LaunchErrorInfo::new(LaunchErrorKind::JavaMissing, "no java")
            .with_log(&PathBuf::from("/tmp/foo.log"));
        assert_eq!(info.log_path.as_deref(), Some("/tmp/foo.log"));
        assert!(info.retryable());

        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"kind\":\"java_missing\""));
        assert!(json.contains("\"logPath\":\"/tmp/foo.log\""));
        // log_path must be omitted (not null) when absent.
        let no_log = LaunchErrorInfo::new(LaunchErrorKind::Unknown, "x");
        let json2 = serde_json::to_string(&no_log).unwrap();
        assert!(!json2.contains("logPath"));
    }

    #[test]
    fn looks_offline_detects_common_conditions() {
        assert!(message_looks_offline(
            "reqwest: error sending request: connection refused"
        ));
        assert!(message_looks_offline(
            "failed to resolve host example.com: DNS error"
        ));
        assert!(message_looks_offline(
            "notify: operation timed out after 30s"
        ));
        assert!(message_looks_offline(
            "os error 10060: network is unreachable"
        ));
        assert!(!message_looks_offline("mod file not found on server"));
        assert!(!message_looks_offline(
            "java.lang.OutOfMemoryError: heap space"
        ));
    }

    #[test]
    fn display_includes_kind_and_message() {
        let info = LaunchErrorInfo::new(LaunchErrorKind::Install, "boom");
        assert_eq!(format!("{info}"), "[install] boom");
    }

    #[test]
    fn classify_build_error_prefers_offline_over_java() {
        // A network failure mentioning java must still surface as Offline so
        // the UI offers a retry rather than blaming the Java install.
        assert_eq!(
            classify_build_error_kind(
                "reqwest: error sending request: connection refused while downloading java runtime"
            ),
            LaunchErrorKind::Offline
        );
        assert_eq!(
            classify_build_error_kind("network is unreachable: failed to fetch version manifest"),
            LaunchErrorKind::Offline
        );
    }

    #[test]
    fn classify_build_error_maps_java_and_not_found() {
        assert_eq!(
            classify_build_error_kind("java.lang.UnsupportedClassVersionError: bad major version"),
            LaunchErrorKind::JavaMissing
        );
        assert_eq!(
            classify_build_error_kind(
                "the system cannot find the path specified: java.exe not found"
            ),
            LaunchErrorKind::JavaMissing
        );
    }

    #[test]
    fn classify_build_error_falls_back_to_install() {
        assert_eq!(
            classify_build_error_kind(
                "failed to build minecraft launch command: unknown profile field"
            ),
            LaunchErrorKind::Install
        );
    }
}
