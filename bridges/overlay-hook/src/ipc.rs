//! HTTP client to the TuffBox launcher overlay IPC proxy.

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::{json, Value};

static IPC_BASE: Lazy<Mutex<String>> = Lazy::new(|| {
    let ep = std::env::var("TUFFBOX_OVERLAY_IPC").unwrap_or_else(|_| "127.0.0.1:8799".into());
    let base = if ep.starts_with("http") {
        ep
    } else {
        format!("http://{ep}")
    };
    Mutex::new(base)
});

pub fn log_debug(msg: &str) -> Result<(), String> {
    let _ = get_json("/health");
    eprintln!("[tuffbox-overlay] {msg}");
    Ok(())
}

pub fn ipc_base() -> String {
    IPC_BASE.lock().clone()
}

pub fn get_json(path: &str) -> Result<Value, String> {
    let url = format!("{}{}", ipc_base(), path);
    let resp = ureq::get(&url)
        .timeout(std::time::Duration::from_secs(8))
        .call()
        .map_err(|e| e.to_string())?;
    resp.into_json().map_err(|e| e.to_string())
}

pub fn post_json(path: &str, body: &Value) -> Result<Value, String> {
    let url = format!("{}{}", ipc_base(), path);
    // send_json serialises with serde_json (UTF-8). Explicit charset helps
    // anything that sniffs the request on the way to the launcher proxy.
    let resp = ureq::post(&url)
        .timeout(std::time::Duration::from_secs(12))
        .set("Content-Type", "application/json; charset=utf-8")
        .set("Accept", "application/json")
        .set("Accept-Charset", "utf-8")
        .send_json(body.clone())
        .map_err(|e| e.to_string())?;
    resp.into_json().map_err(|e| e.to_string())
}

// ── YouTube ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Default)]
pub struct FeedItem {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub thumbnail_url: String,
    #[serde(default)]
    pub channel: String,
}

pub fn fetch_youtube_feed() -> Vec<FeedItem> {
    match get_json("/youtube-feed") {
        Ok(v) => v
            .get("items")
            .and_then(|i| i.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|row| serde_json::from_value(row.clone()).ok())
                    .collect()
            })
            .unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

pub fn resolve_youtube(id: &str) -> Option<String> {
    let path = format!("/youtube-resolve?id={id}");
    get_json(&path)
        .ok()
        .and_then(|v| v.get("url").and_then(|u| u.as_str()).map(|s| s.to_string()))
}

// ── Session ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct SessionInfo {
    pub username: String,
    pub uuid: String,
    pub pack_name: String,
}

pub fn fetch_session() -> SessionInfo {
    match get_json("/session") {
        Ok(v) => SessionInfo {
            username: v
                .get("username")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
            uuid: v
                .get("uuid")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
            pack_name: v
                .get("packName")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
        },
        Err(_) => SessionInfo::default(),
    }
}

// ── Friends ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct Friend {
    pub id: i64,
    pub key: String,
    pub name: String,
    pub since: String,
    pub online: bool,
    pub pack: String,
    pub server: String,
}

#[derive(Debug, Clone, Default)]
pub struct FriendsSnapshot {
    pub ok: bool,
    pub friends: Vec<Friend>,
    pub incoming: Vec<Friend>,
    pub outgoing: Vec<Friend>,
    pub error: String,
}

fn parse_friend(row: &Value) -> Friend {
    Friend {
        id: row.get("id").and_then(|v| v.as_i64()).unwrap_or(0),
        key: row
            .get("key")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        name: row
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        since: row
            .get("since")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        online: row.get("online").and_then(|v| v.as_bool()).unwrap_or(false),
        pack: row
            .get("pack")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        server: row
            .get("server")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
    }
}

fn parse_friend_list(v: &Value, key: &str) -> Vec<Friend> {
    v.get(key)
        .and_then(|a| a.as_array())
        .map(|arr| arr.iter().map(parse_friend).collect())
        .unwrap_or_default()
}

pub fn fetch_friends() -> FriendsSnapshot {
    match get_json("/friends") {
        Ok(v) => {
            if v.get("error").is_some() && !v.get("ok").and_then(|o| o.as_bool()).unwrap_or(false)
            {
                return FriendsSnapshot {
                    error: v
                        .get("error")
                        .and_then(|e| e.as_str())
                        .unwrap_or("friends error")
                        .to_string(),
                    ..Default::default()
                };
            }
            FriendsSnapshot {
                ok: v.get("ok").and_then(|o| o.as_bool()).unwrap_or(true),
                friends: parse_friend_list(&v, "friends"),
                incoming: parse_friend_list(&v, "incoming"),
                outgoing: parse_friend_list(&v, "outgoing"),
                error: String::new(),
            }
        }
        Err(e) => FriendsSnapshot {
            error: e,
            ..Default::default()
        },
    }
}

