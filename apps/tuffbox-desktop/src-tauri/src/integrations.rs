use reqwest::header::{ACCEPT, AUTHORIZATION, USER_AGENT};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};
use tauri::{AppHandle, Emitter};
use tuffbox_core::ProjectManifest;

const KEYRING_SERVICE: &str = "dev.tuffbox.ide";
const DEFAULT_GITHUB_REPOSITORY: &str = "MFcrychelt/tuffbox";
const APP_USER_AGENT: &str = "TuffBox-IDE/0.1";
/// Default Ollama tag for crash plans (smarter; user still must pull once).
pub const DEFAULT_OLLAMA_MODEL: &str = "qwen2.5:7b";
/// Default cloud model (DeepSeek via OpenRouter).
pub const DEFAULT_CLOUD_MODEL: &str = "deepseek/deepseek-chat";
const OLLAMA_PULL_PROGRESS_EVENT: &str = "ollama-pull-progress";
const OLLAMA_PULL_FINISHED_EVENT: &str = "ollama-pull-finished";
/// Distinct error returned when the user pauses an in-flight model pull.
/// Partial blobs stay on disk; a later pull of the same tag resumes.
pub const OLLAMA_PULL_PAUSED_MSG: &str = "OLLAMA_PULL_PAUSED";

static OLLAMA_PULL_CANCEL: AtomicBool = AtomicBool::new(false);
static OLLAMA_PULL_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OllamaPullSnapshot {
    /// `idle` | `running` | `paused` | `failed` | `succeeded`
    pub phase: String,
    pub model: String,
    pub completed: u64,
    pub total: u64,
    pub task_id: String,
    pub error: Option<String>,
    pub models_path: Option<String>,
}

static OLLAMA_PULL_SNAPSHOT: LazyLock<Mutex<OllamaPullSnapshot>> =
    LazyLock::new(|| Mutex::new(OllamaPullSnapshot {
        phase: "idle".into(),
        ..Default::default()
    }));

fn is_ollama_pull_paused(err: &str) -> bool {
    err.contains(OLLAMA_PULL_PAUSED_MSG)
}

fn ollama_pull_task_id(model: &str) -> String {
    format!("ollama-pull-{model}")
}

fn set_ollama_pull_snapshot(mut next: OllamaPullSnapshot) {
    if next.phase.is_empty() {
        next.phase = "idle".into();
    }
    if let Ok(mut g) = OLLAMA_PULL_SNAPSHOT.lock() {
        *g = next;
    }
}

fn patch_ollama_pull_progress(model: &str, status: &str, completed: u64, total: u64) {
    if let Ok(mut g) = OLLAMA_PULL_SNAPSHOT.lock() {
        g.model = model.to_string();
        g.completed = completed;
        g.total = total;
        if status.eq_ignore_ascii_case("paused") {
            g.phase = "paused".into();
        } else if g.phase != "paused" {
            g.phase = "running".into();
        }
    }
}

fn emit_ollama_pull_progress(
    app: Option<&AppHandle>,
    task_id: Option<&str>,
    model: &str,
    status: &str,
    completed: u64,
    total: u64,
) {
    patch_ollama_pull_progress(model, status, completed, total);
    if let Some(tid) = task_id {
        let pct = if total > 0 {
            (completed as f64 / total as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let detail = if total > 0 {
            format!("{status} · {completed}/{total}")
        } else if status.is_empty() {
            "downloading…".into()
        } else {
            status.to_string()
        };
        tuffbox_core::task_progress::set_progress(tid, pct, Some(detail));
    }
    if let Some(handle) = app {
        let _ = handle.emit(
            OLLAMA_PULL_PROGRESS_EVENT,
            json!({
                "model": model,
                "status": status,
                "completed": completed,
                "total": total,
            }),
        );
    }
}

fn emit_ollama_pull_paused(
    app: Option<&AppHandle>,
    task_id: Option<&str>,
    model: &str,
    completed: u64,
    total: u64,
) {
    emit_ollama_pull_progress(app, task_id, model, "paused", completed, total);
    if let Some(tid) = task_id {
        tuffbox_core::task_progress::pause(
            tid,
            Some("Paused — resume anytime; download continues from here".into()),
        );
    }
}

/// Snapshot of the current / last AI model download (for UI restore).
#[tauri::command(rename_all = "camelCase")]
pub fn get_ollama_pull_status() -> OllamaPullSnapshot {
    OLLAMA_PULL_SNAPSHOT
        .lock()
        .map(|g| g.clone())
        .unwrap_or_default()
}

/// Pause the active Ollama model download. Incomplete layers are kept so Resume
/// (another `pull_ollama_model` of the same tag) continues from the same place.
#[tauri::command(rename_all = "camelCase")]
pub fn pause_ollama_model_pull() -> Result<(), String> {
    if !OLLAMA_PULL_IN_FLIGHT.load(Ordering::SeqCst) {
        return Err("No model download is in progress".into());
    }
    OLLAMA_PULL_CANCEL.store(true, Ordering::SeqCst);
    Ok(())
}

/// Installed Ollama model metadata from `/api/tags`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OllamaModelInfo {
    pub name: String,
    #[serde(default)]
    pub size_bytes: u64,
    #[serde(default)]
    pub parameter_size: String,
    #[serde(default)]
    pub quantization: String,
    #[serde(default)]
    pub family: String,
    /// Heuristic fit vs host RAM: `ok` | `tight` | `heavy` | `unknown`.
    #[serde(default)]
    pub fit: String,
}

/// Curated install suggestion with size / hardware hints.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestedModel {
    pub name: String,
    pub note: String,
    pub approx_size_bytes: u64,
    pub min_ram_gb: f64,
    pub min_vram_gb: f64,
}

fn suggested_model_catalog() -> Vec<SuggestedModel> {
    vec![
        SuggestedModel {
            name: DEFAULT_OLLAMA_MODEL.into(),
            note: "Default — better crash plans".into(),
            approx_size_bytes: 4_700_000_000,
            min_ram_gb: 8.0,
            min_vram_gb: 6.0,
        },
        SuggestedModel {
            name: "llama3.1:8b".into(),
            note: "Strong alternative".into(),
            approx_size_bytes: 4_900_000_000,
            min_ram_gb: 10.0,
            min_vram_gb: 7.0,
        },
        SuggestedModel {
            name: "llama3.2:3b".into(),
            note: "Fast / weaker plans".into(),
            approx_size_bytes: 2_000_000_000,
            min_ram_gb: 6.0,
            min_vram_gb: 3.0,
        },
        SuggestedModel {
            name: "phi3:mini".into(),
            note: "Fast / weaker plans".into(),
            approx_size_bytes: 2_300_000_000,
            min_ram_gb: 6.0,
            min_vram_gb: 3.5,
        },
    ]
}

fn host_ram_bytes() -> u64 {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    sys.total_memory()
}

fn parse_param_billions(text: &str) -> Option<f64> {
    let lower = text.to_ascii_lowercase();
    // Prefer explicit "7B" / "3.2B" tokens.
    for token in lower
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '.' && c != '_')
        .filter(|t| !t.is_empty())
    {
        if let Some(num) = token.strip_suffix('b') {
            if let Ok(v) = num.parse::<f64>() {
                if v > 0.0 && v < 10_000.0 {
                    return Some(v);
                }
            }
        }
    }
    None
}

fn quant_bytes_per_param(quant: &str) -> f64 {
    let q = quant.to_ascii_lowercase();
    if q.contains("q2") {
        0.3
    } else if q.contains("q3") {
        0.4
    } else if q.contains("q4") {
        0.55
    } else if q.contains("q5") {
        0.7
    } else if q.contains("q6") {
        0.8
    } else if q.contains("q8") {
        1.0
    } else if q.contains("f16") || q.contains("fp16") {
        2.0
    } else if q.contains("f32") || q.contains("fp32") {
        4.0
    } else {
        0.55 // typical Ollama default quant
    }
}

fn estimate_fit(size_bytes: u64, parameter_size: &str, quantization: &str, name: &str) -> String {
    let ram = host_ram_bytes();
    if ram == 0 {
        return "unknown".into();
    }
    let params_b = parse_param_billions(parameter_size)
        .or_else(|| parse_param_billions(name))
        .unwrap_or(0.0);
    let estimated = if size_bytes > 0 {
        size_bytes as f64
    } else if params_b > 0.0 {
        params_b * 1e9 * quant_bytes_per_param(quantization)
    } else {
        return "unknown".into();
    };
    // Runtime working set ~1.2× weights for KV/context headroom.
    let need = estimated * 1.2;
    let ram_f = ram as f64;
    if need <= ram_f * 0.55 {
        "ok".into()
    } else if need <= ram_f * 0.85 {
        "tight".into()
    } else {
        "heavy".into()
    }
}

fn disk_space_for_path(path: &Path) -> (u64, u64) {
    use sysinfo::Disks;
    let disks = Disks::new_with_refreshed_list();
    let abs = path
        .canonicalize()
        .unwrap_or_else(|_| path.to_path_buf());
    let abs_s = abs.to_string_lossy().to_ascii_lowercase();
    let mut best: Option<(usize, u64, u64)> = None;
    for disk in disks.list() {
        let mount = disk.mount_point().to_string_lossy().to_ascii_lowercase();
        if mount.is_empty() {
            continue;
        }
        if abs_s.starts_with(&mount) {
            let len = mount.len();
            let avail = disk.available_space();
            let total = disk.total_space();
            if best.map(|(l, _, _)| len > l).unwrap_or(true) {
                best = Some((len, avail, total));
            }
        }
    }
    best.map(|(_, a, t)| (a, t)).unwrap_or((0, 0))
}

fn model_info_names(models: &[OllamaModelInfo]) -> Vec<String> {
    models.iter().map(|m| m.name.clone()).collect()
}

