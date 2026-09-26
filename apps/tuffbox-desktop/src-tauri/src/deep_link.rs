//! `tuffbox://install?repo=owner/repo` share links.
//!
//! The URL arrives either as the launch argument that started the process
//! (cold start) or through a second instance's forwarded argv while the app
//! is already running. Both paths funnel into [`handle_install_url`], which
//! parks a verdict for the frontend to drain via [`take_pending_install_repo`]
//! and emits `tuffbox:install-link` so an already-mounted UI reacts at once.

use serde::Serialize;
use std::sync::LazyLock;
use std::sync::Mutex;
use tuffbox_core::github_pack::parse_github_source;

/// Verdict for one install link, drained exactly once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum PendingInstallRepo {
    /// Validated and normalized to what `github_pack_inspect_source` accepts.
    Valid { repo: String },
    /// Not a GitHub repo — surfaced as a toast by the frontend, never installed.
    Invalid { raw: String },
}

/// Parked link from a `tuffbox://install?…` URL that arrived before the UI
/// was ready. Drained on mount (see [`take_pending_install_repo`]).
static PENDING_INSTALL_REPO: LazyLock<Mutex<Option<PendingInstallRepo>>> =
    LazyLock::new(|| Mutex::new(None));

/// `tuffbox://install?repo=<source>` → the repo parameter, percent-decoded
/// (`owner%2Frepo` arrives encoded from most link generators). Windows fills
/// the host slot with the path; both spellings are accepted. Any other path
/// on our scheme returns None so foreign links are ignored rather than
/// reported as broken installs.
fn install_repo_param(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    if parsed.scheme() != "tuffbox" {
        return None;
    }
    // Host slot holds "install" in `tuffbox://install?…`; tolerate the
    // slash spelling too (`tuffbox:///install?…`).
    let path = format!(
        "{}{}",
        parsed.host_str().unwrap_or_default(),
        parsed.path()
    );
    let path = path.trim_start_matches('/').trim_end_matches('/');
    if !path.eq_ignore_ascii_case("install") {
        return None;
    }
    parsed
        .query_pairs()
        .find(|(key, _)| key == "repo")
        .map(|(_, value)| value.into_owned())
}

/// Classifies the repo param of an install link against the same parser the
/// manual import flow uses. None = not an install URL (caller ignores it).
pub fn verdict_for(url: &str) -> Option<PendingInstallRepo> {
    let raw = install_repo_param(url)?;
    Some(match parse_github_source(&raw) {
        Ok(src) => PendingInstallRepo::Valid {
            // Keep a pinned ref when the link carries one; `gh:` is the
            // spelled-out form both parse and inspect accept.
            repo: match src.git_ref {
                Some(git_ref) => format!("gh:{}/{}:{}", src.owner, src.repo, git_ref),
                None => format!("{}/{}", src.owner, src.repo),
            },
        },
        Err(_) => PendingInstallRepo::Invalid { raw },
    })
}

/// Validates and parks one incoming install link. Called for cold start
/// (launch args), warm start (second-instance argv) and runtime open-url
/// events — always exactly once per physical link.
pub fn handle_install_url(app: &tauri::AppHandle, url: &str) {
    use tauri::{Emitter, Manager};

    if !url.starts_with("tuffbox://") {
        return;
    }
    // A scheme URL without an install path or repo param is still a user
    // action worth reporting until the scheme grows other verbs.
    let verdict = verdict_for(url)
        .unwrap_or_else(|| PendingInstallRepo::Invalid { raw: url.to_string() });

    if let Ok(mut slot) = PENDING_INSTALL_REPO.lock() {
        *slot = Some(verdict.clone());
    }
    let _ = app.emit("tuffbox:install-link", verdict.clone());
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Drain one parked install link (frontend mount / event follow-up).
#[tauri::command(rename_all = "camelCase")]
pub fn take_pending_install_repo() -> Option<PendingInstallRepo> {
    PENDING_INSTALL_REPO.lock().ok().and_then(|mut slot| slot.take())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_repo_param() {
        assert_eq!(
            install_repo_param("tuffbox://install?repo=acme/cool-pack"),
            Some("acme/cool-pack".into())
        );
        assert_eq!(
            install_repo_param("tuffbox://install?repo=acme%2Fcool-pack"),
            Some("acme/cool-pack".into())
        );
        assert_eq!(
            install_repo_param("tuffbox://install/?repo=acme/cool-pack"),
            Some("acme/cool-pack".into())
        );
        assert_eq!(install_repo_param("tuffbox://other?repo=a/b"), None);
        assert_eq!(install_repo_param("https://github.com/a/b"), None);
        assert_eq!(install_repo_param("tuffbox://install"), None);
    }

    #[test]
    fn classifies_links() {
        assert_eq!(
            verdict_for("tuffbox://install?repo=acme/cool-pack"),
            Some(PendingInstallRepo::Valid {
                repo: "acme/cool-pack".into()
            })
        );
        // Full GitHub URLs keep their ref through normalization.
        assert_eq!(
            verdict_for(
                "tuffbox://install?repo=https%3A%2F%2Fgithub.com%2Facme%2Fcool-pack%2Freleases%2Ftag%2Fv1.2.3"
            ),
            Some(PendingInstallRepo::Valid {
                repo: "gh:acme/cool-pack:v1.2.3".into()
            })
        );
        assert_eq!(
            verdict_for("tuffbox://install?repo=not+a+repo"),
            Some(PendingInstallRepo::Invalid {
                raw: "not a repo".into()
            })
        );
    }
}
