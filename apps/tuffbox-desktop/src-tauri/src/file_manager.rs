//! Cross-platform "reveal in file manager" helper.
//!
//! Replaces `tauri_plugin_shell::ShellExt::open()` for folder paths. The
//! shell plugin's `open` uses open-rs `shellexecute-on-windows` on Windows,
//! which calls CoInitialize + ShellExecuteExW on the calling (IPC) thread —
//! that COM usage in Tauri's process aborted the whole app with
//! STATUS_STACK_BUFFER_OVERRUN (0xC0000409) when the user pressed the Home
//! "Folder" button (Windows Event Log, crashes 2026-09-04 08:57 & 09:41).
//!
//! This helper spawns the platform file manager as a fully detached child
//! with no COM involvement in our process, and works on every desktop OS:
//! - Windows: `explorer` (also selects the folder in Explorer via /select)
//! - macOS:   `open` (Finder)
//! - Linux:   xdg-open, falling back to known file managers

use std::process::Command;
// Only the unix fallback branch pipes Stdio; a plain global import is
// "unused" on Windows/macOS builds (each branch is cfg-compiled).
#[cfg(all(unix, not(target_os = "macos")))]
use std::process::Stdio;

/// Open a directory in the OS file manager. On Windows the directory itself
/// is opened (pass `select_path` to reveal/select a file or folder inside
/// its parent instead). Never touches COM in our process.
pub(crate) fn open_in_file_manager(dir: &std::path::Path, select_path: Option<&std::path::Path>) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let _ = select_path; // /select reveals a *file*; for a dir we open it directly
        Command::new("explorer")
            .arg(dir)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("failed to launch explorer: {e}"))
    }

    #[cfg(target_os = "macos")]
    {
        let _ = select_path;
        Command::new("open")
            .arg(dir)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("failed to launch Finder: {e}"))
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = select_path;
        // xdg-open covers most desktop environments; fall back to common
        // file managers when it is missing (headless/minimal setups).
        let mut last_err: Option<String> = None;
        let mut try_open = |prog: &str| -> Result<(), String> {
            Command::new(prog)
                .arg(dir)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map(|_| ())
                .map_err(|e| format!("{prog}: {e}"))
        };
        for prog in ["xdg-open", "nautilus", "dolphin", "thunar", "pcmanfm"] {
            match try_open(prog) {
                Ok(()) => return Ok(()),
                Err(e) => last_err = Some(e),
            }
        }
        Err(last_err.unwrap_or_else(|| "no file manager found".into()))
    }
}
