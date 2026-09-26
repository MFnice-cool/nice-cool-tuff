//! GitHub OAuth device flow for pack authors. Consumers stay anonymous.

use keyring::Entry;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const KEYRING_SERVICE: &str = "dev.tuffbox.ide";
const KEYRING_ACCOUNT: &str = "github-oauth-token";
const KEYRING_SIGNER: &str = "github-pack-signer-seed";
const DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
const TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const APP_USER_AGENT: &str = "TuffBox-IDE/0.1";
/// Public OAuth App client ID. Override at compile time with `TUFFBOX_GITHUB_CLIENT_ID`.
const GITHUB_CLIENT_ID: Option<&str> = option_env!("TUFFBOX_GITHUB_CLIENT_ID");

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubDeviceCodeInfo {
    pub user_code: String,
    pub verification_uri: String,
    pub message: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    interval: u64,
    #[serde(default)]
    message: Option<String>,
}

fn client_id() -> Result<&'static str, String> {
    GITHUB_CLIENT_ID
        .filter(|id| !id.is_empty())
        .ok_or_else(|| {
            "GitHub OAuth client ID is not configured. Set TUFFBOX_GITHUB_CLIENT_ID or paste a PAT in Settings.".into()
        })
}

fn oauth_entry() -> Result<Entry, String> {
    Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT).map_err(|e| e.to_string())
}

pub fn stored_author_token() -> Option<String> {
    oauth_entry()
        .ok()
        .and_then(|e| e.get_password().ok())
        .filter(|s| !s.trim().is_empty())
        .or_else(|| crate::integrations::secret_optional("github"))
}

pub fn author_signing_key() -> Result<tuffbox_core::github_pack::Ed25519KeyPair, String> {
    use tuffbox_core::github_pack::Ed25519KeyPair;
    let entry = Entry::new(KEYRING_SERVICE, KEYRING_SIGNER).map_err(|e| e.to_string())?;
    match entry.get_password() {
        Ok(seed) if !seed.trim().is_empty() => {
            Ed25519KeyPair::from_seed_b64(&seed).map_err(|e| e.to_string())
        }
        _ => {
            let key = Ed25519KeyPair::generate();
            let _ = entry.set_password(&key.to_seed_b64());
            Ok(key)
        }
    }
}

fn save_oauth_token(token: &str) -> Result<(), String> {
    oauth_entry()?
        .set_password(token.trim())
        .map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn github_pack_start_device_code() -> Result<GithubDeviceCodeInfo, String> {
    let client_id = client_id()?;
    let resp = reqwest::Client::new()
        .post(DEVICE_CODE_URL)
        .header("Accept", "application/json")
        .header("User-Agent", APP_USER_AGENT)
        .form(&[
            ("client_id", client_id),
            ("scope", "public_repo"),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("GitHub device code failed ({})", resp.status()));
    }
    let data: DeviceCodeResponse = resp.json().await.map_err(|e| e.to_string())?;
    save_pending_device_code(&data.device_code)?;
    Ok(GithubDeviceCodeInfo {
        user_code: data.user_code.clone(),
        verification_uri: data.verification_uri.clone(),
        message: data.message.unwrap_or_else(|| {
            format!(
                "Go to {} and enter code {}",
                data.verification_uri, data.user_code
            )
        }),
        expires_in: data.expires_in,
        interval: data.interval.max(5),
    })
}

fn pending_path() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("TuffBox")
        .join("github-device-code")
}

fn save_pending_device_code(code: &str) -> Result<(), String> {
    let path = pending_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, code).map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn github_pack_poll_device_code() -> Result<String, String> {
    let client_id = client_id()?;
    let device_code = std::fs::read_to_string(pending_path()).map_err(|_| {
        "no GitHub device-code session; start login first".to_string()
    })?;
    let resp = reqwest::Client::new()
        .post(TOKEN_URL)
        .header("Accept", "application/json")
        .header("User-Agent", APP_USER_AGENT)
        .form(&[
            ("client_id", client_id),
            ("device_code", device_code.trim()),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let body: Value = resp.json().await.map_err(|e| e.to_string())?;
    if let Some(token) = body.get("access_token").and_then(|v| v.as_str()) {
        save_oauth_token(token)?;
        let _ = std::fs::remove_file(pending_path());
        return Ok("ok".into());
    }
    let error = body.get("error").and_then(|v| v.as_str()).unwrap_or("unknown");
    match error {
        "authorization_pending" | "slow_down" => Err("authorization_pending".into()),
        "expired_token" => Err("Device code expired".into()),
        "access_denied" => Err("GitHub login was denied".into()),
        _ => Err(format!("GitHub login error: {error}")),
    }
}

#[tauri::command(rename_all = "camelCase")]
pub fn github_pack_auth_status() -> Result<bool, String> {
    Ok(stored_author_token().is_some())
}