/// Pick full vs compact crash Explain prompt for the configured AI settings.
pub fn crash_explain_prompt_for(
    settings: &AiSettings,
    ctx: &tuffbox_core::ai_explanation::CrashAiContext,
) -> (String, bool) {
    let compact = tuffbox_core::ai_explanation::prefers_compact_crash_prompt(
        &settings.provider,
        &settings.model,
    );
    if compact {
        (
            tuffbox_core::ai_explanation::build_compact_crash_prompt(ctx),
            true,
        )
    } else {
        (
            tuffbox_core::ai_explanation::build_crash_prompt(ctx),
            false,
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSettings {
    pub provider: String,
    pub endpoint: String,
    pub model: String,
    /// Crash diagnose transport: `server` (default) | `local` | `kb_only`.
    #[serde(default = "default_diagnose_mode")]
    pub diagnose_mode: String,
    /// Base URL for private crash KB API (`/v1/crash/lookup`, `/v1/crash/diagnose`).
    #[serde(default)]
    pub crash_kb_endpoint: String,
    /// Optional path to `ollama` / `ollama.exe`, or the install folder that contains it.
    /// Empty = look up `ollama` on PATH / default install locations.
    #[serde(default)]
    pub ollama_binary_path: String,
    /// Directory where Ollama stores models (`OLLAMA_MODELS`). Empty = Ollama default
    /// (`~/.ollama/models` / `%USERPROFILE%\.ollama\models`).
    #[serde(default)]
    pub ollama_models_path: String,
    /// Opt-in L3 draft→verify: small local draft model proposes ActionPlan, main model validates.
    #[serde(default)]
    pub speculative_decoding: bool,
    /// Draft model id/tag (Ollama or OpenAI-compatible). Empty → `qwen2.5-coder:0.5b`.
    #[serde(default = "default_draft_model")]
    pub draft_model: String,
    /// Tune Config Advisor: allow allowlisted web research for unknown config keys.
    #[serde(default = "default_true_bool")]
    pub tune_web_research: bool,
}

fn default_true_bool() -> bool {
    true
}

fn default_diagnose_mode() -> String {
    "server".into()
}

fn default_draft_model() -> String {
    crate::speculative::DEFAULT_DRAFT_MODEL.into()
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            provider: "openai-compatible".to_string(),
            endpoint: "https://openrouter.ai/api/v1".to_string(),
            model: DEFAULT_CLOUD_MODEL.to_string(),
            diagnose_mode: default_diagnose_mode(),
            crash_kb_endpoint: String::new(),
            ollama_binary_path: String::new(),
            ollama_models_path: String::new(),
            speculative_decoding: false,
            draft_model: default_draft_model(),
            tune_web_research: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationSettings {
    pub github_repository: String,
    pub ai: AiSettings,
    #[serde(default)]
    pub swarm: tuffbox_core::swarm::SwarmSettings,
}

impl Default for IntegrationSettings {
    fn default() -> Self {
        Self {
            github_repository: DEFAULT_GITHUB_REPOSITORY.to_string(),
            ai: AiSettings::default(),
            swarm: tuffbox_core::swarm::SwarmSettings::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PublishConfig {
    pub github_repository: String,
    pub modrinth_project_id: String,
    pub curseforge_project_id: String,
    pub curseforge_game_version_ids: Vec<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationStatus {
    pub settings: IntegrationSettings,
    pub github_token_set: bool,
    pub modrinth_token_set: bool,
    pub curseforge_token_set: bool,
    pub ai_api_key_set: bool,
    pub crash_kb_token_set: bool,
    pub swarm_supabase_anon_set: bool,
    /// Community Supabase URL+key are built into the app (no user setup required).
    pub swarm_supabase_using_builtin: bool,
    pub swarm_supabase_configured: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheck {
    pub current_version: String,
    pub latest_version: String,
    pub update_available: bool,
    pub release_url: Option<String>,
    pub checked_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishResult {
    pub target: String,
    pub id: String,
    pub url: Option<String>,
    pub uploaded_files: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ArtifactRecord {
    kind: String,
    path: String,
}

fn settings_path() -> PathBuf {
    dirs::config_dir()
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("TuffBox")
        .join("integrations.json")
}

pub(crate) fn read_settings() -> IntegrationSettings {
    fs::read_to_string(settings_path())
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn write_settings(settings: &IntegrationSettings) -> Result<(), String> {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(
        &tmp,
        serde_json::to_vec_pretty(settings).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::rename(&tmp, &path)
        .or_else(|_| {
            fs::remove_file(&path).ok();
            fs::rename(&tmp, &path)
        })
        .map_err(|e| e.to_string())?;
    // So tray / future Ollama launches honor the same folder (not only our serve spawn).
    persist_ollama_models_user_env(settings.ai.ollama_models_path.trim());
    Ok(())
}

/// Persist `OLLAMA_MODELS` at the user environment level.
/// Empty path clears the override so Ollama falls back to its default.
/// On Windows prefer PowerShell `[Environment]::SetEnvironmentVariable` over `setx`
/// (Unicode paths, reliable User hive write, immediate read-back for new processes).
fn persist_ollama_models_user_env(models_path: &str) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let ps = if models_path.is_empty() {
            "[Environment]::SetEnvironmentVariable('OLLAMA_MODELS', $null, 'User')".to_string()
        } else {
            let escaped = models_path.replace('\'', "''");
            format!(
                "[Environment]::SetEnvironmentVariable('OLLAMA_MODELS', '{escaped}', 'User')"
            )
        };
        let _ = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
            .creation_flags(0x08000000)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        // Also set for this process so child spawns inherit immediately.
        if models_path.is_empty() {
            std::env::remove_var("OLLAMA_MODELS");
        } else {
            std::env::set_var("OLLAMA_MODELS", models_path);
        }
    }
    #[cfg(not(windows))]
    {
        if models_path.is_empty() {
            std::env::remove_var("OLLAMA_MODELS");
        } else {
            std::env::set_var("OLLAMA_MODELS", models_path);
        }
    }
}

/// Host for a short-lived TuffBox-managed `ollama serve` used only for pulls when a
/// custom models path is set. Avoids racing the tray app on :11434 (which often keeps
/// writing under `%USERPROFILE%\.ollama\models`).
const MANAGED_OLLAMA_HOST: &str = "127.0.0.1:18434";

fn managed_ollama_root() -> String {
    format!("http://{MANAGED_OLLAMA_HOST}")
}

/// Current swarm settings from integrations.json.
pub fn swarm_settings() -> tuffbox_core::swarm::SwarmSettings {
    read_settings().swarm
}

pub fn swarm_enabled() -> bool {
    swarm_settings().enabled
}

/// Prefer TuffSwarm hub URL, else private Crash KB endpoint.
pub fn swarm_network_base() -> Option<String> {
    let s = read_settings();
    tuffbox_core::swarm::resolve_swarm_network_base(&s.swarm.hub_url, &s.ai.crash_kb_endpoint)
}

/// Supabase project URL: Settings override, else built-in community project.
pub fn swarm_supabase_url() -> Option<String> {
    swarm_settings().effective_supabase_url()
}

/// Anon/publishable key for PostgREST: keyring override, else built-in publishable.
pub fn swarm_supabase_anon_key() -> Option<String> {
    if let Some(k) = secret_optional("swarm_supabase") {
        return Some(k);
    }
    let builtin = tuffbox_core::swarm::BUILTIN_SUPABASE_ANON_KEY.trim();
    if builtin.is_empty() {
        None
    } else {
        Some(builtin.to_string())
    }
}

/// JWT anon for Edge `functions/v1/*`. Keyring override only when it looks like a JWT
/// (`eyJ…`); otherwise always the built-in legacy anon (publishable keys fail the gateway).
pub fn swarm_supabase_edge_anon_key() -> Option<String> {
    if let Some(k) = secret_optional("swarm_supabase") {
        let t = k.trim();
        if t.starts_with("eyJ") {
            return Some(t.to_string());
        }
    }
    let builtin = tuffbox_core::swarm::BUILTIN_SUPABASE_EDGE_ANON_KEY.trim();
    if builtin.is_empty() {
        None
    } else {
        Some(builtin.to_string())
    }
}

/// True when effective URL + anon key resolve (built-in counts).
pub fn swarm_supabase_configured() -> bool {
    swarm_supabase_url().is_some()
        && swarm_supabase_anon_key().is_some()
        && swarm_supabase_edge_anon_key().is_some()
}

/// Whether the client is using the shipped community Supabase defaults (no overrides).
pub fn swarm_supabase_using_builtin() -> bool {
    let s = swarm_settings();
    s.supabase_url.trim().is_empty() && secret_optional("swarm_supabase").is_none()
}

/// Machine-wide durable capsule store (shared across projects on this PC).
pub fn global_capsule_library() -> tuffbox_core::swarm::CapsuleLibrary {
    let path = dirs::config_dir()
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("TuffBox")
        .join("swarm")
        .join("capsules.jsonl");
    tuffbox_core::swarm::CapsuleLibrary::open(path)
}

pub fn require_swarm_enabled() -> Result<(), String> {
    if swarm_enabled() {
        Ok(())
    } else {
        Err(
            "TuffSwarm network is disabled. Enable it in Settings → Use TuffSwarm network."
                .into(),
        )
    }
}

/// Complete first-run onboarding and set enabled flag.
#[tauri::command(rename_all = "camelCase")]
pub fn complete_swarm_onboarding(enabled: bool) -> Result<tuffbox_core::swarm::SwarmSettings, String> {
    let mut settings = read_settings();
    settings.swarm.enabled = enabled;
    settings.swarm.onboarding_done = true;
    write_settings(&settings)?;
    Ok(settings.swarm)
}

#[tauri::command(rename_all = "camelCase")]
pub fn get_swarm_settings() -> tuffbox_core::swarm::SwarmSettings {
    swarm_settings()
}

#[tauri::command(rename_all = "camelCase")]
pub fn set_swarm_enabled(enabled: bool) -> Result<tuffbox_core::swarm::SwarmSettings, String> {
    let mut settings = read_settings();
    settings.swarm.enabled = enabled;
    settings.swarm.onboarding_done = true;
    write_settings(&settings)?;
    Ok(settings.swarm)
}

#[tauri::command(rename_all = "camelCase")]
pub fn set_swarm_share_prompts(enabled: bool) -> Result<tuffbox_core::swarm::SwarmSettings, String> {
    let mut settings = read_settings();
    settings.swarm.share_prompts_enabled = enabled;
    write_settings(&settings)?;
    Ok(settings.swarm)
}

#[tauri::command(rename_all = "camelCase")]
pub fn set_swarm_hub_url(hub_url: String) -> Result<tuffbox_core::swarm::SwarmSettings, String> {
    let mut settings = read_settings();
    settings.swarm.hub_url = hub_url.trim().trim_end_matches('/').to_string();
    write_settings(&settings)?;
    Ok(settings.swarm)
}

#[tauri::command(rename_all = "camelCase")]
pub fn set_swarm_supabase_url(
    supabase_url: String,
) -> Result<tuffbox_core::swarm::SwarmSettings, String> {
    let mut settings = read_settings();
    let url = supabase_url.trim().trim_end_matches('/').to_string();
    if !url.is_empty()
        && !url.starts_with("https://")
        && !url.starts_with("http://127.0.0.1")
        && !url.starts_with("http://localhost")
    {
        return Err("Supabase URL must be https://… (or localhost for local stack)".into());
    }
    settings.swarm.supabase_url = url;
    write_settings(&settings)?;
    Ok(settings.swarm)
}

#[tauri::command(rename_all = "camelCase")]
pub fn set_swarm_p2p(
    enabled: bool,
    control_url: Option<String>,
    bootstrap: Option<String>,
) -> Result<tuffbox_core::swarm::SwarmSettings, String> {
    let mut settings = read_settings();
    settings.swarm.p2p_enabled = enabled;
    settings.swarm.onboarding_done = true;
    if !enabled {
        settings.swarm.volunteer_diagnose = false;
        settings.swarm.creation_worker = false;
        settings.swarm.p2p_relay_server = false;
    }
    if let Some(url) = control_url {
        let url = url.trim().trim_end_matches('/').to_string();
        if !url.is_empty() {
            settings.swarm.p2p_control_url = url;
        }
    }
    if let Some(boot) = bootstrap {
        settings.swarm.p2p_bootstrap = boot.trim().to_string();
    }
    if settings.swarm.p2p_control_url.trim().is_empty() {
        settings.swarm.p2p_control_url = tuffbox_core::swarm::SwarmSettings::default()
            .p2p_control_url;
    }
    write_settings(&settings)?;
    Ok(settings.swarm)
}

#[tauri::command(rename_all = "camelCase")]
pub fn set_swarm_volunteer_diagnose(
    enabled: bool,
) -> Result<tuffbox_core::swarm::SwarmSettings, String> {
    let mut settings = read_settings();
    settings.swarm.volunteer_diagnose = enabled;
    settings.swarm.onboarding_done = true;
    write_settings(&settings)?;
    Ok(settings.swarm)
}

#[tauri::command(rename_all = "camelCase")]
pub fn set_swarm_creation_worker(
    enabled: bool,
) -> Result<tuffbox_core::swarm::SwarmSettings, String> {
    let mut settings = read_settings();
    settings.swarm.creation_worker = enabled;
    settings.swarm.onboarding_done = true;
    write_settings(&settings)?;
    Ok(settings.swarm)
}

#[tauri::command(rename_all = "camelCase")]
pub fn set_swarm_p2p_relay_server(
    enabled: bool,
) -> Result<tuffbox_core::swarm::SwarmSettings, String> {
    let mut settings = read_settings();
    settings.swarm.p2p_relay_server = enabled;
    settings.swarm.onboarding_done = true;
    write_settings(&settings)?;
    Ok(settings.swarm)
}

#[tauri::command(rename_all = "camelCase")]
pub fn set_swarm_advertised_vram_mb(
    vram_mb: u32,
) -> Result<tuffbox_core::swarm::SwarmSettings, String> {
    let mut settings = read_settings();
    settings.swarm.advertised_vram_mb = vram_mb;
    settings.swarm.onboarding_done = true;
    write_settings(&settings)?;
    Ok(settings.swarm)
}

fn keyring_entry(kind: &str) -> Result<keyring::Entry, String> {
    let account = match kind {
        "github" => "github-token",
        "modrinth" => "modrinth-token",
        "curseforge" => "curseforge-token",
        "ai" => "ai-api-key",
        "crash_kb" => "crash-kb-token",
        "swarm_supabase" => "swarm-supabase-anon",
        _ => return Err(format!("unknown credential kind: {kind}")),
    };
    keyring::Entry::new(KEYRING_SERVICE, account).map_err(|e| e.to_string())
}

fn secret(kind: &str) -> Result<String, String> {
    keyring_entry(kind)?
        .get_password()
        .map_err(|_| format!("{kind} credential is not configured"))
}

pub fn secret_optional(kind: &str) -> Option<String> {
    secret(kind).ok().filter(|s| !s.trim().is_empty())
}

fn secret_is_set(kind: &str) -> bool {
    keyring_entry(kind)
        .and_then(|entry| entry.get_password().map_err(|e| e.to_string()))
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

#[tauri::command]
pub fn get_integration_status() -> IntegrationStatus {
    IntegrationStatus {
        settings: read_settings(),
        github_token_set: secret_is_set("github"),
        modrinth_token_set: secret_is_set("modrinth"),
        curseforge_token_set: secret_is_set("curseforge"),
        ai_api_key_set: secret_is_set("ai"),
        crash_kb_token_set: secret_is_set("crash_kb"),
        swarm_supabase_anon_set: secret_is_set("swarm_supabase"),
        swarm_supabase_using_builtin: swarm_supabase_using_builtin(),
        swarm_supabase_configured: swarm_supabase_configured(),
    }
}

#[tauri::command]
pub fn save_integration_settings(mut settings: IntegrationSettings) -> Result<(), String> {
    if settings.github_repository.split('/').count() != 2 {
        return Err("GitHub repository must use owner/repository format".to_string());
    }
    if settings.ai.endpoint.trim().is_empty() {
        return Err("AI endpoint is required".to_string());
    }
    if settings.ai.provider != "ollama" && settings.ai.model.trim().is_empty() {
        return Err("AI model is required".to_string());
    }
    if !matches!(
        settings.ai.provider.as_str(),
        "ollama" | "openai-compatible"
    ) {
        return Err("AI provider must be ollama or openai-compatible".to_string());
    }
    let mode = tuffbox_core::action_plan::DiagnoseMode::parse(&settings.ai.diagnose_mode);
    settings.ai.diagnose_mode = mode.as_str().to_string();
    // Preserve swarm when older clients omit the field (serde default).
    let existing = read_settings();
    if !settings.swarm.onboarding_done && existing.swarm.onboarding_done {
        settings.swarm = existing.swarm;
    }
    write_settings(&settings)
}

#[tauri::command]
pub fn set_integration_secret(kind: String, value: String) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err("credential cannot be empty".to_string());
    }
    keyring_entry(&kind)?
        .set_password(value.trim())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn clear_integration_secret(kind: String) -> Result<(), String> {
    let entry = keyring_entry(&kind)?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn publish_config_path(manifest_path: &Path) -> Result<PathBuf, String> {
    let project_dir = manifest_path
        .parent()
        .ok_or_else(|| "manifest path has no parent directory".to_string())?;
    Ok(project_dir.join(".tuffbox").join("publish.json"))
}

#[tauri::command]
pub fn get_publish_config(path: String) -> Result<PublishConfig, String> {
    let config_path = publish_config_path(Path::new(&path))?;
    let mut config: PublishConfig = fs::read_to_string(config_path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    if config.github_repository.is_empty() {
        config.github_repository = read_settings().github_repository;
    }
    Ok(config)
}

#[tauri::command]
pub fn save_publish_config(path: String, config: PublishConfig) -> Result<(), String> {
    if !config.github_repository.is_empty() && config.github_repository.split('/').count() != 2 {
        return Err("GitHub repository must use owner/repository format".to_string());
    }
    let config_path = publish_config_path(Path::new(&path))?;
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(
        config_path,
        serde_json::to_vec_pretty(&config).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

fn github_headers(
    request: reqwest::RequestBuilder,
    token: Option<&str>,
) -> reqwest::RequestBuilder {
    let request = request
        .header(USER_AGENT, APP_USER_AGENT)
        .header(ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(token) = token {
        request.header(AUTHORIZATION, format!("Bearer {token}"))
    } else {
        request
    }
}

fn normalized_semver(value: &str) -> Option<semver::Version> {
    let trimmed = value.trim().trim_start_matches(['v', 'V']);
    semver::Version::parse(trimmed).ok()
}

#[tauri::command]
pub async fn check_for_app_update() -> Result<UpdateCheck, String> {
    let settings = read_settings();
    let repository = if settings.github_repository.trim().is_empty() {
        DEFAULT_GITHUB_REPOSITORY
    } else {
        settings.github_repository.trim()
    };
    let url = format!("https://api.github.com/repos/{repository}/releases/latest");
    let response = github_headers(reqwest::Client::new().get(url), None)
        .send()
        .await
        .map_err(|e| format!("GitHub update check failed: {e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "GitHub update check failed with status {}",
            response.status()
        ));
    }
    let release: Value = response.json().await.map_err(|e| e.to_string())?;
    let current_version = env!("CARGO_PKG_VERSION").to_string();
    let latest_version = release
        .get("tag_name")
        .and_then(Value::as_str)
        .unwrap_or(&current_version)
        .to_string();
    let update_available = match (
        normalized_semver(&current_version),
        normalized_semver(&latest_version),
    ) {
        (Some(current), Some(latest)) => latest > current,
        _ => latest_version != current_version,
    };
    Ok(UpdateCheck {
        current_version,
        latest_version,
        update_available,
        release_url: release
            .get("html_url")
            .and_then(Value::as_str)
            .map(str::to_string),
        checked_at: tuffbox_core::time_util::rfc3339_now(),
    })
}

#[tauri::command]
pub async fn test_integration(provider: String) -> Result<String, String> {
    match provider.as_str() {
        "github" => {
            let token = secret("github")?;
            let response = github_headers(
                reqwest::Client::new().get("https://api.github.com/user"),
                Some(&token),
            )
            .send()
            .await
            .map_err(|e| e.to_string())?;
            if !response.status().is_success() {
                return Err(format!("GitHub rejected the token ({})", response.status()));
            }
            let body: Value = response.json().await.map_err(|e| e.to_string())?;
            Ok(format!(
                "Connected as {}",
                body.get("login")
                    .and_then(Value::as_str)
                    .unwrap_or("GitHub user")
            ))
        }
        "modrinth" => {
            let token = secret("modrinth")?;
            let response = reqwest::Client::new()
                .get("https://api.modrinth.com/v2/user")
                .header(USER_AGENT, APP_USER_AGENT)
                .bearer_auth(token)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if !response.status().is_success() {
                return Err(format!(
                    "Modrinth rejected the token ({})",
                    response.status()
                ));
            }
            let body: Value = response.json().await.map_err(|e| e.to_string())?;
            Ok(format!(
                "Connected as {}",
                body.get("username")
                    .and_then(Value::as_str)
                    .unwrap_or("Modrinth user")
            ))
        }
        "curseforge" => {
            let token = secret("curseforge")?;
            let response = reqwest::Client::new()
                .get("https://minecraft.curseforge.com/api/game/versions")
                .header(USER_AGENT, APP_USER_AGENT)
                .header("X-Api-Token", token)
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if !response.status().is_success() {
                return Err(format!(
                    "CurseForge rejected the token ({})",
                    response.status()
                ));
            }
            Ok("CurseForge token accepted".to_string())
        }
        "ai" => {
            let settings = read_settings();
            // A connection probe must stay snappy: the underlying client
            // retries with long per-attempt timeouts (worst case ~10 minutes
            // on a black-holing endpoint), which made "Test connection" and
            // the Quest-AI preflight appear to hang forever.
            match tokio::time::timeout(
                std::time::Duration::from_secs(30),
                call_ai(&settings.ai, "Respond with exactly: {\"status\":\"ok\"}"),
            )
            .await
            {
                Ok(result) => {
                    result?;
                    Ok(format!(
                        "{} model {} responded",
                        settings.ai.provider, settings.ai.model
                    ))
                }
                Err(_) => Err(format!(
                    "AI test timed out after 30s — {} endpoint {} did not answer. \
                     Check Settings → AI (provider, endpoint, model).",
                    settings.ai.provider, settings.ai.endpoint
                )),
            }
        }
        _ => Err(format!("unknown integration provider: {provider}")),
    }
}

fn read_artifacts(manifest_path: &Path) -> Result<Vec<ArtifactRecord>, String> {
    let project_dir = manifest_path
        .parent()
        .ok_or_else(|| "manifest path has no parent directory".to_string())?;
    let path = project_dir.join(".tuffbox").join("artifacts.json");
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}

fn latest_artifact<'a>(
    artifacts: &'a [ArtifactRecord],
    kind: &str,
) -> Result<&'a ArtifactRecord, String> {
    artifacts
        .iter()
        .rev()
        .find(|artifact| artifact.kind == kind && Path::new(&artifact.path).is_file())
        .ok_or_else(|| format!("export a {kind} artifact before publishing"))
}

#[tauri::command]
pub async fn publish_release(
    path: String,
    target: String,
    changelog: String,
) -> Result<PublishResult, String> {
    let manifest_path = PathBuf::from(&path);
    let manifest = ProjectManifest::load_from_path(&manifest_path).map_err(|e| e.to_string())?;
    let config = get_publish_config(path)?;
    let artifacts = read_artifacts(&manifest_path)?;
    match target.as_str() {
        "github" => publish_github(&manifest, &config, &artifacts, &changelog).await,
        "modrinth" => publish_modrinth(&manifest, &config, &artifacts, &changelog).await,
        "curseforge" => publish_curseforge(&manifest, &config, &artifacts, &changelog).await,
        _ => Err(format!("unknown publish target: {target}")),
    }
}

async fn publish_github(
    manifest: &ProjectManifest,
    config: &PublishConfig,
    artifacts: &[ArtifactRecord],
    changelog: &str,
) -> Result<PublishResult, String> {
    let token = secret("github")?;
    let repository = config.github_repository.trim();
    if repository.split('/').count() != 2 {
        return Err("configure a GitHub repository as owner/repository".to_string());
    }
    let client = reqwest::Client::new();
    let create_url = format!("https://api.github.com/repos/{repository}/releases");
    let tag = format!("v{}", manifest.project.version);
    let payload = json!({
        "tag_name": tag,
        "name": format!("{} {}", manifest.project.name, manifest.project.version),
        "body": changelog,
        "draft": true,
        "prerelease": manifest.project.version.contains("alpha") || manifest.project.version.contains("beta")
    });
    let response = github_headers(client.post(&create_url), Some(&token))
        .json(&payload)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = response.status();
    let body: Value = response.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(api_error("GitHub", status, &body));
    }
    let release_id = body
        .get("id")
        .and_then(Value::as_u64)
        .ok_or_else(|| "GitHub response did not include a release id".to_string())?;
    let mut uploaded_files = Vec::new();
    for artifact in artifacts {
        let file_path = Path::new(&artifact.path);
        if !file_path.is_file() {
            continue;
        }
        let file_name = file_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("invalid artifact filename: {}", artifact.path))?;
        let bytes = fs::read(file_path).map_err(|e| e.to_string())?;
        let upload_url =
            format!("https://uploads.github.com/repos/{repository}/releases/{release_id}/assets");
        let upload = github_headers(
            client
                .post(upload_url)
                .query(&[("name", file_name)])
                .header("Content-Type", "application/octet-stream")
                .body(bytes),
            Some(&token),
        )
        .send()
        .await
        .map_err(|e| e.to_string())?;
        if !upload.status().is_success() {
            let upload_status = upload.status();
            let upload_body: Value = upload.json().await.unwrap_or_default();
            return Err(api_error(
                "GitHub asset upload",
                upload_status,
                &upload_body,
            ));
        }
        uploaded_files.push(file_name.to_string());
    }
    Ok(PublishResult {
        target: "github".to_string(),
        id: release_id.to_string(),
        url: body
            .get("html_url")
            .and_then(Value::as_str)
            .map(str::to_string),
        uploaded_files,
    })
}

async fn publish_modrinth(
    manifest: &ProjectManifest,
    config: &PublishConfig,
    artifacts: &[ArtifactRecord],
    changelog: &str,
) -> Result<PublishResult, String> {
    let token = secret("modrinth")?;
    let project_id = config.modrinth_project_id.trim();
    if project_id.is_empty() {
        return Err("configure a Modrinth project id or slug".to_string());
    }
    let artifact = latest_artifact(artifacts, "mrpack")?;
    let file_name = Path::new(&artifact.path)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "invalid Modrinth artifact filename".to_string())?
        .to_string();
    let loader = tuffbox_core::graph::loader_kind_slug(&manifest.loader.kind);
    let metadata = json!({
        "name": format!("{} {}", manifest.project.name, manifest.project.version),
        "version_number": manifest.project.version,
        "changelog": changelog,
        "dependencies": [],
        "game_versions": [manifest.minecraft.version],
        "version_type": if manifest.project.version.contains("alpha") { "alpha" } else if manifest.project.version.contains("beta") { "beta" } else { "release" },
        "loaders": [loader],
        "featured": false,
        "project_id": project_id,
        "file_parts": ["file"],
        "primary_file": "file"
    });
    let bytes = fs::read(&artifact.path).map_err(|e| e.to_string())?;
    let form = reqwest::multipart::Form::new()
        .text("data", metadata.to_string())
        .part(
            "file",
            reqwest::multipart::Part::bytes(bytes).file_name(file_name.clone()),
        );
    let response = reqwest::Client::new()
        .post("https://api.modrinth.com/v2/version")
        .header(USER_AGENT, APP_USER_AGENT)
        .bearer_auth(token)
        .multipart(form)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = response.status();
    let body: Value = response.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(api_error("Modrinth", status, &body));
    }
    let id = body
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    Ok(PublishResult {
        target: "modrinth".to_string(),
        id,
        url: Some(format!(
            "https://modrinth.com/modpack/{project_id}/versions"
        )),
        uploaded_files: vec![file_name],
    })
}

async fn publish_curseforge(
    manifest: &ProjectManifest,
    config: &PublishConfig,
    artifacts: &[ArtifactRecord],
    changelog: &str,
) -> Result<PublishResult, String> {
    let token = secret("curseforge")?;
    let project_id = config.curseforge_project_id.trim();
    if project_id.is_empty() {
        return Err("configure a CurseForge project id".to_string());
    }
    if config.curseforge_game_version_ids.is_empty() {
        return Err("configure at least one CurseForge game version id".to_string());
    }
    let artifact = latest_artifact(artifacts, "curseforge")?;
    let file_name = Path::new(&artifact.path)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "invalid CurseForge artifact filename".to_string())?
        .to_string();
    let metadata = json!({
        "changelog": changelog,
        "changelogType": "markdown",
        "displayName": format!("{} {}", manifest.project.name, manifest.project.version),
        "gameVersions": config.curseforge_game_version_ids,
        "releaseType": if manifest.project.version.contains("alpha") { "alpha" } else if manifest.project.version.contains("beta") { "beta" } else { "release" }
    });
    let bytes = fs::read(&artifact.path).map_err(|e| e.to_string())?;
    let form = reqwest::multipart::Form::new()
        .text("metadata", metadata.to_string())
        .part(
            "file",
            reqwest::multipart::Part::bytes(bytes).file_name(file_name.clone()),
        );
    let response = reqwest::Client::new()
        .post(format!(
            "https://minecraft.curseforge.com/api/projects/{project_id}/upload-file"
        ))
        .header(USER_AGENT, APP_USER_AGENT)
        .header("X-Api-Token", token)
        .multipart(form)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or_default();
    if !status.is_success() {
        return Err(api_error("CurseForge", status, &body));
    }
    let id = body
        .get("id")
        .or_else(|| body.get("data").and_then(|data| data.get("id")))
        .and_then(Value::as_u64)
        .map(|value| value.to_string())
        .unwrap_or_default();
    Ok(PublishResult {
        target: "curseforge".to_string(),
        id,
        url: Some(format!(
            "https://www.curseforge.com/minecraft/modpacks/{project_id}/files"
        )),
        uploaded_files: vec![file_name],
    })
}

fn body_looks_like_html_edge_block(body: &Value) -> bool {
    let raw = body
        .pointer("/error")
        .and_then(Value::as_str)
        .or_else(|| body.get("error").and_then(Value::as_str))
        .map(|s| s.to_string())
        .unwrap_or_else(|| body.to_string());
    let lower = raw.to_ascii_lowercase();
    lower.contains("<!doctype html")
        || lower.contains("<html")
        || lower.contains("error 403 (forbidden)")
}

fn api_error(service: &str, status: reqwest::StatusCode, body: &Value) -> String {
    // Google edge/WAF often returns an HTML 403 page for blocked VPN/datacenter IPs
    // before the Generative Language API sees the request (not an API-key failure).
    if status.as_u16() == 403 && body_looks_like_html_edge_block(body) {
        return format!(
            "{service} returned 403 Forbidden from Google's edge (HTML error page) — \
             your network/VPN egress IP is likely blocked. Try another VPN server, \
             split-tunnel generativelanguage.googleapis.com, or disable VPN; this is \
             usually not an API key problem"
        );
    }

    let message = body
        .get("message")
        .or_else(|| body.get("description"))
        .or_else(|| body.pointer("/error/message"))
        .and_then(Value::as_str)
        .map(|s| s.to_string())
        .or_else(|| {
            // Gemini OpenAI-compat sometimes returns `[{ "error": { "message": "…" } }]`
            body.as_array()
                .and_then(|arr| arr.first())
                .and_then(|item| item.pointer("/error/message"))
                .and_then(Value::as_str)
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| {
            let raw = body.to_string();
            if raw == "null" || raw.is_empty() {
                "request rejected".to_string()
            } else {
                raw.chars().take(280).collect()
            }
        });
    let mut out = format!("{service} returned {status}: {message}");
    if status == reqwest::StatusCode::UNAUTHORIZED
        || (status.as_u16() == 403 && !message.to_ascii_lowercase().contains("<!doctype"))
    {
        out.push_str(" — check the AI API key in Settings → AI");
    }
    out
}

fn is_gemini_endpoint(endpoint: &str) -> bool {
    let e = endpoint.to_ascii_lowercase();
    e.contains("generativelanguage.googleapis.com") || e.contains("/v1beta/openai")
}

fn gemini_model_id(model: &str) -> String {
    model
        .trim()
        .trim_start_matches("models/")
        .trim()
        .to_string()
}

fn gemini_generate_url(model: &str) -> String {
    format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
        gemini_model_id(model)
    )
}

/// Native Gemini `generateContent` (more reliable than experimental OpenAI-compat for JSON).
async fn call_gemini_generate_content(
    client: &reqwest::Client,
    api_key: &str,
    model: &str,
    system: &str,
    messages: &[Value],
    json_mode: bool,
) -> Result<(String, Option<tuffbox_core::AiTokenUsage>), String> {
    if api_key.trim().is_empty() {
        return Err("Gemini API key is not set — paste it in Settings → AI".into());
    }
    if gemini_model_id(model).is_empty() {
        return Err("Gemini model is empty — set e.g. gemini-2.0-flash or gemini-flash-latest".into());
    }

    let mut contents: Vec<Value> = Vec::new();
    for m in messages {
        let role = m.get("role").and_then(Value::as_str).unwrap_or("user");
        let text = m
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if text.trim().is_empty() {
            continue;
        }
        if role == "system" {
            // Already passed via systemInstruction; skip duplicates in contents.
            continue;
        }
        let gemini_role = if role == "assistant" { "model" } else { "user" };
        // Gemini requires alternating user/model; merge consecutive same-role turns.
        if let Some(last) = contents.last_mut() {
            if last.get("role").and_then(Value::as_str) == Some(gemini_role) {
                if let Some(parts) = last.get_mut("parts").and_then(Value::as_array_mut) {
                    if let Some(part) = parts.first_mut() {
                        if let Some(existing) = part.get("text").and_then(Value::as_str) {
                            part["text"] = json!(format!("{existing}\n\n{text}"));
                            continue;
                        }
                    }
                }
            }
        }
        contents.push(json!({
            "role": gemini_role,
            "parts": [{ "text": text }]
        }));
    }
    if contents.is_empty() {
        return Err("Gemini request has no user messages".into());
    }
    // First turn must be user.
    if contents
        .first()
        .and_then(|c| c.get("role"))
        .and_then(Value::as_str)
        != Some("user")
    {
        contents.insert(
            0,
            json!({ "role": "user", "parts": [{ "text": "(continue)" }] }),
        );
    }

    let mut body = json!({ "contents": contents });
    if !system.trim().is_empty() {
        body["systemInstruction"] = json!({ "parts": [{ "text": system }] });
    }
    let mut gen = json!({ "temperature": 0.3 });
    if json_mode {
        // Mime-type only — full JSON Schema with null unions often 400s on Gemini.
        gen["responseMimeType"] = json!("application/json");
    }
    body["generationConfig"] = gen;

    let response = send_ai_http_with_retry("Gemini", || {
        client
            .post(gemini_generate_url(model))
            .header(USER_AGENT, APP_USER_AGENT)
            .header("x-goog-api-key", api_key.trim())
            .header(ACCEPT, "application/json")
            .json(&body)
    })
    .await?;
    let status = response.status();
    let body_text = response.text().await.map_err(|e| e.to_string())?;
    let parsed: Value = serde_json::from_str(&body_text).unwrap_or_else(|_| {
        json!({ "error": body_text.chars().take(500).collect::<String>() })
    });
    if !status.is_success() {
        return Err(api_error("Gemini", status, &parsed));
    }

    // Prefer candidates[0].content.parts[*].text
    if let Some(parts) = parsed
        .pointer("/candidates/0/content/parts")
        .and_then(Value::as_array)
    {
        let mut text = String::new();
        for part in parts {
            if let Some(t) = part.get("text").and_then(Value::as_str) {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(t);
            }
        }
        if !text.trim().is_empty() {
            return Ok((text, parse_gemini_usage(&parsed)));
        }
    }
    Err(format!(
        "Gemini response missing candidates[0].content.parts[].text: {}",
        body_text.chars().take(240).collect::<String>()
    ))
}

fn openai_chat_url(endpoint: &str) -> String {
    let endpoint = endpoint.trim_end_matches('/');
    if endpoint.ends_with("/chat/completions") {
        endpoint.to_string()
    } else if endpoint.ends_with("/v1")
        // Gemini OpenAI-compat base: …/v1beta/openai (+ optional trailing slash trimmed)
        || endpoint.ends_with("/openai")
    {
        format!("{endpoint}/chat/completions")
    } else {
        format!("{endpoint}/v1/chat/completions")
    }
}

fn ollama_chat_url(endpoint: &str) -> String {
    let endpoint = endpoint.trim_end_matches('/');
    if endpoint.ends_with("/api/chat") {
        endpoint.to_string()
    } else {
        format!("{endpoint}/api/chat")
    }
}

fn ollama_root(endpoint: &str) -> String {
    endpoint
        .trim()
        .trim_end_matches('/')
        .trim_end_matches("/v1")
        .trim_end_matches("/api/chat")
        .trim_end_matches("/api/tags")
        .trim_end_matches("/api/pull")
        .to_string()
}

fn model_name_matches(installed: &str, wanted: &str) -> bool {
    let a = installed.trim().to_lowercase();
    let b = wanted.trim().to_lowercase();
    if a == b {
        return true;
    }
    // `llama3.2:3b` matches `llama3.2:3b-instruct-q4_K_M` or tags without digest
    let a_base = a.split(':').next().unwrap_or(&a);
    let b_base = b.split(':').next().unwrap_or(&b);
    if a_base == b_base {
        // Same family: accept if tags equal or one is latest / missing tag
        let a_tag = a.split_once(':').map(|(_, t)| t).unwrap_or("latest");
        let b_tag = b.split_once(':').map(|(_, t)| t).unwrap_or("latest");
        return a_tag == b_tag
            || a_tag.starts_with(b_tag)
            || b_tag.starts_with(a_tag)
            || a_tag == "latest"
            || b_tag == "latest";
    }
    false
}

async fn ollama_list_models(root: &str) -> Result<Vec<OllamaModelInfo>, String> {
    ollama_list_models_with_timeout(root, 8).await
}

/// Same probe with a caller-chosen timeout. Diagnose/AI warm-up uses short
/// probes: a live daemon answers /api/tags in milliseconds, so a long hang
/// means the endpoint is dead and retrying with 8s timeouts only froze the
/// AI path for tens of seconds before surfacing the real error.
async fn ollama_list_models_with_timeout(
    root: &str,
    timeout_secs: u64,
) -> Result<Vec<OllamaModelInfo>, String> {
    let url = format!("{root}/api/tags");
    let response = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build()
        .map_err(|e| e.to_string())?
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Cannot reach Ollama at {url}: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("Ollama /api/tags failed ({})", response.status()));
    }
    let body: Value = response.json().await.map_err(|e| e.to_string())?;
    let mut models = Vec::new();
    if let Some(arr) = body.get("models").and_then(Value::as_array) {
        for m in arr {
            let Some(name) = m.get("name").and_then(Value::as_str) else {
                continue;
            };
            let size_bytes = m.get("size").and_then(Value::as_u64).unwrap_or(0);
            let details = m.get("details");
            let parameter_size = details
                .and_then(|d| d.get("parameter_size"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let quantization = details
                .and_then(|d| d.get("quantization_level"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let family = details
                .and_then(|d| d.get("family"))
                .and_then(Value::as_str)
                .or_else(|| {
                    details
                        .and_then(|d| d.get("families"))
                        .and_then(Value::as_array)
                        .and_then(|a| a.first())
                        .and_then(Value::as_str)
                })
                .unwrap_or("")
                .to_string();
            let fit = estimate_fit(size_bytes, &parameter_size, &quantization, name);
            models.push(OllamaModelInfo {
                name: name.to_string(),
                size_bytes,
                parameter_size,
                quantization,
                family,
                fit,
            });
        }
    }
    models.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(models)
}

fn try_start_ollama(binary_hint: &str, models_path: &str) {
    try_start_ollama_on_host(binary_hint, models_path, None);
}

fn try_start_ollama_on_host(binary_hint: &str, models_path: &str, host: Option<&str>) {
    let exe = resolve_ollama_binary(binary_hint);
    let models = models_path.trim();
    if !models.is_empty() {
        let _ = fs::create_dir_all(models);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut cmd = std::process::Command::new(&exe);
        cmd.arg("serve")
            .creation_flags(0x08000000) // CREATE_NO_WINDOW
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        if !models.is_empty() {
            cmd.env("OLLAMA_MODELS", models);
        }
        if let Some(h) = host.filter(|s| !s.trim().is_empty()) {
            cmd.env("OLLAMA_HOST", h);
        }
        let _ = cmd.spawn();
    }
    #[cfg(not(windows))]
    {
        let mut cmd = std::process::Command::new(&exe);
        cmd.arg("serve")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        if !models.is_empty() {
            cmd.env("OLLAMA_MODELS", models);
        }
        if let Some(h) = host.filter(|s| !s.trim().is_empty()) {
            cmd.env("OLLAMA_HOST", h);
        }
        let _ = cmd.spawn();
    }
}

/// Stop existing Ollama processes so a restart can honor `OLLAMA_MODELS`.
/// Without this, pulls hit an already-running daemon that still uses C:\…\.ollama.
fn stop_ollama_processes() {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Broad kill: tray (`ollama app`), CLI/server (`ollama`), capitalized variants.
        for image in ["ollama.exe", "ollama app.exe", "Ollama.exe"] {
            let _ = std::process::Command::new("taskkill")
                .args(["/IM", image, "/F", "/T"])
                .creation_flags(0x08000000)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
        let ps = r#"
Get-Process -ErrorAction SilentlyContinue |
  Where-Object { $_.ProcessName -match 'ollama' } |
  Stop-Process -Force -ErrorAction SilentlyContinue
Get-CimInstance Win32_Process -ErrorAction SilentlyContinue |
  Where-Object {
    $_.Name -match 'ollama' -or
    ($_.CommandLine -and $_.CommandLine -match '(?i)ollama')
  } |
  ForEach-Object {
    Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue
  }
"#;
        let _ = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", ps])
            .creation_flags(0x08000000)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("pkill")
            .args(["-f", "ollama serve"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        let _ = std::process::Command::new("pkill")
            .args(["-x", "ollama"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

/// Rough inventory of Ollama storage under a models dir (blobs + manifests).
fn ollama_storage_stats(models_path: &Path) -> (u64, u64) {
    fn walk_sum(dir: &Path, files: &mut u64, bytes: &mut u64) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if meta.is_dir() {
                walk_sum(&path, files, bytes);
            } else if meta.is_file() {
                *files += 1;
                *bytes = bytes.saturating_add(meta.len());
            }
        }
    }
    let mut files: u64 = 0;
    let mut bytes: u64 = 0;
    for sub in ["blobs", "manifests"] {
        walk_sum(&models_path.join(sub), &mut files, &mut bytes);
    }
    (files, bytes)
}

fn default_ollama_home_models() -> PathBuf {
    #[cfg(windows)]
    {
        if let Ok(home) = std::env::var("USERPROFILE") {
            return PathBuf::from(home).join(".ollama").join("models");
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".ollama").join("models");
    }
    PathBuf::from(".ollama").join("models")
}

/// After pull: ensure weights actually landed under the configured folder
/// (daemon env is what matters — CLI `OLLAMA_MODELS` alone does not).
fn verify_pull_landed_in_path(
    models_path: &str,
    before: (u64, u64),
    model: &str,
) -> Result<(), String> {
    let dir = PathBuf::from(models_path.trim());
    let after = ollama_storage_stats(&dir);
    if after.0 > before.0 || after.1 > before.1 {
        return Ok(());
    }
    // Manifests may already exist for a re-pull; accept if model tag folder is present.
    let slug = model.trim().to_lowercase();
    let (name, tag) = match slug.split_once(':') {
        Some((n, t)) => (n.to_string(), t.to_string()),
        None => (slug.clone(), "latest".into()),
    };
    let manifest_candidates = [
        dir.join("manifests")
            .join("registry.ollama.ai")
            .join("library")
            .join(&name)
            .join(&tag),
        dir.join("manifests").join("library").join(&name).join(&tag),
    ];
    if manifest_candidates.iter().any(|p| p.is_file()) {
        return Ok(());
    }

    let default_dir = default_ollama_home_models();
    let default_stats = ollama_storage_stats(&default_dir);
    let mut hint = format!(
        "Pull reported success but nothing new appeared under '{models_path}'. \
         Ollama stores models via the *daemon* (`OLLAMA_MODELS`); the tray app may still be writing to the default folder."
    );
    if default_dir != dir && (default_stats.0 > 0 || default_stats.1 > 0) {
        hint.push_str(&format!(
            " Default location still has data: {}.",
            default_dir.display()
        ));
    }
    hint.push_str(
        " Fully Quit Ollama from the system tray (right-click → Quit), then click Install model again.",
    );
    Err(hint)
}

async fn wait_ollama_api(root: &str, attempts: u32) -> Result<(), String> {
    let mut last_err = String::new();
    for attempt in 0..attempts {
        let delay_ms = 400u64 + u64::from(attempt) * 200;
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        // Short probe: a starting daemon answers quickly; a black-holing
        // endpoint must not consume the full 8s timeout per attempt.
        match ollama_list_models_with_timeout(root, 2).await {
            Ok(_) => return Ok(()),
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

/// Start a managed daemon on `MANAGED_OLLAMA_HOST` with `OLLAMA_MODELS` set.
/// Prefer leaving the tray on :11434 alone during a long download; if the managed
/// instance cannot bind (single-instance Ollama), fall back to a full bounce.
async fn ensure_managed_pull_daemon(settings: &AiSettings) -> Result<String, String> {
    let models = settings.ollama_models_path.trim();
    if models.is_empty() {
        return Err("models path is empty".into());
    }
    let _ = fs::create_dir_all(models);
    persist_ollama_models_user_env(models);

    let root = managed_ollama_root();
    let binary = settings.ollama_binary_path.as_str();

    if ollama_list_models(&root).await.is_ok() {
        stop_ollama_processes();
        tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    }
    try_start_ollama_on_host(binary, models, Some(MANAGED_OLLAMA_HOST));

    let mut last_err = String::new();
    for attempt in 0..12 {
        tokio::time::sleep(std::time::Duration::from_millis(450 + attempt * 200)).await;
        match ollama_list_models(&root).await {
            Ok(_) => return Ok(root),
            Err(e) => last_err = e,
        }
        if attempt == 2 || attempt == 6 || attempt == 9 {
            // Single-instance installs often refuse a second serve — kill tray and retry.
            stop_ollama_processes();
            tokio::time::sleep(std::time::Duration::from_millis(900)).await;
            try_start_ollama_on_host(binary, models, Some(MANAGED_OLLAMA_HOST));
        }
    }
    Err(format!(
        "Could not start a managed Ollama on {MANAGED_OLLAMA_HOST} with models path '{models}'. {last_err}. \
         Fully Quit Ollama from the tray and try Install again."
    ))
}

/// After a custom-path pull, restart the user's usual endpoint daemon with `OLLAMA_MODELS`
/// so chat/detect on :11434 see the same library.
async fn relaunch_user_endpoint_daemon(settings: &AiSettings) -> Result<String, String> {
    let root = ollama_root(&settings.endpoint);
    let models = settings.ollama_models_path.trim();
    persist_ollama_models_user_env(models);
    stop_ollama_processes();
    tokio::time::sleep(std::time::Duration::from_millis(900)).await;
    try_start_ollama(&settings.ollama_binary_path, models);
    match wait_ollama_api(&root, 6).await {
        Ok(()) => Ok(root),
        Err(e) => {
            // Soft failure: models are on disk; user can open Ollama tray (now with User env).
            Err(format!(
                "Models were saved under '{models}', but Ollama did not come back on {root}: {e}. \
                 Open the Ollama app once (it should pick up OLLAMA_MODELS), then Re-detect."
            ))
        }
    }
}

/// Ensure a daemon is running for the user's configured endpoint.
/// Custom models path: prefer starting with `OLLAMA_MODELS` if the API is down.
/// (Pulls use `ensure_managed_pull_daemon` so they do not race the tray on :11434.)
async fn ensure_ollama_daemon(settings: &AiSettings) -> Result<String, String> {
    let root = ollama_root(&settings.endpoint);
    let models = settings.ollama_models_path.trim();
    if models.is_empty() {
        if ollama_list_models_with_timeout(&root, 2).await.is_err() {
            try_start_ollama(&settings.ollama_binary_path, "");
            tokio::time::sleep(std::time::Duration::from_millis(900)).await;
        }
        return Ok(root);
    }

    let _ = fs::create_dir_all(models);
    persist_ollama_models_user_env(models);
    if ollama_list_models_with_timeout(&root, 2).await.is_ok() {
        return Ok(root);
    }
    try_start_ollama(&settings.ollama_binary_path, models);
    let _ = wait_ollama_api(&root, 5).await;
    Ok(root)
}

async fn ollama_pull_model_cli(
    binary_hint: &str,
    models_path: &str,
    model: &str,
    host: Option<&str>,
) -> Result<(), String> {
    if OLLAMA_PULL_CANCEL.load(Ordering::SeqCst) {
        return Err(OLLAMA_PULL_PAUSED_MSG.into());
    }
    let exe = resolve_ollama_binary(binary_hint);
    let mut cmd = tokio::process::Command::new(&exe);
    cmd.arg("pull").arg(model);
    let models = models_path.trim();
    if !models.is_empty() {
        let _ = fs::create_dir_all(models);
        cmd.env("OLLAMA_MODELS", models);
    }
    if let Some(h) = host.filter(|s| !s.trim().is_empty()) {
        cmd.env("OLLAMA_HOST", h);
    }
    #[cfg(windows)]
    {
        // tokio::process::Command has an inherent `creation_flags` on Windows
        // (no std CommandExt trait import needed).
        cmd.creation_flags(0x08000000);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to run ollama pull: {e}"))?;
    loop {
        match tokio::time::timeout(std::time::Duration::from_millis(250), child.wait()).await {
            Ok(Ok(status)) => {
                if status.success() {
                    return Ok(());
                }
                return Err(format!(
                    "ollama pull failed with exit code {}",
                    status.code().unwrap_or(-1)
                ));
            }
            Ok(Err(e)) => {
                return Err(format!("ollama pull process error: {e}"));
            }
            Err(_elapsed) => {
                if OLLAMA_PULL_CANCEL.load(Ordering::SeqCst) {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    return Err(OLLAMA_PULL_PAUSED_MSG.into());
                }
            }
        }
    }
}

/// Default Ollama models directory when `OLLAMA_MODELS` is unset.
pub fn default_ollama_models_dir() -> PathBuf {
    if let Ok(custom) = std::env::var("OLLAMA_MODELS") {
        let p = PathBuf::from(custom.trim());
        if !p.as_os_str().is_empty() {
            return p;
        }
    }
    #[cfg(windows)]
    {
        if let Ok(home) = std::env::var("USERPROFILE") {
            return PathBuf::from(home).join(".ollama").join("models");
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".ollama").join("models");
    }
    PathBuf::from(".ollama").join("models")
}

/// Resolve a user-configured Ollama path to an executable.
/// Accepts empty (PATH), a file path, or an install directory.
pub fn resolve_ollama_binary(hint: &str) -> PathBuf {
    let trimmed = hint.trim();
    if trimmed.is_empty() {
        return default_ollama_binary_candidates()
            .into_iter()
            .find(|p| p.is_file())
            .unwrap_or_else(|| PathBuf::from("ollama"));
    }
    let path = PathBuf::from(trimmed);
    if path.is_file() {
        return path;
    }
    if path.is_dir() {
        #[cfg(windows)]
        let candidates = ["ollama.exe", "ollama"];
        #[cfg(not(windows))]
        let candidates = ["ollama", "bin/ollama"];
        for name in candidates {
            let candidate = path.join(name);
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    // Path might not exist yet / be a custom name — still try it as-is.
    path
}

fn default_ollama_binary_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    #[cfg(windows)]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            out.push(PathBuf::from(local).join("Programs").join("Ollama").join("ollama.exe"));
        }
        if let Ok(pf) = std::env::var("ProgramFiles") {
            out.push(PathBuf::from(pf).join("Ollama").join("ollama.exe"));
        }
        out.push(PathBuf::from(r"C:\Program Files\Ollama\ollama.exe"));
    }
    #[cfg(target_os = "macos")]
    {
        out.push(PathBuf::from("/usr/local/bin/ollama"));
        out.push(PathBuf::from("/opt/homebrew/bin/ollama"));
        out.push(PathBuf::from("/Applications/Ollama.app/Contents/Resources/ollama"));
    }
    #[cfg(target_os = "linux")]
    {
        out.push(PathBuf::from("/usr/local/bin/ollama"));
        out.push(PathBuf::from("/usr/bin/ollama"));
        if let Ok(home) = std::env::var("HOME") {
            out.push(PathBuf::from(home).join(".local/bin/ollama"));
        }
    }
    out
}

async fn ollama_pull_model(
    app: Option<&AppHandle>,
    task_id: Option<&str>,
    root: &str,
    model: &str,
) -> Result<(), String> {
    if OLLAMA_PULL_CANCEL.load(Ordering::SeqCst) {
        emit_ollama_pull_paused(app, task_id, model, 0, 0);
        return Err(OLLAMA_PULL_PAUSED_MSG.into());
    }
    let url = format!("{root}/api/pull");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60 * 45))
        .build()
        .map_err(|e| e.to_string())?;
    let mut response = client
        .post(&url)
        .json(&json!({ "name": model, "stream": true }))
        .send()
        .await
        .map_err(|e| format!("Ollama pull failed for '{model}': {e}"))?;
    let status = response.status();
    if !status.is_success() {
        let body: Value = response
            .json()
            .await
            .unwrap_or_else(|_| json!({ "error": "empty response" }));
        return Err(api_error("Ollama pull", status, &body));
    }

    let mut buf = String::new();
    let mut last_emit = std::time::Instant::now()
        .checked_sub(std::time::Duration::from_secs(1))
        .unwrap_or_else(std::time::Instant::now);
    let mut last_completed: u64 = 0;
    let mut last_total: u64 = 0;
    loop {
        if OLLAMA_PULL_CANCEL.load(Ordering::SeqCst) {
            // Returning drops `response` and aborts the HTTP pull; Ollama keeps blobs.
            emit_ollama_pull_paused(app, task_id, model, last_completed, last_total);
            return Err(OLLAMA_PULL_PAUSED_MSG.into());
        }
        let chunk = response
            .chunk()
            .await
            .map_err(|e| format!("Ollama pull stream error: {e}"))?;
        let Some(bytes) = chunk else {
            break;
        };
        buf.push_str(&String::from_utf8_lossy(&bytes));
        while let Some(idx) = buf.find('\n') {
            let line = buf[..idx].trim().to_string();
            buf = buf[idx + 1..].to_string();
            if line.is_empty() {
                continue;
            }
            let Ok(evt) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if let Some(err) = evt.get("error").and_then(Value::as_str) {
                if !err.is_empty() {
                    return Err(format!("Ollama pull error: {err}"));
                }
            }
            let status_s = evt
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let completed = evt.get("completed").and_then(Value::as_u64).unwrap_or(0);
            let total = evt.get("total").and_then(Value::as_u64).unwrap_or(0);
            if completed > 0 || total > 0 {
                last_completed = completed;
                last_total = total;
            }
            let now = std::time::Instant::now();
            let done = status_s.eq_ignore_ascii_case("success")
                || (total > 0 && completed >= total && completed > 0);
            if done || now.duration_since(last_emit).as_millis() >= 250 {
                last_emit = now;
                emit_ollama_pull_progress(
                    app,
                    task_id,
                    model,
                    &status_s,
                    completed,
                    total,
                );
            }
            if OLLAMA_PULL_CANCEL.load(Ordering::SeqCst) {
                emit_ollama_pull_paused(app, task_id, model, last_completed, last_total);
                return Err(OLLAMA_PULL_PAUSED_MSG.into());
            }
        }
    }
    emit_ollama_pull_progress(app, task_id, model, "success", 1, 1);
    Ok(())
}

/// Ensure Ollama is reachable and the configured model is already installed.
/// Does **not** auto-pull — the user must pick/install a model in Settings.
/// Returns the model name that should be used for the next request.
pub async fn ensure_ollama_ready(settings: &AiSettings) -> Result<String, String> {
    let root = ollama_root(&settings.endpoint);
    if root.is_empty() {
        return Err("Ollama endpoint is empty".into());
    }

    let mut last_err = String::new();
    let mut models = None;
    // Prefer daemon with configured models dir so later ops don't hit C:\ default.
    if !settings.ollama_models_path.trim().is_empty() {
        let _ = ensure_ollama_daemon(settings).await;
    }
    // Fail-fast budget: at most two short probes. A live daemon answers in
    // milliseconds; the old 4×8s retry storm stalled every Diagnose → AI
    // run for ~35s whenever the endpoint was hanging (sick daemon, firewall
    // drop), and the process spawn below cannot help a remote endpoint.
    let mut started = false;
    for attempt in 0..2 {
        match ollama_list_models_with_timeout(&root, 3).await {
            Ok(list) => {
                models = Some(list);
                break;
            }
            Err(e) => {
                last_err = e;
                if !started {
                    started = true;
                    if ollama_binary_exists(&settings.ollama_binary_path) {
                        try_start_ollama(&settings.ollama_binary_path, &settings.ollama_models_path);
                    }
                }
                if attempt == 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
                }
            }
        }
    }
    let installed = models.ok_or_else(|| {
        let hint_path = settings.ollama_binary_path.trim();
        let resolved = resolve_ollama_binary(hint_path);
        if !ollama_binary_exists(hint_path) {
            format!(
                "{last_err}. Ollama is not installed (or not found). Install from https://ollama.com, then set the path in Settings → AI."
            )
        } else if hint_path.is_empty() {
            format!(
                "{last_err}. Ollama was found at {} but is not responding. Open the Ollama app once, or set the binary path in Settings → AI.",
                resolved.display()
            )
        } else {
            format!(
                "{last_err}. Could not start Ollama from '{}'. Check Settings → AI → Ollama path (resolved to {}).",
                hint_path,
                resolved.display()
            )
        }
    })?;

    let wanted_raw = settings.model.trim();
    if wanted_raw.is_empty() {
        if let Some(first) = installed.first() {
            return Ok(first.name.clone());
        }
        return Err(
            "Ollama is running but no model is installed. Open Settings → AI, enter a model name (or pick a .gguf file), and click Install model."
                .into(),
        );
    }

    let has = |list: &[OllamaModelInfo], name: &str| {
        list.iter().any(|m| model_name_matches(&m.name, name))
    };
    if has(&installed, wanted_raw) {
        return Ok(wanted_raw.to_string());
    }
    if let Some(local) = installed
        .iter()
        .find(|m| model_name_matches(&m.name, wanted_raw))
        .map(|m| m.name.clone())
    {
        return Ok(local);
    }

    let names = model_info_names(&installed);
    Err(format!(
        "Model '{wanted_raw}' is not installed in Ollama. Open Settings → AI, enter the model name you want (e.g. qwen2.5:7b), and click Install model. Installed: {}.",
        if names.is_empty() {
            "none".into()
        } else {
            names.join(", ")
        }
    ))
}

fn ollama_binary_exists(hint: &str) -> bool {
    let resolved = resolve_ollama_binary(hint);
    if resolved.is_file() {
        return true;
    }
    // PATH lookup: try `ollama --version`, but never wait on it indefinitely —
    // a hanging stub on PATH used to block the whole async executor here.
    let mut child = match std::process::Command::new(if hint.trim().is_empty() {
        "ollama"
    } else {
        resolved.to_str().unwrap_or("ollama")
    })
    .arg("--version")
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::null())
    .spawn()
    {
        Ok(child) => child,
        Err(_) => return false,
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(50)),
            Err(_) => return false,
        }
    }
}

/// Probe whether Ollama is installed / running and list local models.
#[tauri::command(rename_all = "camelCase")]
pub async fn detect_ollama(
    endpoint: Option<String>,
    binary_path: Option<String>,
) -> Result<serde_json::Value, String> {
    let settings = read_settings();
    let hint = binary_path
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| settings.ai.ollama_binary_path.clone());
    let models_dir = {
        let custom = settings.ai.ollama_models_path.trim();
        if custom.is_empty() {
            default_ollama_models_dir()
        } else {
            PathBuf::from(custom)
        }
    };
    let resolved = resolve_ollama_binary(&hint);
    let installed_bin = ollama_binary_exists(&hint);
    let root = ollama_root(
        &endpoint
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| settings.ai.endpoint.clone()),
    );

    let mut running = false;
    let mut models = Vec::new();
    let mut api_error = None;
    match ollama_list_models(&root).await {
        Ok(list) => {
            running = true;
            models = list;
        }
        Err(e) => {
            api_error = Some(e);
            if installed_bin {
                try_start_ollama(&hint, &settings.ai.ollama_models_path);
                tokio::time::sleep(std::time::Duration::from_millis(900)).await;
                match ollama_list_models(&root).await {
                    Ok(list) => {
                        running = true;
                        models = list;
                        api_error = None;
                    }
                    Err(e2) => api_error = Some(e2),
                }
            }
        }
    }

    Ok(json!({
        "installed": installed_bin || running,
        "running": running,
        "binaryPath": if resolved.exists() { resolved.to_string_lossy().to_string() } else { String::new() },
        "modelsPath": models_dir.to_string_lossy().to_string(),
        "modelsPathConfigured": !settings.ai.ollama_models_path.trim().is_empty(),
        "defaultModel": DEFAULT_OLLAMA_MODEL,
        "endpoint": root,
        "models": models,
        "needsModel": running && models.is_empty(),
        "error": api_error,
        "suggestedModels": suggested_model_catalog(),
        "hostRamBytes": host_ram_bytes(),
    }))
}

/// Start an Ollama model pull in the background so the user can keep using the launcher.
/// Re-pulling the same tag after pause resumes from already-downloaded blobs.
/// Returns immediately with `{ started: true, model, taskId }`.
#[tauri::command(rename_all = "camelCase")]
pub async fn pull_ollama_model(
    app: AppHandle,
    model: String,
    endpoint: Option<String>,
    binary_path: Option<String>,
    models_path: Option<String>,
) -> Result<serde_json::Value, String> {
    let name = model.trim().to_string();
    if name.is_empty() {
        return Err("Enter a model name to install (e.g. qwen2.5:7b)".into());
    }
    // Reject obvious filesystem paths here — use import_ollama_gguf instead.
    if name.contains('\\') || name.contains('/') || name.ends_with(".gguf") {
        return Err(
            "That looks like a file path. Use “Import .gguf” for local model files, or enter an Ollama tag like qwen2.5:7b."
                .into(),
        );
    }

    if OLLAMA_PULL_IN_FLIGHT.swap(true, Ordering::SeqCst) {
        return Err(
            "A model download is already running in the background. Pause it or wait for it to finish."
                .into(),
        );
    }

    OLLAMA_PULL_CANCEL.store(false, Ordering::SeqCst);

    let settings = read_settings();
    let hint = binary_path.unwrap_or_else(|| settings.ai.ollama_binary_path.clone());
    let root_user = ollama_root(
        &endpoint
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| settings.ai.endpoint.clone()),
    );

    let mut ai = settings.ai.clone();
    if !root_user.is_empty() {
        ai.endpoint = root_user.clone();
    }
    if !hint.trim().is_empty() {
        ai.ollama_binary_path = hint.clone();
    }
    // Prefer path from the UI (may not be flushed to disk yet).
    if let Some(p) = models_path {
        ai.ollama_models_path = p;
    }
    let models_path = ai.ollama_models_path.trim().to_string();
    if !models_path.is_empty() {
        let _ = fs::create_dir_all(&models_path);
        // Persist into settings so later chat/detect use the same folder.
        let mut disk = read_settings();
        disk.ai.ollama_models_path = models_path.clone();
        let _ = write_settings(&disk);
    }

    let task_id = tuffbox_core::task_progress::start_task(
        ollama_pull_task_id(&name),
        format!("AI model · {name}"),
    );
    tuffbox_core::task_progress::set_progress(&task_id, 0.0, Some("Starting download…".into()));
    set_ollama_pull_snapshot(OllamaPullSnapshot {
        phase: "running".into(),
        model: name.clone(),
        completed: 0,
        total: 0,
        task_id: task_id.clone(),
        error: None,
        models_path: if models_path.is_empty() {
            None
        } else {
            Some(models_path.clone())
        },
    });
    emit_ollama_pull_progress(Some(&app), Some(&task_id), &name, "starting", 0, 0);

    let app_bg = app.clone();
    let name_bg = name.clone();
    let task_id_bg = task_id.clone();
    let hint_bg = hint.clone();
    let models_path_bg = models_path.clone();
    let ai_bg = ai.clone();

    tokio::spawn(async move {
        let result =
            run_ollama_model_pull(app_bg.clone(), &task_id_bg, &name_bg, ai_bg, hint_bg, models_path_bg)
                .await;
        OLLAMA_PULL_IN_FLIGHT.store(false, Ordering::SeqCst);
        match result {
            Ok(payload) => {
                let models_path_out = payload
                    .get("modelsPath")
                    .and_then(Value::as_str)
                    .map(|s| s.to_string());
                set_ollama_pull_snapshot(OllamaPullSnapshot {
                    phase: "succeeded".into(),
                    model: name_bg.clone(),
                    completed: 1,
                    total: 1,
                    task_id: task_id_bg.clone(),
                    error: None,
                    models_path: models_path_out,
                });
                tuffbox_core::task_progress::succeed(
                    &task_id_bg,
                    Some(format!("Installed {name_bg}")),
                );
                let _ = app_bg.emit(
                    OLLAMA_PULL_FINISHED_EVENT,
                    json!({
                        "ok": true,
                        "paused": false,
                        "model": name_bg,
                        "result": payload,
                    }),
                );
            }
            Err(err) if is_ollama_pull_paused(&err) => {
                let snap = OLLAMA_PULL_SNAPSHOT
                    .lock()
                    .map(|g| g.clone())
                    .unwrap_or_default();
                set_ollama_pull_snapshot(OllamaPullSnapshot {
                    phase: "paused".into(),
                    model: name_bg.clone(),
                    completed: snap.completed,
                    total: snap.total,
                    task_id: task_id_bg.clone(),
                    error: None,
                    models_path: snap.models_path,
                });
                // emit_ollama_pull_paused already marked the task paused when cancel hit.
                tuffbox_core::task_progress::pause(
                    &task_id_bg,
                    Some("Paused — resume anytime; download continues from here".into()),
                );
                let _ = app_bg.emit(
                    OLLAMA_PULL_FINISHED_EVENT,
                    json!({
                        "ok": false,
                        "paused": true,
                        "model": name_bg,
                        "completed": snap.completed,
                        "total": snap.total,
                    }),
                );
            }
            Err(err) => {
                set_ollama_pull_snapshot(OllamaPullSnapshot {
                    phase: "failed".into(),
                    model: name_bg.clone(),
                    completed: 0,
                    total: 0,
                    task_id: task_id_bg.clone(),
                    error: Some(err.clone()),
                    models_path: None,
                });
                tuffbox_core::task_progress::fail(&task_id_bg, err.clone());
                let _ = app_bg.emit(
                    OLLAMA_PULL_FINISHED_EVENT,
                    json!({
                        "ok": false,
                        "paused": false,
                        "model": name_bg,
                        "error": err,
                    }),
                );
            }
        }
    });

    Ok(json!({
        "started": true,
        "ok": true,
        "model": name,
        "taskId": task_id,
    }))
}

async fn run_ollama_model_pull(
    app: AppHandle,
    task_id: &str,
    name: &str,
    ai: AiSettings,
    hint: String,
    models_path: String,
) -> Result<serde_json::Value, String> {
    let before = if models_path.is_empty() {
        (0, 0)
    } else {
        ollama_storage_stats(Path::new(&models_path))
    };

    // Custom path: pull against a private managed daemon so the tray on :11434
    // cannot swallow the download into %USERPROFILE%\.ollama\models.
    let pull_root = if models_path.is_empty() {
        ensure_ollama_daemon(&ai).await?
    } else {
        ensure_managed_pull_daemon(&ai).await?
    };

    if OLLAMA_PULL_CANCEL.load(Ordering::SeqCst) {
        emit_ollama_pull_paused(Some(&app), Some(task_id), name, 0, 0);
        return Err(OLLAMA_PULL_PAUSED_MSG.into());
    }

    let pull_host = pull_root
        .trim_start_matches("http://")
        .trim_start_matches("https://");

    // Prefer HTTP pull against the daemon that has OLLAMA_MODELS; CLI is a backup.
    match ollama_pull_model(Some(&app), Some(task_id), &pull_root, name).await {
        Ok(()) => {}
        Err(api_err) if is_ollama_pull_paused(&api_err) => {
            return Err(api_err);
        }
        Err(api_err) => {
            if let Err(cli_err) =
                ollama_pull_model_cli(&hint, &models_path, name, Some(pull_host)).await
            {
                if is_ollama_pull_paused(&cli_err) {
                    emit_ollama_pull_paused(Some(&app), Some(task_id), name, 0, 0);
                    return Err(cli_err);
                }
                return Err(format!("{api_err} | CLI fallback: {cli_err}"));
            }
        }
    }

    if !models_path.is_empty() {
        verify_pull_landed_in_path(&models_path, before, name)?;
        // Bring :11434 (or user endpoint) up on the same models dir for chat.
        if let Err(relaunch_err) = relaunch_user_endpoint_daemon(&ai).await {
            // Soft: weights are verified on disk.
            eprintln!("[tuffbox] ollama relaunch after pull: {relaunch_err}");
        }
    }

    let list_root = ollama_root(&ai.endpoint);
    let models = match ollama_list_models(&list_root).await {
        Ok(m) => m,
        Err(_) => ollama_list_models(&pull_root).await.unwrap_or_default(),
    };

    // Persist as active model when pull succeeds.
    let mut next = read_settings();
    next.ai.provider = "ollama".into();
    next.ai.model = name.to_string();
    if !models_path.is_empty() {
        next.ai.ollama_models_path = models_path.clone();
    }
    if next.ai.endpoint.trim().is_empty() {
        next.ai.endpoint = list_root.clone();
    }
    let _ = write_settings(&next);

    Ok(json!({
        "ok": true,
        "model": name,
        "models": models,
        "modelsPath": if models_path.is_empty() {
            default_ollama_models_dir().to_string_lossy().to_string()
        } else {
            models_path
        },
    }))
}

/// Import a local GGUF (or similar) file into Ollama under a user-chosen name.
#[tauri::command(rename_all = "camelCase")]
pub async fn import_ollama_gguf(
    file_path: String,
    model_name: String,
    binary_path: Option<String>,
) -> Result<serde_json::Value, String> {
    let path = PathBuf::from(file_path.trim());
    if !path.is_file() {
        return Err(format!("Model file not found: {}", path.display()));
    }
    let mut name = model_name.trim().to_string();
    if name.is_empty() {
        name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("local-model")
            .to_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '-' })
            .collect();
    }
    if name.is_empty() {
        return Err("Enter a name for the imported model".into());
    }

    let settings = read_settings();
    let hint = binary_path.unwrap_or_else(|| settings.ai.ollama_binary_path.clone());
    let exe = resolve_ollama_binary(&hint);
    if !ollama_binary_exists(&hint) {
        return Err("Ollama is not installed. Install it first, then import the model.".into());
    }

    let abs = std::fs::canonicalize(&path).unwrap_or(path.clone());
    let from_line = format!("FROM {}", abs.display());
    let tmp = std::env::temp_dir().join(format!("tuffbox-modelfile-{name}"));
    fs::write(&tmp, format!("{from_line}\n")).map_err(|e| e.to_string())?;

    // Bounce onto configured models path before create (managed port when custom).
    let mut ai = settings.ai.clone();
    if !hint.trim().is_empty() {
        ai.ollama_binary_path = hint.clone();
    }
    let models_dir = ai.ollama_models_path.trim().to_string();
    let before = if models_dir.is_empty() {
        (0, 0)
    } else {
        ollama_storage_stats(Path::new(&models_dir))
    };
    let create_root = if models_dir.is_empty() {
        ensure_ollama_daemon(&ai).await?
    } else {
        ensure_managed_pull_daemon(&ai).await?
    };
    let create_host = create_root
        .trim_start_matches("http://")
        .trim_start_matches("https://");

    let mut cmd = tokio::process::Command::new(&exe);
    cmd.arg("create").arg(&name).arg("-f").arg(&tmp);
    if !models_dir.is_empty() {
        cmd.env("OLLAMA_MODELS", &models_dir);
    }
    cmd.env("OLLAMA_HOST", create_host);
    let output = cmd
        .output()
        .await
        .map_err(|e| format!("Failed to run ollama create: {e}"))?;
    let _ = fs::remove_file(&tmp);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(format!(
            "ollama create failed: {}",
            if stderr.trim().is_empty() {
                stdout.trim()
            } else {
                stderr.trim()
            }
        ));
    }

    if !models_dir.is_empty() {
        verify_pull_landed_in_path(&models_dir, before, &name)?;
        let _ = relaunch_user_endpoint_daemon(&ai).await;
    }

    let mut next = read_settings();
    next.ai.provider = "ollama".into();
    next.ai.model = name.clone();
    let _ = write_settings(&next);

    let root = ollama_root(&next.ai.endpoint);
    let models = ollama_list_models(&root).await.unwrap_or_default();
    Ok(json!({
        "ok": true,
        "model": name,
        "models": models,
        "modelsPath": if models_dir.is_empty() {
            default_ollama_models_dir().to_string_lossy().to_string()
        } else {
            models_dir
        },
    }))
}

/// Status helper for Settings / Diagnostics (does not pull).
#[tauri::command(rename_all = "camelCase")]
pub async fn ensure_ollama_model() -> Result<serde_json::Value, String> {
    let settings = read_settings();
    if settings.ai.provider != "ollama" {
        return Ok(json!({
            "ok": true,
            "provider": settings.ai.provider,
            "skipped": true,
        }));
    }
    let model = ensure_ollama_ready(&settings.ai).await?;
    Ok(json!({
        "ok": true,
        "provider": "ollama",
        "model": model,
        "endpoint": ollama_root(&read_settings().ai.endpoint),
    }))
}

pub async fn call_ai(settings: &AiSettings, prompt: &str) -> Result<Value, String> {
    call_ai_once(settings, prompt).await
}

/// Multi-turn chat with a custom system prompt (no crash ActionPlan baked in).
/// When `json_mode` is true, providers are asked for JSON and the content is parsed.
pub async fn call_ai_messages(
    settings: &AiSettings,
    system: &str,
    messages: &[Value],
    json_mode: bool,
) -> Result<Value, String> {
    Ok(call_ai_messages_with_usage(settings, system, messages, json_mode, None)
        .await?
        .0)
}

/// Like `call_ai_messages`, but Ollama can take a JSON Schema in `format` (Structured Outputs).
/// OpenAI-compat still uses `response_format: json_object`; callers must validate.
pub async fn call_ai_messages_with_schema(
    settings: &AiSettings,
    system: &str,
    messages: &[Value],
    json_mode: bool,
    json_schema: Option<Value>,
) -> Result<Value, String> {
    Ok(
        call_ai_messages_with_usage(settings, system, messages, json_mode, json_schema)
            .await?
            .0,
    )
}

fn usage_u32(v: &Value, key: &str) -> Option<u32> {
    v.get(key)
        .and_then(|x| x.as_u64().or_else(|| x.as_f64().map(|f| f as u64)))
        .map(|n| n as u32)
}

fn parse_openai_usage(body: &Value) -> Option<tuffbox_core::AiTokenUsage> {
    let u = body.get("usage")?;
    let prompt = usage_u32(u, "prompt_tokens");
    let completion = usage_u32(u, "completion_tokens");
    let total = usage_u32(u, "total_tokens").or_else(|| match (prompt, completion) {
        (Some(a), Some(b)) => Some(a.saturating_add(b)),
        _ => None,
    });
    if prompt.is_none() && completion.is_none() && total.is_none() {
        return None;
    }
    Some(tuffbox_core::AiTokenUsage {
        prompt_tokens: prompt,
        completion_tokens: completion,
        total_tokens: total,
    })
}

fn parse_ollama_usage(body: &Value) -> Option<tuffbox_core::AiTokenUsage> {
    let prompt = usage_u32(body, "prompt_eval_count");
    let completion = usage_u32(body, "eval_count");
    if prompt.is_none() && completion.is_none() {
        return None;
    }
    Some(tuffbox_core::AiTokenUsage {
        prompt_tokens: prompt,
        completion_tokens: completion,
        total_tokens: match (prompt, completion) {
            (Some(a), Some(b)) => Some(a.saturating_add(b)),
            _ => None,
        },
    })
}

fn parse_gemini_usage(body: &Value) -> Option<tuffbox_core::AiTokenUsage> {
    let u = body.get("usageMetadata")?;
    let prompt = usage_u32(u, "promptTokenCount");
    let completion = usage_u32(u, "candidatesTokenCount");
    let total = usage_u32(u, "totalTokenCount").or_else(|| match (prompt, completion) {
        (Some(a), Some(b)) => Some(a.saturating_add(b)),
        _ => None,
    });
    if prompt.is_none() && completion.is_none() && total.is_none() {
        return None;
    }
    Some(tuffbox_core::AiTokenUsage {
        prompt_tokens: prompt,
        completion_tokens: completion,
        total_tokens: total,
    })
}

const AI_HTTP_MAX_RETRIES: u32 = 3;
const AI_STREAM_MAX_BYTES: usize = 1024 * 1024;
/// Connect deadline for AI HTTP (stream + non-stream).
const AI_CONNECT_TIMEOUT_SECS: u64 = 30;
/// TOTAL deadline for one non-streaming AI call (funnel-level). Per-attempt
/// HTTP timeouts (180s) x retry backoff can otherwise stack to ~10 minutes
/// on a black-holing endpoint, and every AI feature built on call_ai_once /
/// call_ai_messages_with_usage (quest plans, pack curation, recommendations)
/// inherits that hang. 300s keeps room for slow local models producing large
/// JSON while turning "forever" into a bounded, clear error. The streaming
/// path intentionally keeps its per-chunk idle timeout instead (progress is
/// visible to the user while tokens flow).
const AI_CALL_TOTAL_TIMEOUT_SECS: u64 = 300;
/// Per-chunk idle timeout for streaming responses. Must NOT use a total
/// `.timeout()` — that kills healthy long Ollama generations mid-body and
/// surfaces as "error decoding response body".
const AI_STREAM_READ_TIMEOUT_SECS: u64 = 300;

fn ai_stream_http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(AI_CONNECT_TIMEOUT_SECS))
        .read_timeout(std::time::Duration::from_secs(AI_STREAM_READ_TIMEOUT_SECS))
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))
}

fn map_ai_stream_read_err(provider: &str, err: reqwest::Error) -> String {
    let msg = err.to_string();
    let timed_out = err.is_timeout()
        || msg.to_ascii_lowercase().contains("timed out")
        || msg.to_ascii_lowercase().contains("timeout");
    let body_decode = err.is_decode()
        || err.is_body()
        || msg.to_ascii_lowercase().contains("decoding response body");
    if timed_out {
        format!(
            "{provider} stream stalled (no tokens for {AI_STREAM_READ_TIMEOUT_SECS}s). \
             Large quest plans on slow local models need steady token flow — keep Ollama loaded and retry. ({msg})"
        )
    } else if body_decode {
        format!(
            "{provider} stream interrupted mid-response (connection closed while decoding the body). \
             Often caused by Ollama restarting, OOM, or a total HTTP timeout — retry the turn. ({msg})"
        )
    } else {
        format!("{provider} stream read failed: {msg}")
    }
}

/// Retry AI HTTP on transport errors, 429, and 5xx (async counterpart to core `http::fetch` backoff).
async fn send_ai_http_with_retry(
    label: &str,
    mut make_req: impl FnMut() -> reqwest::RequestBuilder,
) -> Result<reqwest::Response, String> {
    let mut last_err = format!("{label} request failed");
    for attempt in 0..=AI_HTTP_MAX_RETRIES {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(2u64.pow(attempt.min(4)))).await;
        }
        let response = match make_req().send().await {
            Ok(r) => r,
            Err(e) => {
                last_err = format!("{label} request failed: {e}");
                if attempt < AI_HTTP_MAX_RETRIES {
                    continue;
                }
                return Err(last_err);
            }
        };
        let status = response.status();
        if status.as_u16() == 429 || status.is_server_error() {
            if attempt < AI_HTTP_MAX_RETRIES {
                let delay = response
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.trim().parse::<u64>().ok())
                    .unwrap_or_else(|| (2u64.pow(attempt + 1)).min(60))
                    .min(60);
                let _ = response.bytes().await;
                tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                continue;
            }
        }
        return Ok(response);
    }
    Err(last_err)
}

