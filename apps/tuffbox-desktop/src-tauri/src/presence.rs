//! Discord Rich Presence + app-level presence settings.
//!
//! Uses `discord-rich-presence`. If Discord is not running or client ID is
//! unset, calls are no-ops so launch never fails because of RPC.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresenceSettings {
    #[serde(default)]
    pub discord_rpc_enabled: bool,
    /// Discord Application Client ID (create at https://discord.com/developers/applications).
    #[serde(default)]
    pub discord_client_id: String,
}

impl Default for PresenceSettings {
    fn default() -> Self {
        Self {
            discord_rpc_enabled: false,
            discord_client_id: String::new(),
        }
    }
}

fn settings_path() -> PathBuf {
    dirs::config_dir()
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("TuffBox")
        .join("presence_settings.json")
}

pub fn load_presence_settings() -> PresenceSettings {
    fs::read_to_string(settings_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_presence_settings(settings: &PresenceSettings) -> Result<(), String> {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(
        &path,
        serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

struct RpcState {
    client: Option<discord_rich_presence::DiscordIpcClient>,
    client_id: String,
}

static RPC: Mutex<RpcState> = Mutex::new(RpcState {
    client: None,
    client_id: String::new(),
});

fn ensure_client(client_id: &str) -> Result<(), String> {
    if client_id.is_empty() {
        return Err("Discord client ID not configured".into());
    }
    let mut state = RPC.lock().map_err(|e| e.to_string())?;
    if state.client.is_some() && state.client_id == client_id {
        return Ok(());
    }
    // Drop previous
    if let Some(mut old) = state.client.take() {
        let _ = discord_rich_presence::DiscordIpc::close(&mut old);
    }
    let mut client = discord_rich_presence::DiscordIpcClient::new(client_id)
        .map_err(|e| format!("Discord IPC create failed: {e}"))?;
    discord_rich_presence::DiscordIpc::connect(&mut client)
        .map_err(|e| format!("Discord IPC connect failed: {e}"))?;
    state.client = Some(client);
    state.client_id = client_id.to_string();
    Ok(())
}

pub fn set_playing_activity(instance_name: &str, detail: &str) -> Result<(), String> {
    let settings = load_presence_settings();
    if !settings.discord_rpc_enabled {
        return Ok(());
    }
    if let Err(e) = ensure_client(&settings.discord_client_id) {
        // Soft-fail: Discord may not be open.
        eprintln!("[presence] {e}");
        return Ok(());
    }
    let mut state = RPC.lock().map_err(|e| e.to_string())?;
    let Some(ref mut client) = state.client else {
        return Ok(());
    };
    use discord_rich_presence::activity;
    use discord_rich_presence::DiscordIpc;
    let payload = activity::Activity::new()
        .state(detail)
        .details(instance_name)
        .assets(
            activity::Assets::new()
                .large_text("TuffBox")
                .large_image("tuffbox"),
        );
    if let Err(e) = client.set_activity(payload) {
        eprintln!("[presence] set_activity failed: {e}");
    }
    Ok(())
}

pub fn clear_activity() -> Result<(), String> {
    let mut state = RPC.lock().map_err(|e| e.to_string())?;
    if let Some(ref mut client) = state.client {
        use discord_rich_presence::DiscordIpc;
        let _ = client.clear_activity();
    }
    Ok(())
}
