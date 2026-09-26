//! Quest AI chat sessions under `.tuffbox/quest_chats/`.

use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

use crate::fs_util::atomic_write;
use crate::quest_plan::QuestPlan;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AiTokenUsage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u32>,
}

impl AiTokenUsage {
    pub fn is_empty(&self) -> bool {
        self.prompt_tokens.is_none()
            && self.completion_tokens.is_none()
            && self.total_tokens.is_none()
    }

    pub fn merge_in(&mut self, other: &AiTokenUsage) {
        self.prompt_tokens = sum_opt(self.prompt_tokens, other.prompt_tokens);
        self.completion_tokens = sum_opt(self.completion_tokens, other.completion_tokens);
        self.total_tokens = sum_opt(self.total_tokens, other.total_tokens);
        if self.total_tokens.is_none() {
            if let (Some(p), Some(c)) = (self.prompt_tokens, self.completion_tokens) {
                self.total_tokens = Some(p.saturating_add(c));
            }
        }
    }
}

fn sum_opt(a: Option<u32>, b: Option<u32>) -> Option<u32> {
    match (a, b) {
        (None, None) => None,
        (Some(x), None) | (None, Some(x)) => Some(x),
        (Some(x), Some(y)) => Some(x.saturating_add(y)),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestChatMessage {
    pub role: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    /// Attached QuestPlan when the assistant produced one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<QuestPlan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress_log: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<AiTokenUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestChatSession {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub messages: Vec<QuestChatMessage>,
    /// Last pending plan awaiting Apply in the editor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_plan: Option<QuestPlan>,
    #[serde(default)]
    pub updated_at: String,
}

pub fn quest_chats_dir(project_dir: &Path) -> PathBuf {
    project_dir.join(".tuffbox").join("quest_chats")
}

/// Reject empty / traversal / multi-component chat ids before path join.
pub fn validate_chat_id(chat_id: &str) -> Result<(), String> {
    if chat_id.is_empty() {
        return Err("chat id is empty".into());
    }
    if chat_id.contains('\0')
        || chat_id.contains('/')
        || chat_id.contains('\\')
        || chat_id.contains("..")
    {
        return Err(format!("invalid chat id: {chat_id}"));
    }
    let path = Path::new(chat_id);
    let mut components = path.components();
    match components.next() {
        Some(Component::Normal(_)) if components.next().is_none() => Ok(()),
        _ => Err(format!("invalid chat id: {chat_id}")),
    }
}

fn chat_file_path(project_dir: &Path, chat_id: &str) -> Result<PathBuf, String> {
    validate_chat_id(chat_id)?;
    let dir = quest_chats_dir(project_dir);
    let path = dir.join(format!("{chat_id}.json"));
    // Ensure join did not escape the chats directory (defense in depth).
    let parent = path
        .parent()
        .ok_or_else(|| "invalid chat path".to_string())?;
    if parent != dir.as_path() {
        return Err(format!("invalid chat id: {chat_id}"));
    }
    let expected_name = format!("{chat_id}.json");
    if path.file_name().and_then(|n| n.to_str()) != Some(expected_name.as_str()) {
        return Err(format!("invalid chat id: {chat_id}"));
    }
    Ok(path)
}

pub fn list_quest_chats(project_dir: &Path) -> Result<Vec<QuestChatSession>, String> {
    Ok(list_quest_chats_detailed(project_dir)?.sessions)
}

#[derive(Debug, Clone, Default)]
pub struct QuestChatListResult {
    pub sessions: Vec<QuestChatSession>,
    pub corrupt_skipped: u32,
}

pub fn list_quest_chats_detailed(project_dir: &Path) -> Result<QuestChatListResult, String> {
    let dir = quest_chats_dir(project_dir);
    if !dir.exists() {
        return Ok(QuestChatListResult::default());
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
        match serde_json::from_str::<QuestChatSession>(&text) {
            Ok(session) => sessions.push(session),
            Err(_) => corrupt_skipped += 1,
        }
    }
    sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(QuestChatListResult {
        sessions,
        corrupt_skipped,
    })
}

pub fn save_quest_chat(project_dir: &Path, session: &QuestChatSession) -> Result<PathBuf, String> {
    let path = chat_file_path(project_dir, &session.id)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(session).map_err(|e| e.to_string())?;
    atomic_write(&path, text)?;
    Ok(path)
}

pub fn load_quest_chat(project_dir: &Path, chat_id: &str) -> Result<QuestChatSession, String> {
    let path = chat_file_path(project_dir, chat_id)?;
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

pub fn delete_quest_chat(project_dir: &Path, chat_id: &str) -> Result<(), String> {
    let path = chat_file_path(project_dir, chat_id)?;
    if path.exists() {
        std::fs::remove_file(path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn new_quest_chat_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("qchat-{ms}")
}

pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_path_traversal_chat_ids() {
        for bad in ["../x", "..\\x", "a/b", "a\\b", "", "foo\0bar", ".."] {
            assert!(validate_chat_id(bad).is_err(), "expected err for {bad:?}");
        }
        assert!(validate_chat_id("qchat-123").is_ok());
    }

    #[test]
    fn chat_file_path_stays_in_dir() {
        let dir = tempfile::tempdir().unwrap();
        let path = chat_file_path(dir.path(), "qchat-1").unwrap();
        assert_eq!(
            path,
            dir.path()
                .join(".tuffbox")
                .join("quest_chats")
                .join("qchat-1.json")
        );
        assert!(chat_file_path(dir.path(), "../escape").is_err());
    }

    #[test]
    fn save_load_roundtrip_atomic() {
        let dir = tempfile::tempdir().unwrap();
        let session = QuestChatSession {
            id: "qchat-round".into(),
            title: "t".into(),
            messages: vec![],
            pending_plan: None,
            updated_at: "1".into(),
        };
        let path = save_quest_chat(dir.path(), &session).unwrap();
        assert!(path.exists());
        let loaded = load_quest_chat(dir.path(), "qchat-round").unwrap();
        assert_eq!(loaded.title, "t");
    }
}