fn append_stream_capped(full: &mut String, delta: &str) -> Result<(), String> {
    if full.len().saturating_add(delta.len()) > AI_STREAM_MAX_BYTES {
        return Err(format!(
            "AI stream exceeded {AI_STREAM_MAX_BYTES} bytes — aborting to avoid OOM"
        ));
    }
    full.push_str(delta);
    Ok(())
}

fn strip_ai_fences(content: &str) -> &str {
    let trimmed = content.trim();
    let without_open = if let Some(rest) = trimmed.strip_prefix("```") {
        let rest = rest.trim_start();
        let rest = rest
            .strip_prefix("json")
            .or_else(|| rest.strip_prefix("JSON"))
            .or_else(|| rest.strip_prefix("Json"))
            .unwrap_or(rest);
        rest.trim_start_matches(['\r', '\n']).trim_start()
    } else {
        trimmed
    };
    let s = without_open.trim_end();
    let without_close = if let Some(rest) = s
        .strip_suffix("```json")
        .or_else(|| s.strip_suffix("```JSON"))
        .or_else(|| s.strip_suffix("```Json"))
        .or_else(|| s.strip_suffix("```"))
    {
        rest.trim_end()
    } else {
        s
    };
    without_close.trim()
}

fn finalize_ai_content(
    content: String,
    json_mode: bool,
) -> Result<Value, String> {
    let trimmed = strip_ai_fences(&content);

    if json_mode {
        serde_json::from_str(trimmed)
            .or_else(|_| {
                // Only slice out first `{`…last `}` when the payload looks like prose wrapping JSON.
                // If it already starts with `{`, a failed full parse must not carve a corrupt substring.
                if !trimmed.starts_with('{') {
                    if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
                        if end > start {
                            return serde_json::from_str(&trimmed[start..=end]);
                        }
                    }
                }
                Err(serde_json::Error::io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "not an object",
                )))
            })
            .map_err(|e| format!("AI returned invalid JSON: {e}"))
    } else {
        Ok(json!({ "content": trimmed }))
    }
}

