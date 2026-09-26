//! Tune Config AI chat sessions under `.tuffbox/tune_chats/`.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::action_plan::ActionPlan;
use crate::fs_util::atomic_write;
use crate::quest_chat::{now_iso, validate_chat_id};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TuneChatMessage {
    pub role: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

/// Last advise payload awaiting Review → Apply in Tune.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TunePendingAdvise {
    pub plan: ActionPlan,
    pub explanation: String,
    #[serde(default)]
    pub research_log: Vec<serde_json::Value>,
    #[serde(default)]
    pub unknown_keys: Vec<serde_json::Value>,
    #[serde(default)]
    pub diffs: Vec<serde_json::Value>,
    #[serde(default)]
    pub validation_ok: bool,
    #[serde(default)]
    pub validation_errors: Vec<String>,
    #[serde(default)]
    pub validation_warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TuneChatSession {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub messages: Vec<TuneChatMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_advise: Option<TunePendingAdvise>,
    #[serde(default)]
    pub updated_at: String,
    /// Last focus config path used with this session (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus_path: Option<String>,
}

pub fn tune_chats_dir(project_dir: &Path) -> PathBuf {
    project_dir.join(".tuffbox").join("tune_chats")
}

fn chat_file_path(project_dir: &Path, chat_id: &str) -> Result<PathBuf, String> {
    validate_chat_id(chat_id)?;
    let dir = tune_chats_dir(project_dir);
    let path = dir.join(format!("{chat_id}.json"));
    let parent = path
        .parent()
        .ok_or_else(|| "invalid chat path".to_string())?;
    if parent != dir.as_path() {
        return Err(format!("invalid chat id: {chat_id}"));
    }
    Ok(path)
}

#[derive(Debug, Clone, Default)]
pub struct TuneChatListResult {
    pub sessions: Vec<TuneChatSession>,
    pub corrupt_skipped: u32,
}

pub fn list_tune_chats_detailed(project_dir: &Path) -> Result<TuneChatListResult, String> {
    let dir = tune_chats_dir(project_dir);
    if !dir.exists() {
        return Ok(TuneChatListResult::default());
    }
    let mut sessions = Vec::new();
    let mut corrupt_skipped = 0u32;
    for entry in std::fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .flatten()
    {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(_) => {
                corrupt_skipped += 1;
                continue;
            }
        };
        match serde_json::from_str::<TuneChatSession>(&text) {
            Ok(session) => sessions.push(session),
            Err(_) => corrupt_skipped += 1,
        }
    }
    sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(TuneChatListResult {
        sessions,
        corrupt_skipped,
    })
}

pub fn save_tune_chat(project_dir: &Path, session: &TuneChatSession) -> Result<PathBuf, String> {
    let path = chat_file_path(project_dir, &session.id)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(session).map_err(|e| e.to_string())?;
    atomic_write(&path, text)?;
    Ok(path)
}

pub fn load_tune_chat(project_dir: &Path, chat_id: &str) -> Result<TuneChatSession, String> {
    let path = chat_file_path(project_dir, chat_id)?;
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

pub fn delete_tune_chat(project_dir: &Path, chat_id: &str) -> Result<(), String> {
    let path = chat_file_path(project_dir, chat_id)?;
    if path.exists() {
        std::fs::remove_file(path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn new_tune_chat_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("tchat-{ms}")
}

pub fn new_tune_chat_session(title: Option<&str>) -> TuneChatSession {
    TuneChatSession {
        id: new_tune_chat_id(),
        title: title
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Tune configs".into()),
        messages: Vec::new(),
        pending_advise: None,
        updated_at: now_iso(),
        focus_path: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = new_tune_chat_session(Some("FPS"));
        session.id = "tchat-round".into();
        session.messages.push(TuneChatMessage {
            role: "user".into(),
            content: "lower render distance".into(),
            created_at: Some(now_iso()),
        });
        save_tune_chat(dir.path(), &session).unwrap();
        let loaded = load_tune_chat(dir.path(), "tchat-round").unwrap();
        assert_eq!(loaded.title, "FPS");
        assert_eq!(loaded.messages.len(), 1);
    }

    #[test]
    fn rejects_traversal() {
        let dir = tempfile::tempdir().unwrap();
        assert!(chat_file_path(dir.path(), "../x").is_err());
    }
}
