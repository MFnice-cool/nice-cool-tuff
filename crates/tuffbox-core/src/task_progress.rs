//! Background task progress bus (VisualTask-inspired, TuffBox-owned).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

static TASKS: LazyLock<Mutex<HashMap<String, BackgroundTask>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskStatus {
    Running,
    Paused,
    CancelRequested,
    Cancelled,
    Succeeded,
    Failed,
    Dismissed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundTask {
    pub id: String,
    pub title: String,
    pub status: TaskStatus,
    /// 0.0..=1.0 when known; None = indeterminate.
    pub progress: Option<f64>,
    pub detail: Option<String>,
    pub error: Option<String>,
    pub updated_at_ms: u64,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn start_task(id: impl Into<String>, title: impl Into<String>) -> String {
    let id = id.into();
    let task = BackgroundTask {
        id: id.clone(),
        title: title.into(),
        status: TaskStatus::Running,
        progress: Some(0.0),
        detail: None,
        error: None,
        updated_at_ms: now_ms(),
    };
    if let Ok(mut g) = TASKS.lock() {
        g.insert(id.clone(), task);
    }
    id
}

pub fn set_progress(id: &str, progress: f64, detail: Option<String>) {
    if let Ok(mut g) = TASKS.lock() {
        if let Some(t) = g.get_mut(id) {
            t.progress = Some(progress.clamp(0.0, 1.0));
            if let Some(d) = detail {
                t.detail = Some(d);
            }
            t.updated_at_ms = now_ms();
        }
    }
}

/// Publish a human-readable "what is it doing right now" stage label without
/// touching the numeric progress. Long-running pipelines (crash diagnosis,
/// AI analysis) call this between steps so a polling UI can show what the
/// backend is actually doing instead of a bare spinner. Creates the task if
/// it does not exist yet (indeterminate, Running).
pub fn set_running_stage(id: &str, stage: &str) {
    if let Ok(mut g) = TASKS.lock() {
        let task = g.entry(id.to_string()).or_insert_with(|| BackgroundTask {
            id: id.to_string(),
            title: stage.to_string(),
            status: TaskStatus::Running,
            progress: None,
            detail: None,
            error: None,
            updated_at_ms: now_ms(),
        });
        task.status = TaskStatus::Running;
        task.detail = Some(stage.to_string());
        task.updated_at_ms = now_ms();
    }
}

pub fn succeed(id: &str, detail: Option<String>) {
    if let Ok(mut g) = TASKS.lock() {
        if let Some(t) = g.get_mut(id) {
            t.status = TaskStatus::Succeeded;
            t.progress = Some(1.0);
            if let Some(d) = detail {
                t.detail = Some(d);
            }
            t.updated_at_ms = now_ms();
        }
    }
}

pub fn pause(id: &str, detail: Option<String>) {
    if let Ok(mut g) = TASKS.lock() {
        if let Some(t) = g.get_mut(id) {
            t.status = TaskStatus::Paused;
            t.error = None;
            if let Some(d) = detail {
                t.detail = Some(d);
            }
            t.updated_at_ms = now_ms();
        }
    }
}

pub fn fail(id: &str, error: impl Into<String>) {
    if let Ok(mut g) = TASKS.lock() {
        if let Some(t) = g.get_mut(id) {
            t.status = TaskStatus::Failed;
            t.error = Some(error.into());
            t.updated_at_ms = now_ms();
        }
    }
}

pub fn dismiss(id: &str) {
    if let Ok(mut g) = TASKS.lock() {
        if let Some(t) = g.get_mut(id) {
            t.status = TaskStatus::Dismissed;
            t.updated_at_ms = now_ms();
        }
        // Drop dismissed + old succeeded/paused after mark.
        g.retain(|_, t| {
            t.status == TaskStatus::Running
                || t.status == TaskStatus::Paused
                || t.status == TaskStatus::Failed
                || (t.status == TaskStatus::Succeeded
                    && now_ms().saturating_sub(t.updated_at_ms) < 60_000)
        });
    }
}

// ── Cancellation + duplicate protection (Millida jobs.rs-inspired) ──────

/// Request cancellation of a running task. The long operation polls
/// `is_cancel_requested` between steps and bails out cleanly.
pub fn request_cancel(id: &str) {
    if let Ok(mut g) = TASKS.lock() {
        if let Some(t) = g.get_mut(id) {
            t.status = TaskStatus::CancelRequested;
            t.detail = Some("cancelling…".into());
            t.updated_at_ms = now_ms();
        }
    }
}

/// True when the task was marked `CancelRequested` and should stop.
pub fn is_cancel_requested(id: &str) -> bool {
    TASKS
        .lock()
        .ok()
        .and_then(|g| g.get(id).map(|t| t.status == TaskStatus::CancelRequested))
        .unwrap_or(false)
}

/// Acknowledge the cancel: transition to `Cancelled` (terminal, like Failed).
pub fn mark_cancelled(id: &str, detail: Option<String>) {
    if let Ok(mut g) = TASKS.lock() {
        if let Some(t) = g.get_mut(id) {
            t.status = TaskStatus::Cancelled;
            t.detail = detail;
            t.updated_at_ms = now_ms();
        }
    }
}

/// Try to claim a task id for a new operation. Returns `false` (and leaves
/// the existing task untouched) when a non-terminal task with the same id is
/// already running — prevents two concurrent installs of the same pack.
pub fn try_start_task(id: impl Into<String>, title: impl Into<String>) -> bool {
    let id = id.into();
    let mut g = match TASKS.lock() {
        Ok(g) => g,
        Err(_) => return false,
    };
    if let Some(existing) = g.get(&id) {
        if matches!(
            existing.status,
            TaskStatus::Running | TaskStatus::Paused | TaskStatus::CancelRequested
        ) {
            return false;
        }
    }
    g.insert(
        id.clone(),
        BackgroundTask {
            id: id.clone(),
            title: title.into(),
            status: TaskStatus::Running,
            progress: Some(0.0),
            detail: None,
            error: None,
            updated_at_ms: now_ms(),
        },
    );
    true
}

pub fn list_tasks() -> Vec<BackgroundTask> {
    let Ok(g) = TASKS.lock() else {
        return Vec::new();
    };
    let mut v: Vec<_> = g
        .values()
        .filter(|t| t.status != TaskStatus::Dismissed)
        .cloned()
        .collect();
    v.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_lifecycle() {
        let id = start_task("test-task-1", "Demo");
        set_progress(&id, 0.5, Some("halfway".into()));
        succeed(&id, None);
        let listed = list_tasks();
        assert!(listed
            .iter()
            .any(|t| t.id == id && t.status == TaskStatus::Succeeded));
        dismiss(&id);
    }

    #[test]
    fn try_start_rejects_duplicate_running_task() {
        assert!(try_start_task("test-dup", "First"));
        assert!(!try_start_task("test-dup", "Second"));
        // After a terminal status, the id is claimable again.
        succeed("test-dup", None);
        assert!(try_start_task("test-dup", "Third"));
        dismiss("test-dup");
    }

    #[test]
    fn cancel_flow_request_poll_acknowledge() {
        let id = start_task("test-cancel", "Demo");
        assert!(!is_cancel_requested(&id));
        request_cancel(&id);
        assert!(is_cancel_requested(&id));
        // While cancel is requested, a duplicate start is still rejected.
        assert!(!try_start_task("test-cancel", "Again"));
        mark_cancelled(&id, Some("stopped".into()));
        // Terminal state: not cancel-requested anymore, id reclaimable.
        assert!(!is_cancel_requested(&id));
        assert!(try_start_task("test-cancel", "Retry"));
        dismiss("test-cancel");
    }
}
