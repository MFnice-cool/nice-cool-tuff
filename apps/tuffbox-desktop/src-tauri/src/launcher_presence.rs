//! Community launcher online presence via Supabase (heartbeat + session log).

use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const HEARTBEAT_SECS: u64 = 30;
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

static RUNNING: AtomicBool = AtomicBool::new(false);

fn device_id() -> Result<String, String> {
    tuffbox_core::swarm::load_or_create_device_signing_key().map(|(_, id)| id)
}

fn display_name() -> Option<String> {
    let host = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .or_else(|_| std::env::var("USER"))
        .or_else(|_| std::env::var("USERNAME"))
        .ok()
        .unwrap_or_else(|| "user".into());
    let short: String = host.chars().take(24).collect();
    Some(format!("TuffBox · {short}"))
}

async fn heartbeat_once() -> Result<Value, String> {
    let url = crate::integrations::swarm_supabase_url()
        .ok_or_else(|| "Supabase URL missing".to_string())?;
    let key = crate::integrations::swarm_supabase_anon_key()
        .ok_or_else(|| "Supabase anon key missing".to_string())?;
    let id = device_id()?;
    tuffbox_core::swarm_supabase::launcher_heartbeat_supabase(
        &url,
        &key,
        &id,
        display_name().as_deref(),
        Some(APP_VERSION),
    )
    .await
}

async fn goodbye_once(reason: &str) -> Result<Value, String> {
    let url = crate::integrations::swarm_supabase_url()
        .ok_or_else(|| "Supabase URL missing".to_string())?;
    let key = crate::integrations::swarm_supabase_anon_key()
        .ok_or_else(|| "Supabase anon key missing".to_string())?;
    let id = device_id()?;
    tuffbox_core::swarm_supabase::launcher_goodbye_supabase(&url, &key, &id, reason).await
}

/// Start background heartbeats (idempotent).
pub fn start_presence_loop() {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let _ = heartbeat_once().await;
        while RUNNING.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_secs(HEARTBEAT_SECS)).await;
            if !RUNNING.load(Ordering::SeqCst) {
                break;
            }
            let _ = heartbeat_once().await;
        }
    });
}

/// Blocking-ish goodbye for process exit (wait up to 2s).
pub fn goodbye_on_exit() {
    RUNNING.store(false, Ordering::SeqCst);
    let _ = tauri::async_runtime::block_on(async {
        tokio::time::timeout(Duration::from_secs(2), goodbye_once("exit")).await
    });
}

#[tauri::command(rename_all = "camelCase")]
pub async fn launcher_presence_start() -> Result<Value, String> {
    start_presence_loop();
    heartbeat_once().await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn launcher_presence_stop() -> Result<Value, String> {
    RUNNING.store(false, Ordering::SeqCst);
    goodbye_once("ui_stop").await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_launcher_online() -> Result<Value, String> {
    let url = crate::integrations::swarm_supabase_url()
        .ok_or_else(|| "Supabase URL missing".to_string())?;
    let key = crate::integrations::swarm_supabase_anon_key()
        .ok_or_else(|| "Supabase anon key missing".to_string())?;
    tuffbox_core::swarm_supabase::launcher_online_stats_supabase(&url, &key).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_launcher_recent_sessions(limit: Option<u32>) -> Result<Value, String> {
    let url = crate::integrations::swarm_supabase_url()
        .ok_or_else(|| "Supabase URL missing".to_string())?;
    let key = crate::integrations::swarm_supabase_anon_key()
        .ok_or_else(|| "Supabase anon key missing".to_string())?;
    tuffbox_core::swarm_supabase::launcher_recent_sessions_supabase(
        &url,
        &key,
        limit.unwrap_or(50),
    )
    .await
}

/// Fire-and-forget: Minecraft process started.
pub fn spawn_game_session_start(project_name: String) {
    tauri::async_runtime::spawn(async move {
        let Ok(url) = crate::integrations::swarm_supabase_url()
            .ok_or_else(|| "missing".to_string())
        else {
            return;
        };
        let Ok(key) = crate::integrations::swarm_supabase_anon_key()
            .ok_or_else(|| "missing".to_string())
        else {
            return;
        };
        let Ok(id) = device_id() else { return };
        let _ = tuffbox_core::swarm_supabase::launcher_game_session_start_supabase(
            &url,
            &key,
            &id,
            Some(project_name.as_str()),
            Some(APP_VERSION),
        )
        .await;
    });
}

/// Fire-and-forget: Minecraft process exited.
pub fn spawn_game_session_end(duration_secs: u64, crashed: bool) {
    tauri::async_runtime::spawn(async move {
        let Ok(url) = crate::integrations::swarm_supabase_url()
            .ok_or_else(|| "missing".to_string())
        else {
            return;
        };
        let Ok(key) = crate::integrations::swarm_supabase_anon_key()
            .ok_or_else(|| "missing".to_string())
        else {
            return;
        };
        let Ok(id) = device_id() else { return };
        let reason = if crashed { "crash" } else { "exit" };
        let _ = tuffbox_core::swarm_supabase::launcher_game_session_end_supabase(
            &url,
            &key,
            &id,
            Some(duration_secs),
            reason,
        )
        .await;
    });
}

#[allow(dead_code)]
fn _offline_stub() -> Value {
    json!({ "onlineCount": 0, "online": [] })
}