/// Like `call_ai_messages_with_schema`, also returning provider token usage when available.
pub async fn call_ai_messages_with_usage(
    settings: &AiSettings,
    system: &str,
    messages: &[Value],
    json_mode: bool,
    json_schema: Option<Value>,
) -> Result<(Value, Option<tuffbox_core::AiTokenUsage>), String> {
    tokio::time::timeout(
        std::time::Duration::from_secs(AI_CALL_TOTAL_TIMEOUT_SECS),
        call_ai_messages_with_usage_inner(settings, system, messages, json_mode, json_schema),
    )
    .await
    .map_err(|_| {
        format!(
            "AI call did not finish within {AI_CALL_TOTAL_TIMEOUT_SECS}s — the provider/endpoint \
             is too slow or stalled. Check Settings → AI and retry."
        )
    })?
}

async fn call_ai_messages_with_usage_inner(
    settings: &AiSettings,
    system: &str,
    messages: &[Value],
    json_mode: bool,
    json_schema: Option<Value>,
) -> Result<(Value, Option<tuffbox_core::AiTokenUsage>), String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))?;

    let mut api_messages = Vec::with_capacity(messages.len() + 1);
    api_messages.push(json!({"role": "system", "content": system}));
    for m in messages {
        api_messages.push(m.clone());
    }

    let (content, usage) = if settings.provider == "ollama" {
        let model = ensure_ollama_ready(settings).await?;
        let mut body = json!({
            "model": model,
            "stream": false,
            "messages": api_messages,
        });
        if json_mode {
            if let Some(schema) = json_schema {
                body["format"] = schema;
            } else {
                body["format"] = json!("json");
            }
        }
        let response = send_ai_http_with_retry("Ollama", || {
            client
                .post(ollama_chat_url(&settings.endpoint))
                .json(&body)
        })
        .await?;
        let status = response.status();
        let body_text = response.text().await.map_err(|e| e.to_string())?;
        let body: Value = serde_json::from_str(&body_text).unwrap_or_else(|_| {
            json!({ "error": body_text.chars().take(500).collect::<String>() })
        });
        if !status.is_success() {
            return Err(api_error("Ollama", status, &body));
        }
        let content = body
            .get("message")
            .and_then(|message| message.get("content"))
            .and_then(Value::as_str)
            .ok_or_else(|| "Ollama response did not contain message.content".to_string())?
            .to_string();
        (content, parse_ollama_usage(&body))
    } else if is_gemini_endpoint(&settings.endpoint)
        || settings.model.to_ascii_lowercase().contains("gemini")
    {
        let token = secret("ai").ok().unwrap_or_default();
        call_gemini_generate_content(
            &client,
            &token,
            &settings.model,
            system,
            messages,
            json_mode,
        )
        .await?
    } else {
        let token = secret("ai").ok();
        let mut payload = json!({
            "model": settings.model,
            "temperature": 0.3,
            "messages": api_messages,
        });
        if json_mode {
            payload["response_format"] = json!({"type": "json_object"});
        }
        let response = send_ai_http_with_retry("AI provider", || {
            let mut req = client
                .post(openai_chat_url(&settings.endpoint))
                .header(USER_AGENT, APP_USER_AGENT)
                .json(&payload);
            if let Some(token) = token.as_ref().filter(|t| !t.trim().is_empty()) {
                req = req.bearer_auth(token);
            }
            req
        })
        .await?;
        let status = response.status();
        let body_text = response.text().await.map_err(|e| e.to_string())?;
        let body: Value = serde_json::from_str(&body_text).unwrap_or_else(|_| {
            json!({ "error": body_text.chars().take(500).collect::<String>() })
        });
        if !status.is_success() {
            return Err(api_error("AI provider", status, &body));
        }
        let content = body
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("content"))
            .and_then(Value::as_str)
            .ok_or_else(|| "AI response did not contain choices[0].message.content".to_string())?
            .to_string();
        (content, parse_openai_usage(&body))
    };

    Ok((finalize_ai_content(content, json_mode)?, usage))
}

