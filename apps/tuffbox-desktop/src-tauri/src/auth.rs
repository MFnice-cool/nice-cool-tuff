use keyring::Entry;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

const KEYRING_SERVICE: &str = "dev.tuffbox.ide";
/// Azure AD public client (device-code flow when microsoftonline is reachable).
const MICROSOFT_CLIENT_ID: &str = "89484d4e-6ac2-4643-a786-21386f3269c5";
/// Official Minecraft launcher public client — live.com auth-code / WebView login.
const MS_LIVE_CLIENT_ID: &str = "00000000402b5328";
const MS_OAUTH_TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const MS_OAUTH_AUTHORIZE_URL: &str =
    "https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize";
const MS_OAUTH_DEVICE_CODE_URL: &str =
    "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";
const MS_LIVE_AUTHORIZE_URL: &str = "https://login.live.com/oauth20_authorize.srf";
const MS_LIVE_TOKEN_URL: &str = "https://login.live.com/oauth20_token.srf";
const MS_REDIRECT_NATIVE: &str = "https://login.microsoftonline.com/common/oauth2/nativeclient";
const MS_REDIRECT_LIVE_DESKTOP: &str = "https://login.live.com/oauth20_desktop.srf";
const MS_SCOPE: &str = "XboxLive.signin offline_access";
const MC_PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";
const MS_AUTH_WINDOW_LABEL: &str = "ms-auth";

// Mutex protecting concurrent reads/writes to auth.json and mc_accounts.json.
// Lock is held only during file I/O (brief), so a std::sync::Mutex is fine
// even in async context.
static AUTH_FILE_MUTEX: Mutex<()> = Mutex::new(());

// Cache for mc_get_auth_status: prevents network refresh on every call.
// The frontend polls this on every focus/navigation, so we skip the refresh
// if less than 30 seconds have elapsed since the last successful one.
static LAST_AUTH_REFRESH: Mutex<Option<Instant>> = Mutex::new(None);
const AUTH_REFRESH_TTL: Duration = Duration::from_secs(30);

// ─── Skin source ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SkinSource {
    Mojang,
    Elyby,
    TLauncher,
    Offline,
}

/// Which cape texture to show on the 3D preview (only one at a time).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CapeProvider {
    Mojang,
    Optifine,
    TLauncher,
    None,
}

impl Default for CapeProvider {
    fn default() -> Self {
        Self::Mojang
    }
}

impl std::fmt::Display for CapeProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CapeProvider::Mojang => write!(f, "Mojang"),
            CapeProvider::Optifine => write!(f, "OptiFine"),
            CapeProvider::TLauncher => write!(f, "TLauncher"),
            CapeProvider::None => write!(f, "None"),
        }
    }
}

impl Default for SkinSource {
    fn default() -> Self {
        Self::Mojang
    }
}

impl std::fmt::Display for SkinSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SkinSource::Mojang => write!(f, "Mojang"),
            SkinSource::Elyby => write!(f, "Ely.by"),
            SkinSource::TLauncher => write!(f, "TLauncher"),
            SkinSource::Offline => write!(f, "Offline"),
        }
    }
}

// ─── Login type ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LoginType {
    Microsoft,
    Offline,
    /// Third-party Yggdrasil / authlib-injector (Ely.by, LittleSkin, custom).
    Yggdrasil,
}

impl Default for LoginType {
    fn default() -> Self {
        Self::Offline
    }
}

// ─── Token storage ────────────────────────────────────────────────

fn keyring_entry(name: &str) -> Result<Entry, String> {
    Entry::new(KEYRING_SERVICE, name).map_err(|e| e.to_string())
}

fn save_token(name: &str, value: &str) -> Result<(), String> {
    keyring_entry(name)?
        .set_password(value)
        .map_err(|e| e.to_string())
}

fn load_token(name: &str) -> Result<String, String> {
    keyring_entry(name)?
        .get_password()
        .map_err(|_| "not logged in".to_string())
}

