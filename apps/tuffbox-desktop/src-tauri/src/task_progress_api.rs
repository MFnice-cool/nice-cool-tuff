//! Tauri bridge for background TaskProgress bus.

use tuffbox_core::task_progress::{self, BackgroundTask};

#[tauri::command(rename_all = "camelCase")]
pub fn list_background_tasks() -> Vec<BackgroundTask> {
    task_progress::list_tasks()
}

#[tauri::command(rename_all = "camelCase")]
pub fn dismiss_background_task(id: String) -> Result<(), String> {
    task_progress::dismiss(&id);
    Ok(())
}

/// Start a named task from the UI / other commands (returns id).
#[tauri::command(rename_all = "camelCase")]
pub fn start_background_task(id: String, title: String) -> String {
    task_progress::start_task(id, title)
}

/// Claim a task id for a new operation; rejects when the same id is already
/// running (Millida jobs.rs-style duplicate protection).
#[tauri::command(rename_all = "camelCase")]
pub fn try_start_background_task(id: String, title: String) -> bool {
    task_progress::try_start_task(id, title)
}

/// Ask a running task to cancel; the worker polls and acknowledges.
#[tauri::command(rename_all = "camelCase")]
pub fn cancel_background_task(id: String) -> Result<(), String> {
    task_progress::request_cancel(&id);
    Ok(())
}