fn parse_openai_sse_delta(line: &str) -> Option<String> {
    let data = line.strip_prefix("data:")?.trim();
    if data.is_empty() || data == "[DONE]" {
        return None;
    }
    let v: Value = serde_json::from_str(data).ok()?;
    v.get("choices")
        .and_then(Value::as_array)
        .and_then(|c| c.first())
        .and_then(|c| c.get("delta"))
        .and_then(|d| d.get("content"))
        .and_then(Value::as_str)
        .map(|s| s.to_string())
}

fn parse_openai_sse_usage(line: &str) -> Option<tuffbox_core::AiTokenUsage> {
    let data = line.strip_prefix("data:")?.trim();
    if data.is_empty() || data == "[DONE]" {
        return None;
    }
    let v: Value = serde_json::from_str(data).ok()?;
    parse_openai_usage(&v)
}

fn parse_ollama_stream_delta(chunk_json: &str) -> Option<String> {
    let v: Value = serde_json::from_str(chunk_json).ok()?;
    v.get("message")
        .and_then(|m| m.get("content"))
        .and_then(Value::as_str)
        .map(|s| s.to_string())
}

/// Stream assistant tokens via `on_token`, then parse like `call_ai_messages`.
/// Gemini falls back to non-streaming (full response, one token emit).
/// Returns provider usage when the stream/provider reports it.
pub async fn call_ai_messages_stream<F>(
    settings: &AiSettings,
    system: &str,
    messages: &[Value],
    json_mode: bool,
    mut on_token: F,
) -> Result<(Value, Option<tuffbox_core::AiTokenUsage>), String>
where
    F: FnMut(&str),
{
    // Gemini streaming not wired — use blocking path and emit once.
    if is_gemini_endpoint(&settings.endpoint)
        || settings.model.to_ascii_lowercase().contains("gemini")
    {
        let (value, usage) =
            call_ai_messages_with_usage(settings, system, messages, json_mode, None).await?;
        let preview = if value.is_string() {
            value.as_str().unwrap_or("").to_string()
        } else {
            value.to_string()
        };
        if !preview.is_empty() {
            on_token(&preview);
        }
        return Ok((value, usage));
    }

    let client = ai_stream_http_client()?;

    let mut api_messages = Vec::with_capacity(messages.len() + 1);
    api_messages.push(json!({"role": "system", "content": system}));
    for m in messages {
        api_messages.push(m.clone());
    }

    let mut full = String::new();
    let mut usage: Option<tuffbox_core::AiTokenUsage> = None;

    if settings.provider == "ollama" {
        let model = ensure_ollama_ready(settings).await?;
        let mut body = json!({
            "model": model,
            "stream": true,
            "messages": api_messages,
        });
        if json_mode {
            body["format"] = json!("json");
        }
        let mut response = send_ai_http_with_retry("Ollama", || {
            client
                .post(ollama_chat_url(&settings.endpoint))
                .json(&body)
        })
        .await?;
        if !response.status().is_success() {
            let status = response.status();
            let body_text = response.text().await.unwrap_or_default();
            let body: Value = serde_json::from_str(&body_text).unwrap_or_else(|_| {
                json!({ "error": body_text.chars().take(500).collect::<String>() })
            });
            return Err(api_error("Ollama", status, &body));
        }
        let mut buf = String::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| map_ai_stream_read_err("Ollama", e))?
        {
            buf.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(pos) = buf.find('\n') {
                let line = buf[..pos].trim().to_string();
                buf = buf[pos + 1..].to_string();
                if line.is_empty() {
                    continue;
                }
                if let Ok(v) = serde_json::from_str::<Value>(&line) {
                    if v.get("done").and_then(Value::as_bool) == Some(true) {
                        if let Some(u) = parse_ollama_usage(&v) {
                            usage = Some(u);
                        }
                    }
                }
                if let Some(delta) = parse_ollama_stream_delta(&line) {
                    if !delta.is_empty() {
                        append_stream_capped(&mut full, &delta)?;
                        on_token(&delta);
                    }
                }
            }
        }
        if !buf.trim().is_empty() {
            let line = buf.trim();
            if let Ok(v) = serde_json::from_str::<Value>(line) {
                if v.get("done").and_then(Value::as_bool) == Some(true) {
                    if let Some(u) = parse_ollama_usage(&v) {
                        usage = Some(u);
                    }
                }
            }
            if let Some(delta) = parse_ollama_stream_delta(line) {
                if !delta.is_empty() {
                    append_stream_capped(&mut full, &delta)?;
                    on_token(&delta);
                }
            }
        }
    } else {
        let token = secret("ai").ok();
        let mut payload = json!({
            "model": settings.model,
            "temperature": 0.3,
            "stream": true,
            "stream_options": { "include_usage": true },
            "messages": api_messages,
        });
        if json_mode {
            payload["response_format"] = json!({"type": "json_object"});
        }
        let mut response = send_ai_http_with_retry("AI provider", || {
            let mut req = client
                .post(openai_chat_url(&settings.endpoint))
                .header(USER_AGENT, APP_USER_AGENT)
                .header(ACCEPT, "text/event-stream")
                .json(&payload);
            if let Some(token) = token.as_ref().filter(|t| !t.trim().is_empty()) {
                req = req.bearer_auth(token);
            }
            req
        })
        .await?;
        if !response.status().is_success() {
            let status = response.status();
            let body_text = response.text().await.unwrap_or_default();
            let body: Value = serde_json::from_str(&body_text).unwrap_or_else(|_| {
                json!({ "error": body_text.chars().take(500).collect::<String>() })
            });
            return Err(api_error("AI provider", status, &body));
        }
        let mut buf = String::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| map_ai_stream_read_err("AI provider", e))?
        {
            buf.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(pos) = buf.find('\n') {
                let line = buf[..pos].trim_end_matches('\r').to_string();
                buf = buf[pos + 1..].to_string();
                if let Some(u) = parse_openai_sse_usage(&line) {
                    usage = Some(u);
                }
                if let Some(delta) = parse_openai_sse_delta(&line) {
                    if !delta.is_empty() {
                        append_stream_capped(&mut full, &delta)?;
                        on_token(&delta);
                    }
                }
            }
        }
        for line in buf.lines() {
            let line = line.trim_end_matches('\r');
            if let Some(u) = parse_openai_sse_usage(line) {
                usage = Some(u);
            }
            if let Some(delta) = parse_openai_sse_delta(line) {
                if !delta.is_empty() {
                    append_stream_capped(&mut full, &delta)?;
                    on_token(&delta);
                }
            }
        }
    }

    Ok((finalize_ai_content(full, json_mode)?, usage))
}