/// Merge presence online flags into an existing friends snapshot.
pub fn apply_presence(snap: &mut FriendsSnapshot) {
    let Ok(v) = get_json("/presence") else {
        return;
    };
    let Some(arr) = v.get("friends").and_then(|a| a.as_array()) else {
        return;
    };
    // Reset then set online from presence rows.
    for f in snap
        .friends
        .iter_mut()
        .chain(snap.incoming.iter_mut())
        .chain(snap.outgoing.iter_mut())
    {
        f.online = false;
        f.pack.clear();
        f.server.clear();
    }
    for row in arr {
        let key = row
            .get("key")
            .and_then(|k| k.as_str())
            .unwrap_or("")
            .to_string();
        if key.is_empty() {
            continue;
        }
        let pack = row
            .get("pack")
            .and_then(|p| p.as_str())
            .unwrap_or("")
            .to_string();
        let server = row
            .get("server")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string();
        let name = row
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("")
            .to_string();
        for f in snap.friends.iter_mut() {
            if f.key == key {
                f.online = true;
                f.pack = pack.clone();
                f.server = server.clone();
                if f.name.is_empty() && !name.is_empty() {
                    f.name = name.clone();
                }
            }
        }
    }
}

pub fn friends_add(username: &str) -> Result<Value, String> {
    post_json(
        "/friends/action",
        &json!({
            "action": "add",
            "friendUsername": username,
        }),
    )
}

pub fn friends_accept(friendship_id: i64) -> Result<Value, String> {
    post_json(
        "/friends/action",
        &json!({
            "action": "accept",
            "friendshipId": friendship_id,
        }),
    )
}

pub fn friends_remove(friendship_id: i64) -> Result<Value, String> {
    post_json(
        "/friends/action",
        &json!({
            "action": "remove",
            "friendshipId": friendship_id,
        }),
    )
}

// ── Chat ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct ChatMessage {
    pub id: i64,
    pub conversation: String,
    pub from_key: String,
    pub from_name: String,
    pub to_key: String,
    pub body: String,
    pub at: String,
}

#[derive(Debug, Clone, Default)]
pub struct ChatBatch {
    pub ok: bool,
    pub cursor: i64,
    pub messages: Vec<ChatMessage>,
    pub error: String,
}

fn parse_message(row: &Value) -> Option<ChatMessage> {
    let id = row.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
    if id <= 0 {
        return None;
    }
    Some(ChatMessage {
        id,
        conversation: row
            .get("conversation")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        from_key: row
            .get("fromKey")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        from_name: row
            .get("fromName")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        to_key: row
            .get("toKey")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        body: row
            .get("body")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        at: row
            .get("at")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
    })
}

pub fn fetch_chat(since_id: i64) -> ChatBatch {
    let path = format!("/chat?sinceId={since_id}");
    match get_json(&path) {
        Ok(v) => {
            if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
                if !v.get("ok").and_then(|o| o.as_bool()).unwrap_or(false) {
                    return ChatBatch {
                        error: err.to_string(),
                        cursor: since_id,
                        ..Default::default()
                    };
                }
            }
            let messages = v
                .get("messages")
                .and_then(|a| a.as_array())
                .map(|arr| arr.iter().filter_map(parse_message).collect())
                .unwrap_or_default();
            let cursor = v
                .get("cursor")
                .and_then(|c| c.as_i64())
                .unwrap_or_else(|| messages.last().map(|m| m.id).unwrap_or(since_id));
            ChatBatch {
                ok: true,
                cursor,
                messages,
                error: String::new(),
            }
        }
        Err(e) => ChatBatch {
            error: e,
            cursor: since_id,
            ..Default::default()
        },
    }
}

pub fn send_chat(to_key: &str, body: &str) -> Result<Value, String> {
    // Defense in depth: never ship a peer key / body that failed local checks.
    // (UI already sanitises; this guards other callers.)
    if to_key.len() < 8 || to_key.len() > 64 {
        return Err("invalid toKey".into());
    }
    if !to_key
        .bytes()
        .all(|b| b.is_ascii_hexdigit() || b == b'-')
    {
        return Err("invalid toKey charset".into());
    }
    if body.is_empty() || body.len() > 2000 || body.chars().count() > 500 {
        return Err("invalid body length".into());
    }
    // Reject raw control chars before they hit JSON (serde would escape them,
    // but we don't want them stored server-side either).
    if body.chars().any(|c| {
        let u = c as u32;
        (u < 0x20 && c != '\n' && c != '\t') || (0x7F..=0x9F).contains(&u)
    }) {
        return Err("body contains control characters".into());
    }
    // serde_json emits UTF-8 with proper \u escapes for controls — Content-Type
    // is application/json; charset is UTF-8 by default on the proxy.
    post_json(
        "/chat/send",
        &json!({
            "toKey": to_key,
            "body": body,
        }),
    )
}