fn clear_token(name: &str) -> Result<(), String> {
    let entry = keyring_entry(name)?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

// ─── Per-account token helpers ────────────────────────────────────

fn account_refresh_key(uuid: &str) -> String {
    format!("mc-refresh-{uuid}")
}

fn account_access_key(uuid: &str) -> String {
    format!("mc-access-{uuid}")
}

// ─── Types ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McProfile {
    pub uuid: String,
    pub name: String,
    pub skin_url: Option<String>,
    pub cape_url: Option<String>,
    #[serde(default)]
    pub capes: Vec<McCapeEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McCapeEntry {
    pub id: String,
    pub alias: Option<String>,
    pub url: String,
    pub state: String,
}

/// Microsoft identity platform returns snake_case OAuth fields (not camelCase).
#[derive(Debug, Serialize, Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    #[serde(default)]
    verification_uri_complete: Option<String>,
    expires_in: u64,
    interval: u64,
    #[serde(default)]
    message: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    expires_in: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct McProfileResponse {
    id: String,
    name: String,
    skins: Option<Vec<McSkinEntry>>,
    capes: Option<Vec<McCapeRawEntry>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct McSkinEntry {
    id: String,
    state: String,
    url: String,
    variant: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct McCapeRawEntry {
    id: String,
    alias: Option<String>,
    url: String,
    state: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TextureResponse {
    textures: TexturesData,
}

#[derive(Debug, Serialize, Deserialize)]
struct TexturesData {
    #[serde(alias = "SKIN")]
    skin: Option<TextureInfo>,
    #[serde(alias = "CAPE")]
    cape: Option<TextureInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
struct TextureInfo {
    url: String,
}

// ─── Multi-account types ─────────────────────────────────────────

/// Which Microsoft OAuth endpoint issued the refresh token.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum MsOauthBackend {
    #[default]
    Azure,
    Live,
}

impl MsOauthBackend {
    fn client_id(self) -> &'static str {
        match self {
            Self::Azure => MICROSOFT_CLIENT_ID,
            Self::Live => MS_LIVE_CLIENT_ID,
        }
    }

    fn token_url(self) -> &'static str {
        match self {
            Self::Azure => MS_OAUTH_TOKEN_URL,
            Self::Live => MS_LIVE_TOKEN_URL,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountEntry {
    pub uuid: String,
    pub name: String,
    pub login_type: LoginType,
    pub skin_source: SkinSource,
    pub added_at: u64,
    /// Yggdrasil / authlib-injector API root (e.g. Ely.by, LittleSkin).
    #[serde(default)]
    pub authority: Option<String>,
    /// Microsoft OAuth backend for refresh (`azure` device-code vs `live` WebView/paste).
    #[serde(default)]
    pub ms_oauth_backend: Option<MsOauthBackend>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountsFile {
    pub active_account_uuid: Option<String>,
    pub accounts: Vec<AccountEntry>,
}

impl Default for AccountsFile {
    fn default() -> Self {
        Self {
            active_account_uuid: None,
            accounts: Vec::new(),
        }
    }
}

// ─── Auth state persisted to disk ────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AuthState {
    pub logged_in: bool,
    pub profile: Option<McProfile>,
    pub expires_at: Option<u64>,
    #[serde(default)]
    pub login_type: LoginType,
    #[serde(default)]
    pub skin_source: SkinSource,
    /// Selected cape provider for the 3D preview (mutually exclusive).
    #[serde(default)]
    pub cape_provider: CapeProvider,
    #[serde(default)]
    pub accounts: Vec<AccountEntry>,
    pub active_account_uuid: Option<String>,
}

/// One cape candidate discovered from Mojang / OptiFine / TLauncher.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapeOffer {
    pub provider: CapeProvider,
    /// Mojang cape id when `provider == Mojang`, otherwise a stable key.
    pub id: String,
    pub label: String,
    pub url: String,
    /// True when this cape can be activated on the Mojang account.
    pub can_activate: bool,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapeCatalog {
    pub selected_provider: CapeProvider,
    pub display_url: Option<String>,
    pub offers: Vec<CapeOffer>,
}

fn accounts_path() -> PathBuf {
    dirs::config_dir()
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("TuffBox")
        .join("mc_accounts.json")
}

fn auth_state_path() -> PathBuf {
    dirs::config_dir()
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("TuffBox")
        .join("mc_auth.json")
}

fn load_accounts_file() -> AccountsFile {
    let _guard = AUTH_FILE_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    fs::read_to_string(accounts_path())
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_accounts_file(data: &AccountsFile) -> Result<(), String> {
    let _guard = AUTH_FILE_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let path = accounts_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(
        &tmp,
        serde_json::to_vec_pretty(data).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    Ok(())
}

fn load_auth_state() -> AuthState {
    let _guard = AUTH_FILE_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    fs::read_to_string(auth_state_path())
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// Disk-only auth snapshot (no Microsoft refresh) for home bootstrap P0.
pub(crate) fn cached_auth_state() -> AuthState {
    let mut state = load_auth_state();
    let accounts = load_accounts_file();
    state.accounts = accounts.accounts;
    state.active_account_uuid = accounts.active_account_uuid;
    state
}

fn save_auth_state(state: &AuthState) -> Result<(), String> {
    let _guard = AUTH_FILE_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let path = auth_state_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(
        &tmp,
        serde_json::to_vec_pretty(state).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    Ok(())
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ─── HTTP helpers ────────────────────────────────────────────────

fn client() -> Result<Client, String> {
    Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .map_err(|e| e.to_string())
}

// ─── Offline UUID ────────────────────────────────────────────────

fn offline_uuid(name: &str) -> String {
    use md5::{Digest, Md5};
    let mut hasher = Md5::new();
    hasher.update(b"OfflinePlayer:");
    hasher.update(name.as_bytes());
    let mut bytes: [u8; 16] = hasher.finalize().into();
    bytes[6] = (bytes[6] & 0x0f) | 0x30;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
    )
}

// ─── Skin fetching from multiple sources ─────────────────────────

async fn fetch_skin_elyby(username: &str) -> Option<String> {
    let c = client().ok()?;
    let url = format!("http://skinsystem.ely.by/skins/{username}.png");
    let resp = c.get(&url).send().await.ok()?;
    if resp.status().is_success() {
        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if content_type.contains("image") {
            return Some(url);
        }
    }
    let url = format!("http://skinsystem.ely.by/textures/{username}");
    let resp = c.get(&url).send().await.ok()?;
    if resp.status().is_success() {
        let ct = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if ct.contains("image") {
            return Some(url);
        }
    }
    None
}

fn prefer_https(url: &str) -> String {
    if let Some(rest) = url.strip_prefix("http://") {
        format!("https://{rest}")
    } else {
        url.to_string()
    }
}

/// TLauncher texture API (CustomSkinLoader / auth.tlauncher.org).
/// Old `www.tlauncher.org/skins/{name}.png` now returns HTML, not a skin.
async fn fetch_tlauncher_textures(username: &str) -> (Option<String>, Option<String>) {
    let Ok(c) = client() else {
        return (None, None);
    };
    let lookup = format!("https://auth.tlauncher.org/skin/profile/texture/login/{username}");
    let Ok(resp) = c.get(&lookup).send().await else {
        return (None, None);
    };
    if !resp.status().is_success() {
        return (None, None);
    }
    let Ok(body) = resp.json::<Value>().await else {
        return (None, None);
    };

    let skin = body
        .pointer("/SKIN/url")
        .or_else(|| body.pointer("/skin/url"))
        .and_then(|v| v.as_str())
        .map(prefer_https);
    let cape = body
        .pointer("/CAPE/url")
        .or_else(|| body.pointer("/cape/url"))
        .and_then(|v| v.as_str())
        .map(prefer_https);

    (skin, cape)
}

async fn fetch_skin_tlauncher(username: &str) -> Option<String> {
    let (skin, _) = fetch_tlauncher_textures(username).await;
    if let Some(url) = skin {
        if probe_image_url(&url).await.is_some() {
            return Some(url);
        }
        // Texture URL may still be valid even if HEAD/GET probe is picky.
        return Some(url);
    }
    // Fallback: fileservice uses a lowercased nick.
    let direct = format!(
        "https://auth.tlauncher.org/skin/fileservice/skins/skin_{}.png",
        username.to_lowercase()
    );
    probe_image_url(&direct).await
}

async fn fetch_skin_mojang(uuid: &str) -> Option<String> {
    let c = client().ok()?;
    let resp = c
        .get(format!(
            "https://sessionserver.mojang.com/session/minecraft/profile/{uuid}"
        ))
        .send()
        .await
        .ok()?;

    if !resp.status().is_success() {
        return None;
    }

    let body: Value = resp.json().await.ok()?;
    let texture_b64 = body
        .get("properties")
        .and_then(|p| p.as_array())
        .and_then(|props| {
            props
                .iter()
                .find(|p| p.get("name").and_then(|n| n.as_str()) == Some("textures"))
        })
        .and_then(|p| p.get("value"))
        .and_then(|v| v.as_str())?;

    let decoded = base64_decode(texture_b64)?;
    let tex: TextureResponse = serde_json::from_str(&decoded).ok()?;
    tex.textures.skin.map(|s| s.url)
}

async fn fetch_skin_for_username(username: &str, source: &SkinSource) -> Option<String> {
    match source {
        SkinSource::Elyby => fetch_skin_elyby(username).await,
        SkinSource::TLauncher => fetch_skin_tlauncher(username).await,
        SkinSource::Mojang => {
            let c = client().ok()?;
            let resp = c
                .get(format!(
                    "https://api.mojang.com/users/profiles/minecraft/{username}"
                ))
                .send()
                .await
                .ok()?;
            if resp.status().is_success() {
                let body: Value = resp.json().await.ok()?;
                if let Some(uuid) = body.get("id").and_then(|v| v.as_str()) {
                    return fetch_skin_mojang(uuid).await;
                }
            }
            None
        }
        SkinSource::Offline => None,
    }
}

/// Short-timeout client for cheap existence probes (capes can be dead URLs).
fn probe_client() -> Result<Client, String> {
    // Task: cape probes chained HEAD→GET with 8s timeouts made first load hang
    // for tens of seconds when a host was slow/down. 4s is plenty for a probe.
    Client::builder()
        .timeout(Duration::from_secs(4))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|e| e.to_string())
}

/// Probe whether a remote URL returns an image (used for OptiFine / TL capes).
///
/// Uses HEAD first — it costs one round-trip instead of downloading the whole
/// PNG. Falls back to a header-only GET (body is never read) for hosts that
/// reject HEAD.
async fn probe_image_url(url: &str) -> Option<String> {
    let c = probe_client().ok()?;
    let resp = match c.head(url).send().await {
        Ok(r) if r.status().is_success() => Some(r),
        _ => c.get(url).send().await.ok(),
    };
    let resp = resp.filter(|r| r.status().is_success())?;
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if ct.contains("image") || ct.contains("octet-stream") || ct.is_empty() {
        Some(url.to_string())
    } else {
        None
    }
}

async fn fetch_cape_optifine(username: &str) -> Option<String> {
    let https = format!("https://optifine.net/capes/{username}.png");
    if let Some(u) = probe_image_url(&https).await {
        return Some(u);
    }
    let http = format!("http://s.optifine.net/capes/{username}.png");
    probe_image_url(&http).await
}

async fn fetch_cape_tlauncher(username: &str) -> Option<String> {
    let (_, cape) = fetch_tlauncher_textures(username).await;
    if let Some(url) = cape {
        return Some(url);
    }
    let mut direct = format!(
        "https://auth.tlauncher.org/skin/fileservice/cloaks/cloak_{}.png",
        username.to_lowercase()
    );
    if let Some(url) = probe_image_url(&direct).await {
        return Some(url);
    }
    direct = format!(
        "https://auth.tlauncher.org/skin/fileservice/capes/cape_{}.png",
        username.to_lowercase()
    );
    probe_image_url(&direct).await
}

async fn fetch_cape_mojang_session(uuid: &str) -> Option<String> {
    let c = client().ok()?;
    let resp = c
        .get(format!(
            "https://sessionserver.mojang.com/session/minecraft/profile/{uuid}"
        ))
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body: Value = resp.json().await.ok()?;
    let texture_b64 = body
        .get("properties")
        .and_then(|p| p.as_array())
        .and_then(|props| {
            props
                .iter()
                .find(|p| p.get("name").and_then(|n| n.as_str()) == Some("textures"))
        })
        .and_then(|p| p.get("value"))
        .and_then(|v| v.as_str())?;
    let decoded = base64_decode(texture_b64)?;
    let tex: TextureResponse = serde_json::from_str(&decoded).ok()?;
    tex.textures.cape.map(|c| c.url)
}

async fn resolve_display_cape(
    username: &str,
    uuid: &str,
    provider: &CapeProvider,
    mojang_owned: &[McCapeEntry],
) -> Option<String> {
    match provider {
        CapeProvider::None => None,
        CapeProvider::Mojang => mojang_owned
            .iter()
            .find(|c| c.state.eq_ignore_ascii_case("ACTIVE"))
            .map(|c| c.url.clone())
            .or_else(|| {
                mojang_owned
                    .first()
                    .map(|c| c.url.clone())
            })
            .or(fetch_cape_mojang_session(uuid).await),
        CapeProvider::Optifine => fetch_cape_optifine(username).await,
        CapeProvider::TLauncher => fetch_cape_tlauncher(username).await,
    }
}

async fn build_cape_catalog(
    username: &str,
    uuid: &str,
    selected: CapeProvider,
    mojang_owned: &[McCapeEntry],
) -> CapeCatalog {
    let mut offers = Vec::new();

    // Probe all external cape sources in parallel — this is the slow part and
    // was previously done sequentially (twice, via resolve_display_cape).
    let (mojang_session, optifine_url, tlauncher_url) = tokio::join!(
        fetch_cape_mojang_session(uuid),
        fetch_cape_optifine(username),
        fetch_cape_tlauncher(username),
    );

    for cape in mojang_owned {
        offers.push(CapeOffer {
            provider: CapeProvider::Mojang,
            id: cape.id.clone(),
            label: cape
                .alias
                .clone()
                .unwrap_or_else(|| "Mojang cape".into()),
            url: cape.url.clone(),
            can_activate: true,
            active: cape.state.eq_ignore_ascii_case("ACTIVE"),
        });
    }
    if mojang_owned.is_empty() {
        if let Some(ref url) = mojang_session {
            offers.push(CapeOffer {
                provider: CapeProvider::Mojang,
                id: "mojang-session".into(),
                label: "Mojang cape".into(),
                url: url.clone(),
                can_activate: false,
                active: selected == CapeProvider::Mojang,
            });
        }
    }

    if let Some(ref url) = optifine_url {
        offers.push(CapeOffer {
            provider: CapeProvider::Optifine,
            id: "optifine".into(),
            label: "OptiFine cape".into(),
            url: url.clone(),
            can_activate: false,
            active: selected == CapeProvider::Optifine,
        });
    }

    if let Some(ref url) = tlauncher_url {
        offers.push(CapeOffer {
            provider: CapeProvider::TLauncher,
            id: "tlauncher".into(),
            label: "TLauncher cape".into(),
            url: url.clone(),
            can_activate: false,
            active: selected == CapeProvider::TLauncher,
        });
    }

    offers.push(CapeOffer {
        provider: CapeProvider::None,
        id: "none".into(),
        label: "No cape".into(),
        url: String::new(),
        can_activate: true,
        active: selected == CapeProvider::None,
    });

    // Derive the display URL from what we already probed — no second network pass.
    let display_url = match &selected {
        CapeProvider::None => None,
        CapeProvider::Mojang => mojang_owned
            .iter()
            .find(|c| c.state.eq_ignore_ascii_case("ACTIVE"))
            .map(|c| c.url.clone())
            .or_else(|| mojang_owned.first().map(|c| c.url.clone()))
            .or(mojang_session),
        CapeProvider::Optifine => optifine_url,
        CapeProvider::TLauncher => tlauncher_url,
    };

    CapeCatalog {
        selected_provider: selected,
        display_url,
        offers,
    }
}

fn base64_decode(input: &str) -> Option<String> {
    use base64::Engine;
    let engine = base64::engine::general_purpose::STANDARD;
    let decoded = engine.decode(input).ok()?;
    String::from_utf8(decoded).ok()
}

// ─── Device code flow ────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCodeInfo {
    pub user_code: String,
    pub verification_uri: String,
    /// One-click login URL (`verification_uri?otc=user_code`) — same trick MinecraftAuth uses.
    /// Uses Mojang-approved public client IDs; no Azure app registration required.
    pub login_url: String,
    pub message: String,
    pub expires_in: u64,
    /// Suggested poll interval in seconds (from Microsoft).
    pub interval: u64,
}

pub async fn start_device_code_flow() -> Result<(DeviceCodeInfo, String, u64), String> {
    let c = client()?;
    let resp = c
        .post(MS_OAUTH_DEVICE_CODE_URL)
        .form(&[
            ("client_id", MICROSOFT_CLIENT_ID),
            ("scope", MS_SCOPE),
        ])
        .send()
        .await
        .map_err(|e| format!("device code request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("device code failed ({status}): {body}"));
    }

    let data: DeviceCodeResponse = resp
        .json()
        .await
        .map_err(|e| format!("device code parse failed: {e}"))?;
    let verification_uri = data
        .verification_uri_complete
        .as_deref()
        .filter(|u| !u.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| data.verification_uri.clone());
    // Prefer Microsoft's complete URI when present; otherwise append ?otc= (MinecraftAuth style).
    let login_url = if data
        .verification_uri_complete
        .as_deref()
        .map(|u| !u.is_empty())
        .unwrap_or(false)
    {
        verification_uri.clone()
    } else {
        format!(
            "{}{}otc={}",
            data.verification_uri,
            if data.verification_uri.contains('?') {
                "&"
            } else {
                "?"
            },
            urlencoding_encode(&data.user_code)
        )
    };
    let info = DeviceCodeInfo {
        user_code: data.user_code.clone(),
        verification_uri: data.verification_uri.clone(),
        login_url: login_url.clone(),
        message: data.message.unwrap_or_else(|| {
            format!(
                "Open {} to sign in (code {})",
                login_url, data.user_code
            )
        }),
        expires_in: data.expires_in,
        interval: data.interval.max(1),
    };
    Ok((info, data.device_code, data.interval.max(1)))
}

/// One token poll attempt. Returns `Err("authorization_pending")` while the user
/// has not finished the device-code flow yet (frontend should retry).
pub async fn poll_device_code_token_once(device_code: &str) -> Result<TokenResponse, String> {
    let c = client()?;
    let resp = c
        .post(MS_OAUTH_TOKEN_URL)
        .form(&[
            ("client_id", MICROSOFT_CLIENT_ID),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("device_code", device_code),
        ])
        .send()
        .await
        .map_err(|e| format!("token poll failed: {e}"))?;

    let status = resp.status();
    let body: Value = resp.json().await.map_err(|e| e.to_string())?;

    if status.is_success() {
        return serde_json::from_value(body)
            .map_err(|e| format!("token response parse failed: {e}"));
    }

    let error = body
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    match error {
        "authorization_pending" | "slow_down" => Err("authorization_pending".into()),
        "authorization_declined" => Err("Login was declined".to_string()),
        "expired_token" => Err("Device code expired".to_string()),
        "bad_verification_code" => Err("Invalid device code".to_string()),
        _ => {
            let desc = body
                .get("error_description")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error");
            Err(format!("Login error: {error}: {desc}"))
        }
    }
}

#[allow(dead_code)]
pub async fn poll_device_code_token(
    device_code: &str,
    interval: u64,
) -> Result<TokenResponse, String> {
    let start = Instant::now();
    let max_wait = Duration::from_secs(900);
    let step = interval.max(1);

    loop {
        if start.elapsed() > max_wait {
            return Err("Login timed out".to_string());
        }
        match poll_device_code_token_once(device_code).await {
            Ok(token) => return Ok(token),
            Err(e) if e == "authorization_pending" => {
                tokio::time::sleep(Duration::from_secs(step)).await;
            }
            Err(e) => return Err(e),
        }
    }
}

// ─── Full auth chain: MS → XBL → XSTS → MC ──────────────────────

async fn authenticate_with_xbl(ms_token: &str) -> Result<(String, String), String> {
    let c = client()?;
    let resp = c
        .post("https://user.auth.xboxlive.com/user/authenticate")
        .json(&serde_json::json!({
            "Properties": {
                "AuthMethod": "RPS",
                "SiteName": "user.auth.xboxlive.com",
                "RpsTicket": format!("d={ms_token}")
            },
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT"
        }))
        .send()
        .await
        .map_err(|e| format!("XBL auth failed: {e}"))?;

    let body: Value = resp.json().await.map_err(|e| e.to_string())?;
    let token = body
        .get("Token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "XBL response missing token".to_string())?
        .to_string();
    let userhash = body
        .get("DisplayClaims")
        .and_then(|dc| dc.get("xui"))
        .and_then(|xui| xui.as_array())
        .and_then(|arr| arr.first())
        .and_then(|claim| claim.get("uhs"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "XBL response missing userhash".to_string())?
        .to_string();
    Ok((token.to_string(), userhash))
}

async fn authenticate_with_xsts(xbl_token: &str) -> Result<String, String> {
    let c = client()?;
    let resp = c
        .post("https://xsts.auth.xboxlive.com/xsts/authorize")
        .json(&serde_json::json!({
            "Properties": {
                "SandboxId": "RETAIL",
                "UserTokens": [xbl_token]
            },
            "RelyingParty": "rp://api.minecraftservices.com/",
            "TokenType": "JWT"
        }))
        .send()
        .await
        .map_err(|e| format!("XSTS auth failed: {e}"))?;

    let body: Value = resp.json().await.map_err(|e| e.to_string())?;

    if let Some(err) = body.get("XErr") {
        let err_code = err.as_u64().unwrap_or(0);
        let message = body
            .get("Message")
            .and_then(|v| v.as_str())
            .unwrap_or("XSTS authorization failed");
        return Err(format!(
            "XSTS error {err_code}: {message}. Ensure your Microsoft account has Xbox Live access."
        ));
    }

    body.get("Token")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "XSTS response missing token".to_string())
}

async fn authenticate_with_minecraft(userhash: &str, xsts_token: &str) -> Result<String, String> {
    let c = client()?;

    // Try launcher/login first (more reliable), fallback to login_with_xbox
    let identity_token = format!("XBL3.0 x={userhash};{xsts_token}");

    let resp = c
        .post("https://api.minecraftservices.com/launcher/login")
        .json(&serde_json::json!({
            "identityToken": identity_token
        }))
        .send()
        .await;

    match resp {
        Ok(r) if r.status().is_success() => {
            let body: Value = r.json().await.map_err(|e| e.to_string())?;
            if let Some(token) = body.get("access_token").and_then(|v| v.as_str()) {
                return Ok(token.to_string());
            }
        }
        _ => {}
    }

    // Fallback: login_with_xbox
    let resp = c
        .post("https://api.minecraftservices.com/authentication/login_with_xbox")
        .json(&serde_json::json!({
            "identityToken": format!("XBL3.0 x={userhash};{xsts_token}")
        }))
        .send()
        .await
        .map_err(|e| format!("MC auth failed: {e}"))?;

    let body: Value = resp.json().await.map_err(|e| e.to_string())?;
    body.get("access_token")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "MC auth response missing access_token".to_string())
}

async fn fetch_mc_profile(mc_token: &str) -> Result<McProfile, String> {
    let c = client()?;
    let resp = c
        .get(MC_PROFILE_URL)
        .bearer_auth(mc_token)
        .send()
        .await
        .map_err(|e| format!("MC profile fetch failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("MC profile error ({status}): {body}"));
    }

    let data: McProfileResponse = resp.json().await.map_err(|e| e.to_string())?;

    let skin_url = data
        .skins
        .as_ref()
        .and_then(|skins| skins.iter().find(|s| s.url.contains("texture")))
        .map(|s| s.url.clone());

    let skin_url = match skin_url {
        Some(url) => Some(url),
        None => fetch_skin_mojang(&data.id).await,
    };

    let capes: Vec<McCapeEntry> = data
        .capes
        .as_ref()
        .map(|capes| {
            capes
                .iter()
                .map(|c| McCapeEntry {
                    id: c.id.clone(),
                    alias: c.alias.clone(),
                    url: c.url.clone(),
                    state: c.state.clone(),
                })
                .collect()
        })
        .unwrap_or_default();

    let cape_url = capes.iter().find(|c| c.state == "ACTIVE").map(|c| c.url.clone());

    Ok(McProfile {
        uuid: data.id,
        name: data.name,
        skin_url,
        cape_url,
        capes,
    })
}

// ─── Entitlement check ───────────────────────────────────────────

pub async fn check_minecraft_entitlement(mc_token: &str) -> Result<bool, String> {
    let c = client()?;
    let resp = c
        .get("https://api.minecraftservices.com/entitlements/mcstore")
        .bearer_auth(mc_token)
        .send()
        .await
        .map_err(|e| format!("entitlement check failed: {e}"))?;

    if !resp.status().is_success() {
        return Ok(false);
    }

    let body: Value = resp.json().await.map_err(|e| e.to_string())?;
    // Mojang / Game Pass variants seen in the wild.
    const OWNED: &[&str] = &[
        "game_minecraft",
        "product_minecraft",
        "product_minecraft_java",
        "game_minecraft_bedrock",
        "product_game_pass",
        "product_minecraft_java_realtime",
    ];
    let has_game = body
        .get("items")
        .and_then(|items| items.as_array())
        .map(|items| {
            items.iter().any(|item| {
                item.get("name")
                    .and_then(|n| n.as_str())
                    .map(|name| OWNED.iter().any(|o| *o == name) || name.contains("minecraft"))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false);
    Ok(has_game)
}

// ─── Skin upload ─────────────────────────────────────────────────

pub async fn apply_minecraft_skin(
    mc_token: &str,
    skin_url: &str,
    variant: &str,
) -> Result<(), String> {
    let c = client()?;
    let resp = c
        .post(format!("{MC_PROFILE_URL}/skins"))
        .bearer_auth(mc_token)
        .json(&serde_json::json!({
            "variant": variant,
            "url": skin_url
        }))
        .send()
        .await
        .map_err(|e| format!("skin upload failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("skin upload error ({status}): {body}"));
    }
    Ok(())
}

/// Upload a local PNG skin (base64, with or without data-URL prefix).
pub async fn upload_minecraft_skin_bytes(
    mc_token: &str,
    png_base64: &str,
    variant: &str,
) -> Result<(), String> {
    use base64::Engine;
    let raw = png_base64
        .split(',')
        .next_back()
        .unwrap_or(png_base64)
        .trim();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(raw)
        .map_err(|e| format!("invalid skin base64: {e}"))?;
    if bytes.len() < 100 || bytes.len() > 8 * 1024 * 1024 {
        return Err("Skin file size is invalid".into());
    }

    let form = reqwest::multipart::Form::new()
        .text("variant", variant.to_string())
        .part(
            "file",
            reqwest::multipart::Part::bytes(bytes)
                .file_name("skin.png")
                .mime_str("image/png")
                .map_err(|e| e.to_string())?,
        );

    let c = client()?;
    let resp = c
        .post(format!("{MC_PROFILE_URL}/skins"))
        .bearer_auth(mc_token)
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("skin upload failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("skin upload error ({status}): {body}"));
    }
    Ok(())
}

pub async fn apply_minecraft_cape(mc_token: &str, cape_id: &str) -> Result<(), String> {
    let c = client()?;
    // Modern Mojang Capes API: PUT /minecraft/profile/capes/active with a
    // JSON body selecting the cape by id. The legacy
    // `/capes/{id}/activate` path now returns 404 NOT_FOUND.
    let resp = c
        .put(format!("{MC_PROFILE_URL}/capes/active"))
        .bearer_auth(mc_token)
        .json(&serde_json::json!({ "capeId": cape_id }))
        .send()
        .await
        .map_err(|e| format!("cape activate failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        // 400 means the authenticated account does not own the requested cape.
        if status.as_u16() == 400 {
            return Err("This Minecraft account does not own that cape.".into());
        }
        return Err(format!("cape activate error ({status}): {body}"));
    }
    Ok(())
}

/// Hide the currently active Mojang cape (DELETE /minecraft/profile/capes/active).
pub async fn hide_minecraft_cape(mc_token: &str) -> Result<(), String> {
    let c = client()?;
    let resp = c
        .delete(format!("{MC_PROFILE_URL}/capes/active"))
        .bearer_auth(mc_token)
        .send()
        .await
        .map_err(|e| format!("cape hide failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("cape hide error ({status}): {body}"));
    }
    Ok(())
}

// ─── Refresh token ───────────────────────────────────────────────

pub async fn refresh_minecraft_token(
    refresh_token: &str,
    backend: MsOauthBackend,
) -> Result<TokenResponse, String> {
    let c = client()?;
    let resp = c
        .post(backend.token_url())
        .form(&[
            ("client_id", backend.client_id()),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("scope", MS_SCOPE),
        ])
        .send()
        .await
        .map_err(|e| format!("token refresh failed: {e}"))?;

    let status = resp.status();
    let body: Value = resp.json().await.map_err(|e| e.to_string())?;

    if !status.is_success() {
        let error = body
            .get("error_description")
            .and_then(|v| v.as_str())
            .or_else(|| body.get("error").and_then(|v| v.as_str()))
            .unwrap_or("unknown");
        return Err(format!("Token refresh failed: {error}"));
    }

    serde_json::from_value(body).map_err(|e| format!("token refresh parse failed: {e}"))
}

fn account_ms_oauth_backend(uuid: &str) -> Option<MsOauthBackend> {
    load_accounts_file()
        .accounts
        .iter()
        .find(|a| a.uuid == uuid)
        .and_then(|a| a.ms_oauth_backend)
}

// ─── Authorization-code (paste URL / WebView) flow ───────────────

/// Live.com authorize URL (public Minecraft launcher client) — WebView + paste fallback.
pub fn microsoft_live_authorize_url() -> String {
    format!(
        "{MS_LIVE_AUTHORIZE_URL}?client_id={}&response_type=code&redirect_uri={}&scope={}&display=touch",
        urlencoding_encode(MS_LIVE_CLIENT_ID),
        urlencoding_encode(MS_REDIRECT_LIVE_DESKTOP),
        urlencoding_encode(MS_SCOPE),
    )
}

/// Prefer live.com (Azure may be blocked).
pub fn microsoft_authorize_url() -> String {
    microsoft_live_authorize_url()
}

#[allow(dead_code)]
fn microsoft_azure_authorize_url() -> String {
    format!(
        "{MS_OAUTH_AUTHORIZE_URL}?client_id={}&response_type=code&redirect_uri={}&scope={}&prompt=select_account",
        urlencoding_encode(MICROSOFT_CLIENT_ID),
        urlencoding_encode(MS_REDIRECT_NATIVE),
        urlencoding_encode(MS_SCOPE),
    )
}

fn urlencoding_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Pull `code` from a redirect URL, raw query string, or bare authorization code.
pub fn extract_oauth_code(input: &str) -> Result<(String, &'static str), String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("Paste the redirect URL (or authorization code)".into());
    }

    // Bare code (no URL)
    if !trimmed.contains("://") && !trimmed.contains('=') && !trimmed.contains('?') {
        let code = trimmed.trim_matches(|c| c == '"' || c == '\'' || c == '<' || c == '>');
        if code.len() < 8 {
            return Err("Authorization code looks too short".into());
        }
        return Ok((code.to_string(), MS_REDIRECT_LIVE_DESKTOP));
    }

    let lower = trimmed.to_ascii_lowercase();
    let redirect = if lower.contains("oauth20_desktop.srf")
        || lower.contains("login.live.com")
        || lower.contains("://live.com")
        || lower.contains("://www.live.com")
    {
        MS_REDIRECT_LIVE_DESKTOP
    } else {
        MS_REDIRECT_NATIVE
    };

    // Prefer query string; fall back to fragment (some browsers).
    let query = if let Some(q) = trimmed.split_once('?').map(|(_, rest)| rest) {
        q.split('#').next().unwrap_or(q)
    } else if let Some(frag) = trimmed.split_once('#').map(|(_, rest)| rest) {
        frag.trim_start_matches('?')
    } else {
        return Err(
            "Could not parse URL. Paste the full redirect address from the browser address bar."
                .into(),
        );
    };

    let mut code: Option<String> = None;
    let mut error: Option<String> = None;
    let mut error_desc: Option<String> = None;
    for part in query.split('&') {
        let Some((k, v)) = part.split_once('=') else {
            continue;
        };
        let key = urlencoding_decode(k);
        let val = urlencoding_decode(v);
        match key.as_str() {
            "code" => code = Some(val),
            "error" => error = Some(val),
            "error_description" => error_desc = Some(val),
            _ => {}
        }
    }

    if let Some(err) = error {
        let desc = error_desc.unwrap_or(err);
        return Err(format!("Microsoft login error: {desc}"));
    }

    let code = code.ok_or_else(|| {
        "No authorization code found in the URL. Copy the full address after Microsoft redirects you."
            .to_string()
    })?;

    Ok((code, redirect))
}

/// True when the WebView navigated to an OAuth redirect carrying `code` or `error`.
fn is_ms_oauth_callback(url: &url::Url) -> bool {
    let has_params = url
        .query_pairs()
        .any(|(k, _)| k == "code" || k == "error");
    if !has_params {
        return false;
    }
    let host = url.host_str().unwrap_or("").to_ascii_lowercase();
    let path = url.path().to_ascii_lowercase();
    host == "login.live.com"
        || host == "live.com"
        || host.ends_with(".live.com")
        || host.contains("microsoftonline.com")
        || path.contains("oauth20_desktop.srf")
        || path.contains("nativeclient")
}

fn urlencoding_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = &s[i + 1..i + 3];
            if let Ok(v) = u8::from_str_radix(hex, 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'+' {
            out.push(b' ');
        } else {
            out.push(bytes[i]);
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub async fn exchange_authorization_code(
    code: &str,
    redirect_uri: &str,
    backend: MsOauthBackend,
) -> Result<TokenResponse, String> {
    let c = client()?;
    let resp = c
        .post(backend.token_url())
        .form(&[
            ("client_id", backend.client_id()),
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("scope", MS_SCOPE),
        ])
        .send()
        .await
        .map_err(|e| format!("code exchange failed: {e}"))?;

    let status = resp.status();
    let body: Value = resp.json().await.map_err(|e| e.to_string())?;

    if !status.is_success() {
        let error = body
            .get("error_description")
            .and_then(|v| v.as_str())
            .or_else(|| body.get("error").and_then(|v| v.as_str()))
            .unwrap_or("unknown");
        return Err(format!("Authorization code exchange failed: {error}"));
    }

    serde_json::from_value(body).map_err(|e| format!("token response parse failed: {e}"))
}

// ─── Full login flow ─────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginResult {
    pub profile: McProfile,
    /// Never returned to the webview — tokens stay in the OS keyring.
    #[serde(skip_serializing)]
    pub mc_access_token: String,
}

pub async fn complete_microsoft_login(ms_token: &str) -> Result<LoginResult, String> {
    let (xbl_token, userhash) = authenticate_with_xbl(ms_token).await?;
    let xsts_token = authenticate_with_xsts(&xbl_token).await?;
    let mc_token = authenticate_with_minecraft(&userhash, &xsts_token).await?;
    let profile = fetch_mc_profile(&mc_token).await?;
    Ok(LoginResult {
        profile,
        mc_access_token: mc_token,
    })
}

/// MS access → Minecraft profile, then persist tokens + account list.
async fn finalize_microsoft_token_login(
    token_resp: TokenResponse,
    backend: MsOauthBackend,
) -> Result<LoginResult, String> {
    let login = complete_microsoft_login(&token_resp.access_token).await?;

    // Soft entitlement gate — warn via Err only when we are sure ownership is missing.
    match check_minecraft_entitlement(&login.mc_access_token).await {
        Ok(true) => {}
        Ok(false) => {
            return Err(
                "This Microsoft account does not own Minecraft Java (or Game Pass entitlement was not found). Buy the game or use Game Pass, then try again."
                    .into(),
            );
        }
        Err(e) => {
            // Network blip on entitlement — continue; launch will fail later if needed.
            eprintln!("entitlement check skipped: {e}");
        }
    }

    if let Some(ref rt) = token_resp.refresh_token {
        save_token(&account_refresh_key(&login.profile.uuid), rt)?;
    } else {
        return Err(
            "Microsoft did not return a refresh token. Try signing in again (do not revoke offline_access)."
                .into(),
        );
    }
    save_token(
        &account_access_key(&login.profile.uuid),
        &login.mc_access_token,
    )?;
    save_token("mc-access-token", &login.mc_access_token)?;

    let entry = AccountEntry {
        uuid: login.profile.uuid.clone(),
        name: login.profile.name.clone(),
        login_type: LoginType::Microsoft,
        skin_source: SkinSource::Mojang,
        added_at: now_secs(),
        authority: None,
        ms_oauth_backend: Some(backend),
    };
    add_account_to_list(&entry)?;

    let mut profile = login.profile.clone();
    let cape_provider = load_auth_state().cape_provider;
    profile.cape_url = resolve_display_cape(
        &profile.name,
        &profile.uuid,
        &cape_provider,
        &profile.capes,
    )
    .await;

    let accounts = load_accounts_file();
    let state = AuthState {
        logged_in: true,
        profile: Some(profile.clone()),
        expires_at: Some(now_secs() + 86400),
        login_type: LoginType::Microsoft,
        skin_source: SkinSource::Mojang,
        cape_provider,
        accounts: accounts.accounts,
        active_account_uuid: accounts.active_account_uuid,
    };
    save_auth_state(&state)?;

    if let Some(ref skin_url) = profile.skin_url {
        let _ = download_and_cache_skin(skin_url, &profile.uuid).await;
    }

    Ok(LoginResult {
        profile,
        mc_access_token: login.mc_access_token,
    })
}

pub async fn login_with_refresh_token(
    refresh_token: &str,
    backend: Option<MsOauthBackend>,
) -> Result<(LoginResult, Option<String>), String> {
    let token_resp = match backend {
        Some(b) => refresh_minecraft_token(refresh_token, b).await?,
        None => match refresh_minecraft_token(refresh_token, MsOauthBackend::Azure).await {
            Ok(t) => t,
            Err(azure_err) => match refresh_minecraft_token(refresh_token, MsOauthBackend::Live)
                .await
            {
                Ok(t) => t,
                Err(_) => return Err(azure_err),
            },
        },
    };
    let login = complete_microsoft_login(&token_resp.access_token).await?;
    Ok((login, token_resp.refresh_token))
}

// ─── Skin caching ────────────────────────────────────────────────

fn skin_cache_dir() -> PathBuf {
    dirs::cache_dir()
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("TuffBox")
        .join("skins")
}

pub fn cached_skin_path(uuid: &str) -> PathBuf {
    skin_cache_dir().join(format!("{uuid}.png"))
}

const SKIN_CACHE_TTL: Duration = Duration::from_secs(86400);

fn cache_file_fresh(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    let Ok(modified) = meta.modified() else {
        return false;
    };
    let Ok(elapsed) = modified.elapsed() else {
        return false;
    };
    elapsed < SKIN_CACHE_TTL
}

fn cached_skin_path_for_url(url: &str) -> PathBuf {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(url.as_bytes());
    skin_cache_dir().join(format!("url-{}.png", hex::encode(&hash[..16])))
}

fn encode_png_data_url(bytes: &[u8]) -> String {
    use base64::Engine;
    let engine = base64::engine::general_purpose::STANDARD;
    format!("data:image/png;base64,{}", engine.encode(bytes))
}

pub async fn download_and_cache_skin(skin_url: &str, uuid: &str) -> Result<PathBuf, String> {
    let path = cached_skin_path(uuid);
    if path.exists() && cache_file_fresh(&path) {
        return Ok(path);
    }

    let c = client()?;
    let bytes = c
        .get(skin_url)
        .send()
        .await
        .map_err(|e| format!("skin download failed: {e}"))?
        .bytes()
        .await
        .map_err(|e| format!("skin download body failed: {e}"))?;

    let dir = skin_cache_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    fs::write(&path, &bytes).map_err(|e| e.to_string())?;
    let url_path = cached_skin_path_for_url(skin_url);
    let _ = fs::write(&url_path, &bytes);
    Ok(path)
}

pub fn load_mc_access_token() -> Result<String, String> {
    // Try per-account key first, then legacy
    if let Ok(accounts) = fs::read_to_string(accounts_path()) {
        if let Ok(data) = serde_json::from_str::<AccountsFile>(&accounts) {
            if let Some(ref uuid) = data.active_account_uuid {
                if let Ok(token) = load_token(&account_access_key(uuid)) {
                    return Ok(token);
                }
            }
        }
    }
    load_token("mc-access-token")
}

// ─── Skin as base64 for 3D viewer ───────────────────────────────

pub async fn fetch_skin_as_base64(url: &str) -> Result<String, String> {
    let path = cached_skin_path_for_url(url);
    if path.exists() && cache_file_fresh(&path) {
        if let Ok(bytes) = fs::read(&path) {
            if !bytes.is_empty() {
                return Ok(encode_png_data_url(&bytes));
            }
        }
    }

    let c = client()?;
    let bytes = c
        .get(url)
        .send()
        .await
        .map_err(|e| format!("skin fetch failed: {e}"))?
        .bytes()
        .await
        .map_err(|e| format!("skin fetch body failed: {e}"))?;

    let dir = skin_cache_dir();
    let _ = fs::create_dir_all(&dir);
    let _ = fs::write(&path, &bytes);
    Ok(encode_png_data_url(&bytes))
}

// ─── Multi-account helpers ───────────────────────────────────────

fn add_account_to_list(entry: &AccountEntry) -> Result<(), String> {
    let mut data = load_accounts_file();
    data.accounts.retain(|a| a.uuid != entry.uuid);
    data.accounts.push(entry.clone());
    // New login always becomes the active account.
    data.active_account_uuid = Some(entry.uuid.clone());
    save_accounts_file(&data)
}

fn remove_account_from_list(uuid: &str) -> Result<(), String> {
    let mut data = load_accounts_file();
    data.accounts.retain(|a| a.uuid != uuid);
    if data.active_account_uuid.as_deref() == Some(uuid) {
        data.active_account_uuid = data.accounts.first().map(|a| a.uuid.clone());
    }
    save_accounts_file(&data)
}

fn set_active_account(uuid: &str) -> Result<(), String> {
    let mut data = load_accounts_file();
    if data.accounts.iter().any(|a| a.uuid == uuid) {
        data.active_account_uuid = Some(uuid.to_string());
        save_accounts_file(&data)?;
    }
    Ok(())
}

fn sync_auth_state_from_accounts() -> Result<(), String> {
    let accounts = load_accounts_file();
    let mut state = load_auth_state();
    state.accounts = accounts.accounts.clone();
    state.active_account_uuid = accounts.active_account_uuid.clone();

    match accounts.active_account_uuid.as_ref() {
        Some(uuid) => {
            if let Some(entry) = accounts.accounts.iter().find(|a| &a.uuid == uuid) {
                state.login_type = entry.login_type.clone();
                state.skin_source = entry.skin_source.clone();
                // Keep profile if it matches; otherwise leave for switch/refresh to fill.
                if state.profile.as_ref().map(|p| &p.uuid) != Some(uuid) {
                    state.profile = None;
                    state.logged_in = false;
                }
            }
        }
        None => {
            state.logged_in = false;
            state.profile = None;
            state.expires_at = None;
        }
    }
    save_auth_state(&state)
}

// ─── Tauri commands ──────────────────────────────────────────────

#[tauri::command(rename_all = "camelCase")]
pub async fn mc_start_device_code() -> Result<DeviceCodeInfo, String> {
    let (info, device_code, interval) = start_device_code_flow().await?;
    save_token("mc-device-code", &device_code)?;
    save_token("mc-device-interval", &interval.to_string())?;
    Ok(info)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn mc_poll_device_code() -> Result<LoginResult, String> {
    let device_code = load_token("mc-device-code")?;

    // Single attempt — frontend polls on an interval. Avoids stacking
    // long-running blocking polls that race on the same device code.
    let token_resp = poll_device_code_token_once(&device_code).await?;

    // Only clear after Xbox/MC chain succeeds — otherwise the user can retry poll.
    let result = finalize_microsoft_token_login(token_resp, MsOauthBackend::Azure).await?;
    let _ = clear_token("mc-device-code");
    let _ = clear_token("mc-device-interval");
    Ok(result)
}

#[tauri::command(rename_all = "camelCase")]
pub fn mc_get_microsoft_login_url() -> Result<String, String> {
    Ok(microsoft_authorize_url())
}

/// Complete Microsoft login by pasting the browser redirect URL (or raw auth code).
#[tauri::command(rename_all = "camelCase")]
pub async fn mc_login_with_auth_url(url_or_code: String) -> Result<LoginResult, String> {
    let (code, redirect_uri) = extract_oauth_code(&url_or_code)?;
    let backend = if redirect_uri == MS_REDIRECT_LIVE_DESKTOP {
        MsOauthBackend::Live
    } else {
        MsOauthBackend::Azure
    };
    let token_resp = exchange_authorization_code(&code, redirect_uri, backend).await?;
    finalize_microsoft_token_login(token_resp, backend).await
}

/// Open an in-app WebView for Microsoft login; intercept live.com redirect and finish auth.
#[tauri::command(rename_all = "camelCase")]
pub async fn mc_start_microsoft_webview_auth(app: AppHandle) -> Result<LoginResult, String> {
    if let Some(existing) = app.get_webview_window(MS_AUTH_WINDOW_LABEL) {
        let _ = existing.close();
    }

    let (tx, rx) = tokio::sync::oneshot::channel::<Result<(String, String), String>>();
    let tx = Arc::new(Mutex::new(Some(tx)));

    let auth_url = microsoft_live_authorize_url();
    let parsed: url::Url = auth_url
        .parse()
        .map_err(|e| format!("invalid auth URL: {e}"))?;

    let tx_nav = Arc::clone(&tx);
    let app_nav = app.clone();
    let tx_close = Arc::clone(&tx);

    let auth_window = WebviewWindowBuilder::new(
        &app,
        MS_AUTH_WINDOW_LABEL,
        WebviewUrl::External(parsed),
    )
    .title("Sign in with Microsoft")
    .inner_size(500.0, 650.0)
    .resizable(false)
    .center()
    .on_navigation(move |url| {
        if !is_ms_oauth_callback(&url) {
            return true;
        }
        match extract_oauth_code(url.as_str()) {
            Ok((code, redirect)) => {
                if let Ok(mut guard) = tx_nav.lock() {
                    if let Some(sender) = guard.take() {
                        let _ = sender.send(Ok((code, redirect.to_string())));
                    }
                }
            }
            Err(e) => {
                if let Ok(mut guard) = tx_nav.lock() {
                    if let Some(sender) = guard.take() {
                        let _ = sender.send(Err(e));
                    }
                }
            }
        }
        if let Some(w) = app_nav.get_webview_window(MS_AUTH_WINDOW_LABEL) {
            let _ = w.close();
        }
        false
    })
    .build()
    .map_err(|e| format!("Failed to open Microsoft login window: {e}"))?;

    auth_window.on_window_event(move |event| {
        if matches!(event, WindowEvent::Destroyed) {
            if let Ok(mut guard) = tx_close.lock() {
                if let Some(sender) = guard.take() {
                    let _ = sender.send(Err("Microsoft login cancelled".into()));
                }
            }
        }
    });

    let (code, redirect_uri) = rx
        .await
        .map_err(|_| "Microsoft login cancelled".to_string())??;

    let token_resp =
        exchange_authorization_code(&code, &redirect_uri, MsOauthBackend::Live).await?;
    finalize_microsoft_token_login(token_resp, MsOauthBackend::Live).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn mc_offline_login(
    username: String,
    skin_source: SkinSource,
) -> Result<LoginResult, String> {
    let trimmed = username.trim().to_string();
    if trimmed.is_empty() {
        return Err("Username cannot be empty".to_string());
    }
    if trimmed.len() > 16 {
        return Err("Username must be 16 characters or less".to_string());
    }

    let uuid = offline_uuid(&trimmed);

    let skin_url = fetch_skin_for_username(&trimmed, &skin_source).await;
    let prev = load_auth_state();
    let cape_provider = match skin_source {
        SkinSource::TLauncher => CapeProvider::TLauncher,
        _ => prev.cape_provider.clone(),
    };
    let cape_url = resolve_display_cape(&trimmed, &uuid, &cape_provider, &[]).await;

    let profile = McProfile {
        uuid: uuid.clone(),
        name: trimmed,
        skin_url: skin_url.clone(),
        cape_url,
        capes: vec![],
    };

    if let Some(ref url) = skin_url {
        let _ = download_and_cache_skin(url, &uuid).await;
    }

    let entry = AccountEntry {
        uuid: uuid.clone(),
        name: profile.name.clone(),
        login_type: LoginType::Offline,
        skin_source: skin_source.clone(),
        added_at: now_secs(),
        authority: None,
        ms_oauth_backend: None,
    };
    add_account_to_list(&entry)?;

    let accounts = load_accounts_file();
    let state = AuthState {
        logged_in: true,
        profile: Some(profile.clone()),
        expires_at: None,
        login_type: LoginType::Offline,
        skin_source: skin_source.clone(),
        cape_provider,
        accounts: accounts.accounts,
        active_account_uuid: accounts.active_account_uuid,
    };
    save_auth_state(&state)?;

    Ok(LoginResult {
        profile,
        mc_access_token: "0".to_string(),
    })
}

#[tauri::command(rename_all = "camelCase")]
pub async fn mc_get_auth_status() -> Result<AuthState, String> {
    let mut state = load_auth_state();
    let accounts = load_accounts_file();
    state.accounts = accounts.accounts;
    state.active_account_uuid = accounts.active_account_uuid;

    // Only refresh Microsoft tokens; offline login persists until explicit logout.
    // Skip the network call if we refreshed recently (frontend polls on every focus).
    let should_refresh = state.logged_in
        && state.login_type == LoginType::Microsoft
        && LAST_AUTH_REFRESH
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .map(|t| t.elapsed() >= AUTH_REFRESH_TTL)
            .unwrap_or(true);

    if should_refresh {
        if let Some(ref uuid) = state.active_account_uuid {
            if let Ok(refresh_token) = load_token(&account_refresh_key(uuid)) {
                let backend = account_ms_oauth_backend(uuid);
                match login_with_refresh_token(&refresh_token, backend).await {
                    Ok((login, new_refresh)) => {
                        let mut profile = login.profile.clone();
                        // Keep the selected display cape provider (OptiFine / TLauncher / …)
                        // over the raw Mojang ACTIVE cape from the profile endpoint.
                        if state.cape_provider != CapeProvider::Mojang {
                            profile.cape_url = resolve_display_cape(
                                &profile.name,
                                &profile.uuid,
                                &state.cape_provider,
                                &profile.capes,
                            )
                            .await;
                        }
                        state.profile = Some(profile);
                        // The silent renewal keeps the session alive — slide
                        // the reported expiry forward so the UI (and the
                        // launch path) don't treat a fresh token as stale.
                        state.expires_at = Some(now_secs() + 86400);
                        save_token(&account_access_key(uuid), &login.mc_access_token)?;
                        save_token("mc-access-token", &login.mc_access_token)?;
                        if let Some(rt) = new_refresh {
                            let _ = save_token(&account_refresh_key(uuid), &rt);
                        }
                        if let Some(ref skin_url) = login.profile.skin_url {
                            let _ =
                                download_and_cache_skin(skin_url, &login.profile.uuid).await;
                        }
                        save_auth_state(&state)?;
                        if let Ok(mut last) = LAST_AUTH_REFRESH.lock() {
                            *last = Some(Instant::now());
                        }
                    }
                    Err(e) => {
                        let el = e.to_lowercase();
                        let fatal = el.contains("invalid_grant")
                            || el.contains("revoked")
                            || (el.contains("expired") && el.contains("refresh"));
                        if fatal {
                            state.logged_in = false;
                            state.profile = None;
                            save_auth_state(&state)?;
                            let _ = clear_token(&account_refresh_key(uuid));
                            let _ = clear_token(&account_access_key(uuid));
                        } else {
                            // Soft-fail network / transient errors — keep last known profile.
                            eprintln!("token refresh soft-fail: {e}");
                            if let Ok(mut last) = LAST_AUTH_REFRESH.lock() {
                                *last = Some(Instant::now());
                            }
                        }
                    }
                }
            }
        }
    }

    // For offline login, refresh skin + display cape from selected sources
    if state.logged_in && state.login_type == LoginType::Offline {
        // Existing sessions may still have capeProvider=mojang from before
        // TLauncher skin logins auto-selected the matching cloak.
        if state.skin_source == SkinSource::TLauncher
            && state.cape_provider == CapeProvider::Mojang
        {
            state.cape_provider = CapeProvider::TLauncher;
        }
        if let Some(ref profile) = state.profile {
            let skin_url = fetch_skin_for_username(&profile.name, &state.skin_source).await;
            if let Some(ref url) = skin_url {
                let _ = download_and_cache_skin(url, &profile.uuid).await;
            }
            let cape_url = resolve_display_cape(
                &profile.name,
                &profile.uuid,
                &state.cape_provider,
                &profile.capes,
            )
            .await;
            let updated_profile = McProfile {
                skin_url: skin_url.or_else(|| profile.skin_url.clone()),
                cape_url,
                ..profile.clone()
            };
            state.profile = Some(updated_profile);
            save_auth_state(&state)?;
        }
    }

    Ok(state)
}

#[tauri::command(rename_all = "camelCase")]
/// Renew the active Microsoft session: exchange the stored refresh token for
/// fresh MSA + Minecraft tokens, persist both to the keyring, slide the ~24h
/// expiry forward, and return the refreshed auth state. Errors when the
/// session cannot be renewed (revoked refresh token, network) — the caller
/// decides whether to ask for a fresh sign-in.
pub async fn refresh_active_microsoft_session() -> Result<AuthState, String> {
    let state = load_auth_state();
    if state.login_type != LoginType::Microsoft {
        return Err("active session is not a Microsoft account".into());
    }
    let uuid = state
        .active_account_uuid
        .clone()
        .ok_or("no active Microsoft account")?;
    let refresh_token = load_token(&account_refresh_key(&uuid))?;
    let backend = account_ms_oauth_backend(&uuid);
    let (login, new_refresh) = login_with_refresh_token(&refresh_token, backend).await?;
    save_token(&account_access_key(&uuid), &login.mc_access_token)?;
    save_token("mc-access-token", &login.mc_access_token)?;
    if let Some(rt) = new_refresh {
        let _ = save_token(&account_refresh_key(&uuid), &rt);
    }

    let accounts = load_accounts_file();
    let new_state = AuthState {
        logged_in: true,
        profile: Some(login.profile.clone()),
        expires_at: Some(now_secs() + 86400),
        login_type: LoginType::Microsoft,
        skin_source: SkinSource::Mojang,
        cape_provider: state.cape_provider.clone(),
        accounts: accounts.accounts,
        active_account_uuid: accounts.active_account_uuid,
    };
    save_auth_state(&new_state)?;

    if let Some(ref skin_url) = login.profile.skin_url {
        let _ = download_and_cache_skin(skin_url, &login.profile.uuid).await;
    }
    Ok(new_state)
}

/// Force an MSA token renewal, bypassing the 30s status-refresh throttle:
/// used right before a launch so the game's auth handshake never sees a
/// stale token. Non-Microsoft sessions error — offline and Yggdrasil
/// accounts have no refresh token to renew (their expiry is null and the
/// frontend never calls this for them).
#[tauri::command(rename_all = "camelCase")]
pub async fn mc_refresh_token() -> Result<AuthState, String> {
    refresh_active_microsoft_session().await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn mc_refresh_profile() -> Result<McProfile, String> {
    let state = load_auth_state();

    if state.login_type == LoginType::Microsoft {
        let new_state = refresh_active_microsoft_session().await?;
        let mut profile = new_state.profile.ok_or("Not logged in")?;

        // Apply selected display cape provider over Mojang active cape when needed.
        if state.cape_provider != CapeProvider::Mojang {
            profile.cape_url = resolve_display_cape(
                &profile.name,
                &profile.uuid,
                &state.cape_provider,
                &profile.capes,
            )
            .await;
            let mut s = load_auth_state();
            s.profile = Some(profile.clone());
            let _ = save_auth_state(&s);
        }

        return Ok(profile);
    }

    // Offline: refresh skin from source
    let profile = state.profile.ok_or("Not logged in")?;
    let skin_url = fetch_skin_for_username(&profile.name, &state.skin_source).await;
    if let Some(ref url) = skin_url {
        let _ = fs::remove_file(cached_skin_path(&profile.uuid));
        let _ = download_and_cache_skin(url, &profile.uuid).await;
    }
    let cape_url = resolve_display_cape(
        &profile.name,
        &profile.uuid,
        &state.cape_provider,
        &profile.capes,
    )
    .await;
    let updated = McProfile {
        skin_url: skin_url.or_else(|| profile.skin_url.clone()),
        cape_url,
        ..profile
    };
    let accounts = load_accounts_file();
    let new_state = AuthState {
        profile: Some(updated.clone()),
        accounts: accounts.accounts,
        active_account_uuid: accounts.active_account_uuid,
        ..state
    };
    save_auth_state(&new_state)?;
    Ok(updated)
}

#[tauri::command(rename_all = "camelCase")]
pub fn mc_get_skin_path(uuid: String) -> Result<String, String> {
    let path = cached_skin_path(&uuid);
    if path.exists() {
        Ok(path.to_string_lossy().to_string())
    } else {
        Err("skin not cached".to_string())
    }
}

#[tauri::command(rename_all = "camelCase")]
pub async fn mc_fetch_skin_url(uuid: String) -> Result<Option<String>, String> {
    if let Some(url) = fetch_skin_mojang(&uuid).await {
        let _ = download_and_cache_skin(&url, &uuid).await;
        return Ok(Some(url));
    }
    Ok(None)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn mc_fetch_skin_for_username(
    username: String,
    source: SkinSource,
) -> Result<Option<String>, String> {
    Ok(fetch_skin_for_username(&username, &source).await)
}

#[tauri::command(rename_all = "camelCase")]
pub fn mc_set_skin_source(source: SkinSource) -> Result<(), String> {
    let mut state = load_auth_state();
    state.skin_source = source;
    save_auth_state(&state)
}

// ─── Yggdrasil / authlib-injector ────────────────────────────────

pub fn preset_authority(preset: &str) -> &'static str {
    match preset {
        "littleskin" | "little-skin" => "https://littleskin.cn/api/yggdrasil",
        "custom" => "",
        _ => "https://authserver.ely.by/api/authlib-injector", // elyby default
    }
}

fn normalize_authority(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

struct YggAuthResult {
    access_token: String,
    client_token: Option<String>,
    uuid: String,
    name: String,
}

async fn yggdrasil_authenticate(
    authority: &str,
    username: &str,
    password: &str,
) -> Result<YggAuthResult, String> {
    let authority = normalize_authority(authority);
    if authority.is_empty() {
        return Err("Yggdrasil authority URL required".into());
    }
    let c = client()?;
    let url = format!("{authority}/authserver/authenticate");
    let resp = c
        .post(&url)
        .json(&serde_json::json!({
            "agent": { "name": "Minecraft", "version": 1 },
            "username": username,
            "password": password,
            "requestUser": true
        }))
        .send()
        .await
        .map_err(|e| format!("Yggdrasil auth failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Yggdrasil authenticate error ({status}): {body}"));
    }

    let body: Value = resp.json().await.map_err(|e| e.to_string())?;
    let access_token = body
        .get("accessToken")
        .and_then(|v| v.as_str())
        .ok_or("missing accessToken")?
        .to_string();
    let client_token = body
        .get("clientToken")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let profile = body
        .get("selectedProfile")
        .ok_or("missing selectedProfile — does this account own a Minecraft profile?")?;
    let uuid = profile
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or("missing profile id")?
        .to_string();
    let name = profile
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(username)
        .to_string();

    Ok(YggAuthResult {
        access_token,
        client_token,
        uuid,
        name,
    })
}

async fn yggdrasil_refresh(
    authority: &str,
    access_token: &str,
    client_token: &str,
) -> Result<YggAuthResult, String> {
    let authority = normalize_authority(authority);
    let c = client()?;
    let url = format!("{authority}/authserver/refresh");
    let resp = c
        .post(&url)
        .json(&serde_json::json!({
            "accessToken": access_token,
            "clientToken": client_token,
            "requestUser": true
        }))
        .send()
        .await
        .map_err(|e| format!("Yggdrasil refresh failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Yggdrasil refresh error ({status}): {body}"));
    }

    let body: Value = resp.json().await.map_err(|e| e.to_string())?;
    let access_token = body
        .get("accessToken")
        .and_then(|v| v.as_str())
        .ok_or("missing accessToken")?
        .to_string();
    let client_token = body
        .get("clientToken")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let profile = body.get("selectedProfile");
    let (uuid, name) = if let Some(p) = profile {
        (
            p.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            p.get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("Player")
                .to_string(),
        )
    } else {
        (String::new(), "Player".into())
    };
    if uuid.is_empty() {
        return Err("refresh response missing selectedProfile".into());
    }
    Ok(YggAuthResult {
        access_token,
        client_token,
        uuid,
        name,
    })
}

fn skin_source_for_authority(authority: &str) -> SkinSource {
    let a = authority.to_lowercase();
    if a.contains("ely.by") {
        SkinSource::Elyby
    } else {
        SkinSource::Mojang
    }
}

#[tauri::command(rename_all = "camelCase")]
pub fn mc_list_yggdrasil_presets() -> Result<Vec<serde_json::Value>, String> {
    Ok(vec![
        serde_json::json!({
            "id": "elyby",
            "label": "Ely.by",
            "authority": preset_authority("elyby"),
        }),
        serde_json::json!({
            "id": "littleskin",
            "label": "LittleSkin",
            "authority": preset_authority("littleskin"),
        }),
        serde_json::json!({
            "id": "custom",
            "label": "Custom authlib-injector / Yggdrasil",
            "authority": "",
        }),
    ])
}

#[tauri::command(rename_all = "camelCase")]
pub async fn mc_yggdrasil_login(
    username: String,
    password: String,
    authority: String,
) -> Result<LoginResult, String> {
    let authority = normalize_authority(&authority);
    if authority.is_empty() {
        return Err("Enter an authlib-injector / Yggdrasil authority URL".into());
    }
    let auth = yggdrasil_authenticate(&authority, username.trim(), &password).await?;
    let skin_source = skin_source_for_authority(&authority);
    let skin_url = fetch_skin_for_username(&auth.name, &skin_source).await;
    if let Some(ref url) = skin_url {
        let _ = download_and_cache_skin(url, &auth.uuid).await;
    }
    let prev = load_auth_state();
    let cape_url =
        resolve_display_cape(&auth.name, &auth.uuid, &prev.cape_provider, &[]).await;

    let profile = McProfile {
        uuid: auth.uuid.clone(),
        name: auth.name.clone(),
        skin_url,
        cape_url,
        capes: vec![],
    };

    save_token(&account_access_key(&auth.uuid), &auth.access_token)?;
    save_token("mc-access-token", &auth.access_token)?;
    if let Some(ref ct) = auth.client_token {
        save_token(&account_refresh_key(&auth.uuid), ct)?;
    }

    let entry = AccountEntry {
        uuid: auth.uuid.clone(),
        name: auth.name.clone(),
        login_type: LoginType::Yggdrasil,
        skin_source: skin_source.clone(),
        added_at: now_secs(),
        authority: Some(authority),
        ms_oauth_backend: None,
    };
    add_account_to_list(&entry)?;
    set_active_account(&auth.uuid)?;

    let accounts = load_accounts_file();
    let state = AuthState {
        logged_in: true,
        profile: Some(profile.clone()),
        expires_at: Some(now_secs() + 86400),
        login_type: LoginType::Yggdrasil,
        skin_source,
        cape_provider: prev.cape_provider,
        accounts: accounts.accounts,
        active_account_uuid: accounts.active_account_uuid,
    };
    save_auth_state(&state)?;

    Ok(LoginResult {
        profile,
        mc_access_token: auth.access_token,
    })
}

/// Returns active account launch identity for the JVM (uuid, name, token, userType, authority).
pub fn load_active_launch_identity() -> Option<(String, String, String, String, Option<String>)> {
    let state = load_auth_state();
    let profile = state.profile?;
    let token = load_mc_access_token().unwrap_or_else(|_| "0".into());
    let user_type = match state.login_type {
        LoginType::Microsoft => "msa",
        LoginType::Yggdrasil => "mojang",
        LoginType::Offline => "legacy",
    };
    let authority = state
        .accounts
        .iter()
        .find(|a| a.uuid == profile.uuid)
        .and_then(|a| a.authority.clone());
    Some((
        profile.uuid,
        profile.name,
        token,
        user_type.to_string(),
        authority,
    ))
}

// ─── Multi-account commands ──────────────────────────────────────

#[tauri::command(rename_all = "camelCase")]
pub fn mc_list_accounts() -> Result<Vec<AccountEntry>, String> {
    let data = load_accounts_file();
    Ok(data.accounts)
}

/// Switch active account and fully reload skin + display cape for that account.
#[tauri::command(rename_all = "camelCase")]
pub async fn mc_switch_account(uuid: String) -> Result<AuthState, String> {
    set_active_account(&uuid)?;
    let accounts = load_accounts_file();
    let entry = accounts
        .accounts
        .iter()
        .find(|a| a.uuid == uuid)
        .cloned()
        .ok_or_else(|| "Account not found".to_string())?;

    let prev = load_auth_state();
    let cape_provider = prev.cape_provider.clone();

    // Bust skin cache so the preview reloads for the new account.
    let _ = fs::remove_file(cached_skin_path(&uuid));

    let mut state = match entry.login_type {
        LoginType::Microsoft => {
            let mut profile = None;
            let mut expires = None;
            if let Ok(refresh_token) = load_token(&account_refresh_key(&uuid)) {
                let backend = entry.ms_oauth_backend;
                if let Ok((login, new_refresh)) =
                    login_with_refresh_token(&refresh_token, backend).await
                {
                    let _ = save_token(&account_access_key(&uuid), &login.mc_access_token);
                    let _ = save_token("mc-access-token", &login.mc_access_token);
                    if let Some(rt) = new_refresh {
                        let _ = save_token(&account_refresh_key(&uuid), &rt);
                    }
                    if let Some(ref skin_url) = login.profile.skin_url {
                        let _ = download_and_cache_skin(skin_url, &login.profile.uuid).await;
                    }
                    let mut p = login.profile;
                    p.cape_url = resolve_display_cape(
                        &p.name,
                        &p.uuid,
                        &cape_provider,
                        &p.capes,
                    )
                    .await;
                    profile = Some(p);
                    expires = Some(now_secs() + 86400);
                }
            }
            // Fallback: keep a stub profile from the account list if token refresh failed.
            if profile.is_none() {
                let skin_url = fetch_skin_for_username(&entry.name, &SkinSource::Mojang).await;
                if let Some(ref url) = skin_url {
                    let _ = download_and_cache_skin(url, &uuid).await;
                }
                let cape_url =
                    resolve_display_cape(&entry.name, &uuid, &cape_provider, &[]).await;
                profile = Some(McProfile {
                    uuid: uuid.clone(),
                    name: entry.name.clone(),
                    skin_url,
                    cape_url,
                    capes: vec![],
                });
            }
            AuthState {
                logged_in: true,
                profile,
                expires_at: expires,
                login_type: LoginType::Microsoft,
                skin_source: SkinSource::Mojang,
                cape_provider,
                accounts: accounts.accounts,
                active_account_uuid: Some(uuid),
            }
        }
        LoginType::Offline => {
            let skin_url = fetch_skin_for_username(&entry.name, &entry.skin_source).await;
            if let Some(ref url) = skin_url {
                let _ = download_and_cache_skin(url, &uuid).await;
            }
            let cape_url = resolve_display_cape(&entry.name, &uuid, &cape_provider, &[]).await;
            AuthState {
                logged_in: true,
                profile: Some(McProfile {
                    uuid: uuid.clone(),
                    name: entry.name.clone(),
                    skin_url,
                    cape_url,
                    capes: vec![],
                }),
                expires_at: None,
                login_type: LoginType::Offline,
                skin_source: entry.skin_source,
                cape_provider,
                accounts: accounts.accounts.clone(),
                active_account_uuid: Some(uuid.clone()),
            }
        }
        LoginType::Yggdrasil => {
            let authority = entry
                .authority
                .clone()
                .unwrap_or_else(|| preset_authority("elyby").to_string());
            let mut profile = None;
            if let Ok(client_token) = load_token(&account_refresh_key(&uuid)) {
                if let Ok(access) = load_token(&account_access_key(&uuid)) {
                    if let Ok(refreshed) =
                        yggdrasil_refresh(&authority, &access, &client_token).await
                    {
                        let _ = save_token(&account_access_key(&uuid), &refreshed.access_token);
                        let _ = save_token("mc-access-token", &refreshed.access_token);
                        if let Some(ref ct) = refreshed.client_token {
                            let _ = save_token(&account_refresh_key(&uuid), ct);
                        }
                        let skin_url =
                            fetch_skin_for_username(&refreshed.name, &entry.skin_source).await;
                        if let Some(ref url) = skin_url {
                            let _ = download_and_cache_skin(url, &refreshed.uuid).await;
                        }
                        let cape_url = resolve_display_cape(
                            &refreshed.name,
                            &refreshed.uuid,
                            &cape_provider,
                            &[],
                        )
                        .await;
                        profile = Some(McProfile {
                            uuid: refreshed.uuid.clone(),
                            name: refreshed.name,
                            skin_url,
                            cape_url,
                            capes: vec![],
                        });
                    }
                }
            }
            if profile.is_none() {
                let skin_url = fetch_skin_for_username(&entry.name, &entry.skin_source).await;
                if let Some(ref url) = skin_url {
                    let _ = download_and_cache_skin(url, &uuid).await;
                }
                let cape_url =
                    resolve_display_cape(&entry.name, &uuid, &cape_provider, &[]).await;
                profile = Some(McProfile {
                    uuid: uuid.clone(),
                    name: entry.name.clone(),
                    skin_url,
                    cape_url,
                    capes: vec![],
                });
            }
            AuthState {
                logged_in: true,
                profile,
                expires_at: Some(now_secs() + 86400),
                login_type: LoginType::Yggdrasil,
                skin_source: entry.skin_source,
                cape_provider,
                accounts: accounts.accounts,
                active_account_uuid: Some(uuid),
            }
        }
    };

    save_auth_state(&state)?;
    // Ensure accounts list is current.
    let fresh = load_accounts_file();
    state.accounts = fresh.accounts;
    state.active_account_uuid = fresh.active_account_uuid;
    save_auth_state(&state)?;
    Ok(state)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn mc_remove_account(uuid: String) -> Result<AuthState, String> {
    let state = load_auth_state();
    let was_active = state.active_account_uuid.as_deref() == Some(uuid.as_str());
    invalidate_cape_cache(&uuid);
    let _ = clear_token(&account_refresh_key(&uuid));
    let _ = clear_token(&account_access_key(&uuid));
    remove_account_from_list(&uuid)?;
    sync_auth_state_from_accounts()?;

    if was_active {
        let accounts = load_accounts_file();
        if let Some(next) = accounts.active_account_uuid {
            return mc_switch_account(next).await;
        }
        let mut cleared = load_auth_state();
        cleared.logged_in = false;
        cleared.profile = None;
        cleared.active_account_uuid = None;
        cleared.expires_at = None;
        let _ = clear_token("mc-access-token");
        save_auth_state(&cleared)?;
        return Ok(cleared);
    }
    Ok(load_auth_state())
}

// ─── Cape catalog cache ─────────────────────────────────────────

/// Capes are re-discovered from Mojang / OptiFine / TLauncher — a handful of
/// network round-trips. Cache the catalog per UUID so re-opening the Me tab is
/// instant; the next cold load refreshes it.
const CAPE_CACHE_TTL_SECS: u64 = 600; // 10 min

fn cape_cache_path(uuid: &str) -> PathBuf {
    dirs::cache_dir()
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("TuffBox")
        .join("capes")
        .join(format!("{uuid}.json"))
}

#[derive(Debug, Deserialize)]
struct CapeCacheEnvelope {
    #[serde(default)]
    saved_at: u64,
    catalog: CapeCatalog,
}

fn load_cape_cache(uuid: &str) -> Option<CapeCatalog> {
    let raw = fs::read_to_string(cape_cache_path(uuid)).ok()?;
    let env: CapeCacheEnvelope = serde_json::from_str(&raw).ok()?;
    if now_secs().saturating_sub(env.saved_at) >= CAPE_CACHE_TTL_SECS {
        return None;
    }
    Some(env.catalog)
}

/// Stale-while-revalidate helper: any cached catalog regardless of age.
fn load_cape_cache_any_age(uuid: &str) -> Option<CapeCatalog> {
    let raw = fs::read_to_string(cape_cache_path(uuid)).ok()?;
    serde_json::from_str(&raw).ok().map(|env: CapeCacheEnvelope| env.catalog)
}

fn save_cape_cache(uuid: &str, catalog: &CapeCatalog) {
    let path = cape_cache_path(uuid);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let env = serde_json::json!({ "savedAt": now_secs(), "catalog": catalog });
    let Ok(bytes) = serde_json::to_vec(&env) else {
        return;
    };
    let _ = fs::write(&path, bytes);
}

fn invalidate_cape_cache(uuid: &str) {
    let _ = fs::remove_file(cape_cache_path(uuid));
}

/// Discover capes from Mojang / OptiFine / TLauncher for the active profile.
#[tauri::command(rename_all = "camelCase")]
pub async fn mc_list_capes() -> Result<CapeCatalog, String> {
    let state = load_auth_state();
    let profile = state.profile.ok_or("Not logged in")?;
    // Task: capes "load forever". Two mitigations:
    // 1. Fresh cache → instant return (unchanged).
    // 2. Stale cache → return it immediately and refresh in the background;
    //    the UI gets data at once instead of waiting on external probes.
    if let Some(cached) = load_cape_cache_any_age(&profile.uuid) {
        let fresh = load_cape_cache(&profile.uuid).is_some();
        if !fresh {
            let username = profile.name.clone();
            let uuid = profile.uuid.clone();
            let provider = state.cape_provider.clone();
            let owned = profile.capes.clone();
            tokio::spawn(async move {
                let catalog =
                    build_cape_catalog(&username, &uuid, provider, &owned).await;
                save_cape_cache(&uuid, &catalog);
            });
        }
        return Ok(cached);
    }
    let catalog = build_cape_catalog(
        &profile.name,
        &profile.uuid,
        state.cape_provider,
        &profile.capes,
    )
    .await;
    save_cape_cache(&profile.uuid, &catalog);
    Ok(catalog)
}

/// Select which cape provider is shown on the skin preview (only one).
#[tauri::command(rename_all = "camelCase")]
pub async fn mc_set_cape_provider(provider: CapeProvider) -> Result<AuthState, String> {
    let mut state = load_auth_state();

    // "None" deactivates the active Mojang cape so it is truly hidden in-game.
    if provider == CapeProvider::None {
        if let Ok(token) = load_mc_access_token() {
            let _ = hide_minecraft_cape(&token).await;
        }
    }

    state.cape_provider = provider.clone();
    if let Some(ref mut profile) = state.profile {
        invalidate_cape_cache(&profile.uuid);
        profile.cape_url = resolve_display_cape(
            &profile.name,
            &profile.uuid,
            &provider,
            &profile.capes,
        )
        .await;
    }
    save_auth_state(&state)?;
    Ok(state)
}

// ─── Skin upload commands ────────────────────────────────────────

async fn refresh_profile_after_skin_change(access_token: &str) -> Result<AuthState, String> {
    let mut state = load_auth_state();
    if let Ok(mut profile) = fetch_mc_profile(access_token).await {
        if let Some(ref url) = profile.skin_url {
            let _ = download_and_cache_skin(url, &profile.uuid).await;
        }
        profile.cape_url = resolve_display_cape(
            &profile.name,
            &profile.uuid,
            &state.cape_provider,
            &profile.capes,
        )
        .await;
        state.profile = Some(profile);
        state.logged_in = true;
        save_auth_state(&state)?;
    }
    Ok(load_auth_state())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn mc_apply_skin(skin_url: String, variant: String) -> Result<AuthState, String> {
    let access_token = load_mc_access_token()?;
    apply_minecraft_skin(&access_token, &skin_url, &variant).await?;
    refresh_profile_after_skin_change(&access_token).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn mc_upload_skin(png_base64: String, variant: String) -> Result<AuthState, String> {
    let access_token = load_mc_access_token()?;
    upload_minecraft_skin_bytes(&access_token, &png_base64, &variant).await?;
    refresh_profile_after_skin_change(&access_token).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn mc_upload_skin_file(path: String, variant: String) -> Result<AuthState, String> {
    let bytes = std::fs::read(&path).map_err(|e| format!("cannot read skin file: {e}"))?;
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let access_token = load_mc_access_token()?;
    upload_minecraft_skin_bytes(&access_token, &b64, &variant).await?;
    refresh_profile_after_skin_change(&access_token).await
}

#[tauri::command(rename_all = "camelCase")]
pub async fn mc_logout() -> Result<AuthState, String> {
    let state = load_auth_state();
    if let Some(uuid) = state.active_account_uuid.clone() {
        invalidate_cape_cache(&uuid);
        let _ = clear_token(&account_access_key(&uuid));
        let _ = clear_token(&account_refresh_key(&uuid));
        let _ = remove_account_from_list(&uuid);
    }
    let _ = clear_token("mc-access-token");
    let _ = clear_token("mc-refresh-token");
    let _ = clear_token("mc-device-code");
    let _ = clear_token("mc-device-interval");

    let accounts = load_accounts_file();
    if let Some(next) = accounts.active_account_uuid.clone() {
        return mc_switch_account(next).await;
    }
    let new_state = AuthState {
        logged_in: false,
        profile: None,
        expires_at: None,
        login_type: LoginType::default(),
        skin_source: SkinSource::default(),
        cape_provider: state.cape_provider,
        accounts: accounts.accounts,
        active_account_uuid: None,
    };
    save_auth_state(&new_state)?;
    Ok(new_state)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn mc_apply_cape(cape_id: String) -> Result<AuthState, String> {
    let access_token = load_mc_access_token()?;
    apply_minecraft_cape(&access_token, &cape_id).await?;

    // Refresh Mojang profile so ACTIVE cape updates, then re-apply display provider.
    let mut state = load_auth_state();
    if let Ok(mut profile) = fetch_mc_profile(&access_token).await {
        invalidate_cape_cache(&profile.uuid);
        if let Some(ref skin_url) = profile.skin_url {
            let _ = download_and_cache_skin(skin_url, &profile.uuid).await;
        }
        // Prefer Mojang display after activating a cape.
        state.cape_provider = CapeProvider::Mojang;
        profile.cape_url = resolve_display_cape(
            &profile.name,
            &profile.uuid,
            &CapeProvider::Mojang,
            &profile.capes,
        )
        .await;
        state.profile = Some(profile);
        state.logged_in = true;
        save_auth_state(&state)?;
    }
    Ok(load_auth_state())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn mc_check_entitlement() -> Result<bool, String> {
    let access_token = load_mc_access_token()?;
    check_minecraft_entitlement(&access_token).await
}

// ─── Skin base64 for 3D viewer ──────────────────────────────────

#[tauri::command(rename_all = "camelCase")]
pub async fn mc_get_skin_base64(url: String) -> Result<String, String> {
    fetch_skin_as_base64(&url).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_state_serializes() {
        let state = AuthState::default();
        let json = serde_json::to_string(&state).unwrap();
        // AuthState uses rename_all = "camelCase".
        assert!(
            json.contains("loggedIn"),
            "expected camelCase loggedIn in {json}"
        );
    }

    #[test]
    fn skin_cache_path_is_deterministic() {
        let a = cached_skin_path("abc123");
        let b = cached_skin_path("abc123");
        assert_eq!(a, b);
    }

    #[test]
    fn skin_url_cache_path_is_deterministic() {
        let a = cached_skin_path_for_url("https://textures.minecraft.net/texture/abc");
        let b = cached_skin_path_for_url("https://textures.minecraft.net/texture/abc");
        let c = cached_skin_path_for_url("https://textures.minecraft.net/texture/xyz");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(a
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("url-") && n.ends_with(".png")));
    }

    #[test]
    fn offline_uuid_is_deterministic() {
        assert_eq!(offline_uuid("Steve"), offline_uuid("Steve"));
        assert_ne!(offline_uuid("Steve"), offline_uuid("Alex"));
    }

    #[test]
    fn prefer_https_upgrades_http() {
        assert_eq!(
            prefer_https("http://auth.tlauncher.org/skin/x.png"),
            "https://auth.tlauncher.org/skin/x.png"
        );
        assert_eq!(
            prefer_https("https://auth.tlauncher.org/skin/x.png"),
            "https://auth.tlauncher.org/skin/x.png"
        );
    }

    #[test]
    fn skin_source_serializes() {
        let src = SkinSource::Elyby;
        let json = serde_json::to_string(&src).unwrap();
        assert_eq!(json, "\"elyby\"");
    }

    #[test]
    fn account_entry_serializes() {
        let entry = AccountEntry {
            uuid: "abc123".to_string(),
            name: "Test".to_string(),
            login_type: LoginType::Microsoft,
            skin_source: SkinSource::Mojang,
            added_at: 12345,
            authority: None,
            ms_oauth_backend: Some(MsOauthBackend::Live),
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("uuid"));
        assert!(json.contains("loginType"));
    }
}