/// Result of crash Explain, including optional draft→verify metadata.
#[derive(Debug, Clone, Default)]
pub struct CrashExplainDetailed {
    pub value: Value,
    pub speculative: crate::speculative::SpeculativeMeta,
}

/// Call AI and ensure the response parses as ActionPlan; one repair retry on failure.
pub async fn call_ai_crash_explain(settings: &AiSettings, prompt: &str) -> Result<Value, String> {
    Ok(call_ai_crash_explain_detailed(settings, prompt).await?.value)
}

/// Same as [`call_ai_crash_explain`], plus whether draft→verify ran.
pub async fn call_ai_crash_explain_detailed(
    settings: &AiSettings,
    prompt: &str,
) -> Result<CrashExplainDetailed, String> {
    if crate::speculative::should_run(settings) {
        match try_speculative_crash_explain(settings, prompt).await {
            Ok(detailed) => return Ok(detailed),
            Err(e) => {
                eprintln!(
                    "[ai] speculative draft→verify skipped ({e}); falling back to single-shot"
                );
            }
        }
    }
    let value = call_ai_crash_explain_single(settings, prompt).await?;
    Ok(CrashExplainDetailed {
        value,
        speculative: crate::speculative::SpeculativeMeta::default(),
    })
}

async fn try_speculative_crash_explain(
    settings: &AiSettings,
    prompt: &str,
) -> Result<CrashExplainDetailed, String> {
    let draft_model = crate::speculative::resolve_draft_model(settings);
    let mut draft_settings = settings.clone();
    draft_settings.model = draft_model.clone();

    let draft_prompt = crate::speculative::build_draft_prompt(prompt);
    let draft_value = call_ai_once(&draft_settings, &draft_prompt)
        .await
        .map_err(|e| format!("draft model `{draft_model}`: {e}"))?;
    let draft_raw = serde_json::to_string(&draft_value).unwrap_or_default();
    // Soft-check: prefer a parseable ActionPlan as draft; otherwise still pass text through.
    let draft_json = match tuffbox_core::action_plan::parse_action_plan(&draft_raw) {
        Ok(plan) => serde_json::to_string(&plan).unwrap_or(draft_raw),
        Err(_) => draft_raw,
    };

    let verify_prompt = crate::speculative::build_verify_prompt(prompt, &draft_json);
    let value = call_ai_crash_explain_single(settings, &verify_prompt).await?;
    Ok(CrashExplainDetailed {
        value,
        speculative: crate::speculative::SpeculativeMeta {
            used: true,
            draft_model: Some(draft_model),
        },
    })
}

async fn call_ai_crash_explain_single(
    settings: &AiSettings,
    prompt: &str,
) -> Result<Value, String> {
    match call_ai_once(settings, prompt).await {
        Ok(value) => {
            let raw = serde_json::to_string(&value).unwrap_or_default();
            match tuffbox_core::action_plan::parse_action_plan(&raw) {
                Ok(plan) => Ok(serde_json::to_value(plan).unwrap_or(value)),
                Err(_) => Ok(value),
            }
        }
        Err(first_err) => {
            if !first_err.to_lowercase().contains("invalid json")
                && !first_err.to_lowercase().contains("did not contain")
            {
                return Err(first_err);
            }
            let repair = format!(
                "{prompt}\n\nYour previous answer was invalid JSON ({first_err}).\n{}\nReturn ONLY the JSON object.",
                tuffbox_core::ai_explanation::CRASH_JSON_SCHEMA_HINT
            );
            let value = call_ai_once(settings, &repair).await?;
            let raw = serde_json::to_string(&value).unwrap_or_default();
            let plan = tuffbox_core::action_plan::parse_action_plan(&raw)
                .map_err(|e| format!("AI returned invalid JSON after retry: {e}"))?;
            Ok(serde_json::to_value(plan).map_err(|e| e.to_string())?)
        }
    }
}

async fn call_ai_once(settings: &AiSettings, prompt: &str) -> Result<Value, String> {
    tokio::time::timeout(
        std::time::Duration::from_secs(AI_CALL_TOTAL_TIMEOUT_SECS),
        call_ai_once_inner(settings, prompt),
    )
    .await
    .map_err(|_| {
        format!(
            "AI call did not finish within {AI_CALL_TOTAL_TIMEOUT_SECS}s — the provider/endpoint \
             is too slow or stalled. Check Settings → AI and retry."
        )
    })?
}

async fn call_ai_once_inner(settings: &AiSettings, prompt: &str) -> Result<Value, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))?;
    let content = if settings.provider == "ollama" {
        let model = ensure_ollama_ready(settings).await?;
        let body = json!({
            "model": model,
            "stream": false,
            "format": "json",
            "messages": [
                {"role": "system", "content": format!("{}\n\n{}", tuffbox_core::action_plan::ACTION_PLAN_SYSTEM_PROMPT, tuffbox_core::ai_explanation::CRASH_JSON_SCHEMA_HINT)},
                {"role": "user", "content": prompt}
            ]
        });
        let response = send_ai_http_with_retry("Ollama", || {
            client
                .post(ollama_chat_url(&settings.endpoint))
                .json(&body)
        })
        .await?;
        let status = response.status();
        let body_text = response.text().await.map_err(|e| e.to_string())?;
        let body: Value = serde_json::from_str(&body_text).unwrap_or_else(|_| {
            json!({ "error": body_text.chars().take(500).collect::<String>() })
        });
        if !status.is_success() {
            return Err(api_error("Ollama", status, &body));
        }
        body.get("message")
            .and_then(|message| message.get("content"))
            .and_then(Value::as_str)
            .ok_or_else(|| "Ollama response did not contain message.content".to_string())?
            .to_string()
    } else if is_gemini_endpoint(&settings.endpoint)
        || settings.model.to_ascii_lowercase().contains("gemini")
    {
        let token = secret("ai").ok().unwrap_or_default();
        let system = format!(
            "{}\n\n{}",
            tuffbox_core::action_plan::ACTION_PLAN_SYSTEM_PROMPT,
            tuffbox_core::ai_explanation::CRASH_JSON_SCHEMA_HINT
        );
        let msgs = vec![json!({"role": "user", "content": prompt})];
        call_gemini_generate_content(&client, &token, &settings.model, &system, &msgs, true)
            .await?
            .0
    } else {
        // OpenAI-compatible / Hermes-style: API key optional for local endpoints.
        let token = secret("ai").ok();
        let body = json!({
            "model": settings.model,
            "temperature": 0.2,
            "response_format": {"type": "json_object"},
            "messages": [
                {"role": "system", "content": format!("{}\n\n{}", tuffbox_core::action_plan::ACTION_PLAN_SYSTEM_PROMPT, tuffbox_core::ai_explanation::CRASH_JSON_SCHEMA_HINT)},
                {"role": "user", "content": prompt}
            ]
        });
        let response = send_ai_http_with_retry("AI provider", || {
            let mut req = client
                .post(openai_chat_url(&settings.endpoint))
                .header(USER_AGENT, APP_USER_AGENT)
                .json(&body);
            if let Some(token) = token.as_ref().filter(|t| !t.trim().is_empty()) {
                req = req.bearer_auth(token);
            }
            req
        })
        .await?;
        let status = response.status();
        let body_text = response.text().await.map_err(|e| e.to_string())?;
        let body: Value = serde_json::from_str(&body_text).unwrap_or_else(|_| {
            json!({ "error": body_text.chars().take(500).collect::<String>() })
        });
        if !status.is_success() {
            return Err(api_error("AI provider", status, &body));
        }
        body.get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("content"))
            .and_then(Value::as_str)
            .ok_or_else(|| "AI response did not contain choices[0].message.content".to_string())?
            .to_string()
    };
    let trimmed = strip_ai_fences(&content);
    // Prefer ActionPlan parse; fall back to raw JSON.
    match tuffbox_core::action_plan::parse_action_plan(trimmed) {
        Ok(plan) => serde_json::to_value(plan).map_err(|e| e.to_string()),
        Err(_) => match tuffbox_core::ai_explanation::parse_crash_response(trimmed) {
            Ok(parsed) => serde_json::to_value(parsed).map_err(|e| e.to_string()),
            Err(_) => {
                serde_json::from_str(trimmed).map_err(|e| format!("AI returned invalid JSON: {e}"))
            }
        },
    }
}

/// List local Ollama models via `/api/tags` (with size / quant / fit metadata).
#[tauri::command(rename_all = "camelCase")]
pub async fn list_ollama_models(endpoint: Option<String>) -> Result<Vec<OllamaModelInfo>, String> {
    let settings = read_settings();
    let base = endpoint
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| settings.ai.endpoint.clone());
    let root = ollama_root(&base);
    ollama_list_models(&root).await
}

/// Disk usage for the Ollama models folder + free space on that volume.
#[tauri::command(rename_all = "camelCase")]
pub async fn get_ollama_storage(models_path: Option<String>) -> Result<Value, String> {
    let settings = read_settings();
    let path = models_path
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            let custom = settings.ai.ollama_models_path.trim();
            if custom.is_empty() {
                default_ollama_models_dir().to_string_lossy().to_string()
            } else {
                custom.to_string()
            }
        });
    let dir = PathBuf::from(&path);
    let (files, used_bytes) = ollama_storage_stats(&dir);
    let (available_bytes, total_bytes) = disk_space_for_path(&dir);
    Ok(json!({
        "path": path,
        "files": files,
        "usedBytes": used_bytes,
        "availableBytes": available_bytes,
        "totalBytes": total_bytes,
        "hostRamBytes": host_ram_bytes(),
    }))
}

/// Delete an installed Ollama model by tag.
#[tauri::command(rename_all = "camelCase")]
pub async fn delete_ollama_model(
    model: String,
    endpoint: Option<String>,
    binary_path: Option<String>,
) -> Result<Value, String> {
    let name = model.trim().to_string();
    if name.is_empty() {
        return Err("Enter a model name to delete".into());
    }
    let settings = read_settings();
    let hint = binary_path.unwrap_or_else(|| settings.ai.ollama_binary_path.clone());
    let root = ollama_root(
        &endpoint
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| settings.ai.endpoint.clone()),
    );

    // Prefer HTTP delete; CLI fallback.
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?;
    let url = format!("{root}/api/delete");
    let api_result = client
        .post(&url)
        .json(&json!({ "name": name }))
        .send()
        .await;
    let api_ok = match api_result {
        Ok(resp) if resp.status().is_success() => true,
        Ok(resp) => {
            let status = resp.status();
            let body: Value = resp.json().await.unwrap_or_else(|_| json!({}));
            // Older Ollama used DELETE; try once more.
            let del = client
                .delete(&url)
                .json(&json!({ "name": name }))
                .send()
                .await;
            match del {
                Ok(r) if r.status().is_success() => true,
                _ => {
                    eprintln!(
                        "[tuffbox] ollama delete API failed ({status}): {body}"
                    );
                    false
                }
            }
        }
        Err(e) => {
            eprintln!("[tuffbox] ollama delete API error: {e}");
            false
        }
    };

    if !api_ok {
        let exe = resolve_ollama_binary(&hint);
        let mut cmd = tokio::process::Command::new(&exe);
        cmd.arg("rm").arg(&name);
        let models = settings.ai.ollama_models_path.trim();
        if !models.is_empty() {
            cmd.env("OLLAMA_MODELS", models);
        }
        #[cfg(windows)]
        {
            cmd.creation_flags(0x08000000);
        }
        let output = cmd
            .output()
            .await
            .map_err(|e| format!("Failed to run ollama rm: {e}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            return Err(format!(
                "Failed to delete model '{name}': {}",
                if stderr.trim().is_empty() {
                    stdout.trim()
                } else {
                    stderr.trim()
                }
            ));
        }
    }

    let models = ollama_list_models(&root).await.unwrap_or_default();
    let mut next = read_settings();
    if model_name_matches(&next.ai.model, &name) {
        next.ai.model = models
            .first()
            .map(|m| m.name.clone())
            .unwrap_or_default();
        let _ = write_settings(&next);
    }

    Ok(json!({
        "ok": true,
        "model": name,
        "models": models,
        "activeModel": next.ai.model,
    }))
}

fn is_ollama_models_dir(path: &Path) -> bool {
    path.join("blobs").is_dir() && path.join("manifests").is_dir()
}

fn should_skip_scan_dir(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    matches!(
        n.as_str(),
        "windows"
            | "winsxs"
            | "system volume information"
            | "$recycle.bin"
            | "recovery"
            | "windowsapps"
            | "node_modules"
            | ".git"
            | "target"
            | "cache"
            | "caches"
            | "__pycache__"
            | "temp"
            | "tmp"
            | "packages"
            | "nuget"
            | "i386"
            | "assembly"
    ) || n.starts_with('$')
}

/// Read installed model tags from an Ollama models folder (manifests tree).
pub fn list_models_from_ollama_dir(models_dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let root = models_dir.join("manifests").join("registry.ollama.ai");
    let Ok(ns_entries) = fs::read_dir(&root) else {
        return out;
    };
    for ns in ns_entries.flatten() {
        if !ns.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let ns_name = ns.file_name().to_string_lossy().to_string();
        let Ok(models) = fs::read_dir(ns.path()) else {
            continue;
        };
        for model_ent in models.flatten() {
            if !model_ent.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let model_name = model_ent.file_name().to_string_lossy().to_string();
            let Ok(tags) = fs::read_dir(model_ent.path()) else {
                continue;
            };
            for tag in tags.flatten() {
                if tag.file_type().map(|t| t.is_file()).unwrap_or(false) {
                    let tag_name = tag.file_name().to_string_lossy().to_string();
                    if ns_name == "library" {
                        out.push(format!("{model_name}:{tag_name}"));
                    } else {
                        out.push(format!("{ns_name}/{model_name}:{tag_name}"));
                    }
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

fn push_unique_path(out: &mut Vec<String>, path: PathBuf) {
    let s = path.to_string_lossy().to_string();
    if s.is_empty() || out.iter().any(|x| x.eq_ignore_ascii_case(&s)) {
        return;
    }
    out.push(s);
}

/// Scan a drive (default `C:\`) for `ollama.exe` and Ollama models folders.
/// Skips Windows system trees. Cap visits so it cannot hang forever.
///
/// # ponytail: BFS with skip-list + visit cap; full unrestricted C: crawl if misses persist
#[tauri::command(rename_all = "camelCase")]
pub async fn scan_ollama_disk(root: Option<String>) -> Result<Value, String> {
    tokio::task::spawn_blocking(move || {
        let root = root
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| r"C:\".into());
        let root_path = PathBuf::from(&root);
        if !root_path.exists() {
            return Err(format!("Drive/path not found: {root}"));
        }

        let mut binaries: Vec<String> = Vec::new();
        let mut models_dirs: Vec<String> = Vec::new();

        for p in default_ollama_binary_candidates() {
            if p.is_file() {
                push_unique_path(&mut binaries, p);
            }
        }
        if let Ok(custom) = std::env::var("OLLAMA_MODELS") {
            let p = PathBuf::from(custom.trim());
            if is_ollama_models_dir(&p) {
                push_unique_path(&mut models_dirs, p);
            }
        }
        let default_models = default_ollama_models_dir();
        if is_ollama_models_dir(&default_models) {
            push_unique_path(&mut models_dirs, default_models);
        }
        if let Ok(home) = std::env::var("USERPROFILE") {
            let home = PathBuf::from(home);
            for rel in [
                PathBuf::from(".ollama").join("models"),
                PathBuf::from("Ollama").join("models"),
                PathBuf::from("LLM").join("ollama").join("models"),
                PathBuf::from("Models").join("ollama"),
            ] {
                let p = home.join(rel);
                if is_ollama_models_dir(&p) {
                    push_unique_path(&mut models_dirs, p);
                }
            }
        }

        const MAX_VISITS: usize = 250_000;
        let mut visits = 0usize;
        let mut queue: std::collections::VecDeque<PathBuf> = std::collections::VecDeque::new();
        queue.push_back(root_path);
        while let Some(dir) = queue.pop_front() {
            if visits >= MAX_VISITS {
                break;
            }
            visits += 1;
            let Ok(rd) = fs::read_dir(&dir) else {
                continue;
            };
            for ent in rd.flatten() {
                let path = ent.path();
                let Ok(ft) = ent.file_type() else {
                    continue;
                };
                let name = ent.file_name().to_string_lossy().to_string();
                if ft.is_file() {
                    if name.eq_ignore_ascii_case("ollama.exe") || name == "ollama" {
                        push_unique_path(&mut binaries, path);
                    }
                    continue;
                }
                if !ft.is_dir() {
                    continue;
                }
                if should_skip_scan_dir(&name) {
                    continue;
                }
                if is_ollama_models_dir(&path) {
                    push_unique_path(&mut models_dirs, path.clone());
                }
                if name.eq_ignore_ascii_case("blobs") || name.eq_ignore_ascii_case("manifests") {
                    continue;
                }
                queue.push_back(path);
            }
        }

        binaries.sort();
        models_dirs.sort_by(|a, b| {
            let ca = list_models_from_ollama_dir(Path::new(a)).len();
            let cb = list_models_from_ollama_dir(Path::new(b)).len();
            cb.cmp(&ca).then_with(|| a.cmp(b))
        });

        let best_binary = binaries.first().cloned();
        let best_models = models_dirs.first().cloned();
        let mut disk_models = Vec::new();
        for p in &models_dirs {
            for m in list_models_from_ollama_dir(Path::new(p)) {
                if !disk_models.iter().any(|x| x == &m) {
                    disk_models.push(m);
                }
            }
        }
        disk_models.sort();
        disk_models.dedup();

        Ok(json!({
            "root": root,
            "visited": visits,
            "truncated": visits >= MAX_VISITS,
            "binaries": binaries,
            "modelsDirs": models_dirs,
            "bestBinary": best_binary,
            "bestModelsDir": best_models,
            "models": disk_models,
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_provider_urls() {
        assert_eq!(
            openai_chat_url("http://localhost:1234/v1"),
            "http://localhost:1234/v1/chat/completions"
        );
        assert_eq!(
            openai_chat_url("https://generativelanguage.googleapis.com/v1beta/openai"),
            "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions"
        );
        assert_eq!(
            openai_chat_url("https://generativelanguage.googleapis.com/v1beta/openai/"),
            "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions"
        );
        assert_eq!(
            ollama_chat_url("http://127.0.0.1:11434"),
            "http://127.0.0.1:11434/api/chat"
        );
        assert_eq!(
            gemini_generate_url("gemini-flash-latest"),
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-flash-latest:generateContent"
        );
        assert_eq!(gemini_model_id("models/gemini-2.0-flash"), "gemini-2.0-flash");
        assert!(is_gemini_endpoint(
            "https://generativelanguage.googleapis.com/v1beta/openai"
        ));
    }

    #[test]
    fn api_error_reads_nested_gemini_message() {
        let body = json!({
            "error": { "code": 400, "message": "Request contains an invalid argument.", "status": "INVALID_ARGUMENT" }
        });
        let msg = api_error("Gemini", reqwest::StatusCode::BAD_REQUEST, &body);
        assert!(msg.contains("invalid argument"), "{msg}");
    }

    #[test]
    fn api_error_401_hints_api_key() {
        let body = json!({ "error": { "message": "API key not valid" } });
        let msg = api_error("Gemini", reqwest::StatusCode::UNAUTHORIZED, &body);
        assert!(msg.contains("API key"), "{msg}");
        assert!(msg.contains("Settings"), "{msg}");
    }

    #[test]
    fn api_error_html_403_hints_vpn_not_api_key() {
        let html = "<!DOCTYPE html>\n<html lang=en>\n<title>Error 403 (Forbidden)!!1</title>";
        let body = json!({ "error": html });
        let msg = api_error("Gemini", reqwest::StatusCode::FORBIDDEN, &body);
        assert!(msg.contains("VPN") || msg.contains("egress"), "{msg}");
        assert!(
            !msg.contains("check the AI API key"),
            "must not blame API key for HTML edge 403: {msg}"
        );
    }

    #[test]
    fn normalizes_release_versions() {
        assert_eq!(
            normalized_semver("v1.2.3"),
            Some(semver::Version::new(1, 2, 3))
        );
    }

    #[test]
    fn skip_list_covers_windows() {
        assert!(should_skip_scan_dir("Windows"));
        assert!(should_skip_scan_dir("node_modules"));
        assert!(!should_skip_scan_dir("Users"));
    }

    #[test]
    fn detects_models_dir_shape() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_ollama_models_dir(dir.path()));
        fs::create_dir(dir.path().join("blobs")).unwrap();
        fs::create_dir(dir.path().join("manifests")).unwrap();
        assert!(is_ollama_models_dir(dir.path()));
    }

    #[test]
    fn strip_ai_fences_closing_language_tag() {
        assert_eq!(
            strip_ai_fences("```json\n{\"ok\":true}\n```json"),
            "{\"ok\":true}"
        );
        assert_eq!(
            strip_ai_fences("```JSON\n{\"ok\":1}\n```JSON"),
            "{\"ok\":1}"
        );
        assert_eq!(strip_ai_fences("```\n{}\n```"), "{}");
    }

    // ------------------------------------------------------------------
    // End-to-end local LLM error behaviour — a throwaway TCP server that
    // pretends to be an OpenAI-compatible endpoint, exercised through the
    // exact same code path the app uses (`call_ai_messages`).
    // ------------------------------------------------------------------

    fn spawn_mock_llm(responses: Vec<(u16, String)>) -> String {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").expect("mock bind");
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for (status, body) in responses {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                let mut buf = [0u8; 8192];
                let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(500)));
                let _ = stream.read(&mut buf);
                let reason = match status {
                    200 => "OK",
                    401 => "Unauthorized",
                    429 => "Too Many Requests",
                    500 => "Internal Server Error",
                    503 => "Service Unavailable",
                    _ => "Error",
                };
                let out = format!(
                    "HTTP/1.1 {status} {reason}\r\n\
                     Content-Type: application/json\r\n\
                     Content-Length: {}\r\n\
                     Connection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(out.as_bytes());
            }
        });
        format!("http://{addr}/v1")
    }

    fn mock_llm_settings(endpoint: String) -> AiSettings {
        let mut s = AiSettings::default();
        s.provider = "openai-compatible".into();
        s.endpoint = endpoint;
        s
    }

    fn run_llm_call(
        settings: &AiSettings,
        json_mode: bool,
    ) -> Result<Value, String> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(call_ai_messages(
            settings,
            "system",
            &[json!({"role": "user", "content": "hi"})],
            json_mode,
        ))
    }

    fn ok_body(content: &str) -> String {
        json!({"choices": [{"message": {"content": content}}]}).to_string()
    }

    #[test]
    fn llm_ok_returns_content() {
        let endpoint = spawn_mock_llm(vec![(200, ok_body("{\"ok\":true}"))]);
        let res = run_llm_call(&mock_llm_settings(endpoint), true).unwrap();
        assert_eq!(res["ok"], json!(true));
    }

    #[test]
    fn llm_200_but_no_choices_is_explicit() {
        let endpoint = spawn_mock_llm(vec![(200, json!({"foo": 1}).to_string())]);
        let err = run_llm_call(&mock_llm_settings(endpoint), true).unwrap_err();
        assert!(
            err.contains("did not contain choices[0].message.content"),
            "{err}"
        );
    }

    #[test]
    fn llm_content_not_json_reports_invalid_json() {
        let endpoint = spawn_mock_llm(vec![(200, ok_body("this is not json"))]);
        let err = run_llm_call(&mock_llm_settings(endpoint), true).unwrap_err();
        assert!(err.contains("AI returned invalid JSON"), "{err}");
    }

    #[test]
    fn llm_401_hints_api_key() {
        let body = json!({"error": {"message": "Invalid API key provided"}}).to_string();
        let endpoint = spawn_mock_llm(vec![(401, body)]);
        let err = run_llm_call(&mock_llm_settings(endpoint), true).unwrap_err();
        assert!(err.contains("401"), "{err}");
        assert!(err.contains("API key"), "{err}");
    }

    #[test]
    fn llm_malformed_body_becomes_dirty_json_error() {
        let endpoint = spawn_mock_llm(vec![(200, "<!DOCTYPE html><html>edge</html>".into())]);
        let err = run_llm_call(&mock_llm_settings(endpoint), false).unwrap_err();
        assert!(
            err.contains("did not contain choices[0].message.content"),
            "{err}"
        );
    }

    #[test]
    fn llm_503_reports_provider_error() {
        let endpoint = spawn_mock_llm(vec![
            (503, json!({"error":"overloaded"}).to_string()),
            (503, json!({"error":"overloaded"}).to_string()),
            (503, json!({"error":"overloaded"}).to_string()),
            (503, json!({"error":"overloaded"}).to_string()),
        ]);
        let err = run_llm_call(&mock_llm_settings(endpoint), true).unwrap_err();
        assert!(err.contains("503"), "{err}");
    }
}
