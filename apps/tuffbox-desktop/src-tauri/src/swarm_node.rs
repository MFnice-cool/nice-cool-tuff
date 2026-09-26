//! Phase C: spawn/attach `tuffswarm-node` and prefer its control HTTP over hub.

use once_cell::sync::Lazy;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;

use crate::integrations;

static NODE_CHILD: Lazy<Mutex<Option<Child>>> = Lazy::new(|| Mutex::new(None));
/// Bearer for the node we spawned or re-attached to (process memory + persisted file).
static CONTROL_TOKEN: Lazy<Mutex<Option<String>>> = Lazy::new(|| Mutex::new(None));
/// Fog volunteer poller started once for the process lifetime (loop self-gates on settings).
static VOLUNTEER_POLLER: Lazy<std::sync::atomic::AtomicBool> =
    Lazy::new(|| std::sync::atomic::AtomicBool::new(false));
/// Generation bump forces poller to notice volunteer/node restart.
static VOLUNTEER_GEN: Lazy<std::sync::atomic::AtomicU64> =
    Lazy::new(|| std::sync::atomic::AtomicU64::new(0));

/// Test/IPC hook: does a control token exist? (taurpc_api pilot uses this
/// instead of the private `control_token`.)
pub fn control_token_is_some() -> bool {
    control_token().is_some()
}

fn control_token() -> Option<String> {
    CONTROL_TOKEN.lock().ok().and_then(|g| g.clone())
}

fn set_control_token(token: String) {
    if let Ok(mut g) = CONTROL_TOKEN.lock() {
        *g = Some(token);
    }
}

fn clear_control_token() {
    if let Ok(mut g) = CONTROL_TOKEN.lock() {
        *g = None;
    }
}

fn is_p2p_control_base(base: &str) -> bool {
    let swarm = integrations::swarm_settings();
    let control = swarm.p2p_control_url.trim().trim_end_matches('/');
    base.trim().trim_end_matches('/') == control
}

/// Token for a transport base: P2P control uses spawn token; hub uses crash_kb secret.
pub fn auth_token_for_base(base: &str) -> Option<String> {
    if is_p2p_control_base(base) {
        control_token()
    } else {
        integrations::secret_optional("crash_kb")
    }
}

fn p2p_token_path() -> PathBuf {
    dirs::data_dir()
        .or_else(dirs::config_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("TuffBox")
        .join("swarm")
        .join("p2p_control_token")
}

fn load_persisted_token() -> Option<String> {
    let path = p2p_token_path();
    let raw = std::fs::read_to_string(path).ok()?;
    let token = raw.trim().to_string();
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

fn persist_token(token: &str) -> Result<(), String> {
    let path = p2p_token_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, token.as_bytes()).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn new_ephemeral_token() -> String {
    let mut hasher = Sha256::new();
    hasher.update(
        format!(
            "tuffswarm-{}-{}",
            std::process::id(),
            tuffbox_core::time_util::compact_now()
        )
        .as_bytes(),
    );
    hex::encode(hasher.finalize())
}

/// Ordered capsule HTTP bases: P2P control (if authorized) then hub/KB fallback.
pub async fn capsule_transport_bases() -> Vec<String> {
    let swarm = integrations::swarm_settings();
    let mut bases = Vec::new();
    if swarm.enabled && swarm.p2p_enabled {
        let control = swarm.p2p_control_url.trim().trim_end_matches('/').to_string();
        if !control.is_empty() {
            let _ = ensure_node_running(&control).await;
            if p2p_authorized(&control).await {
                bases.push(control);
            }
        }
    }
    if let Some(hub) = integrations::swarm_network_base() {
        if !bases.iter().any(|b| b == &hub) {
            bases.push(hub);
        }
    }
    bases
}

pub async fn p2p_healthy(control_base: &str) -> bool {
    let url = format!("{}/health", control_base.trim_end_matches('/'));
    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
    else {
        return false;
    };
    match client.get(&url).send().await {
        Ok(resp) if resp.status().is_success() => {
            let body: Value = resp.json().await.unwrap_or(Value::Null);
            body.get("ok").and_then(|v| v.as_bool()).unwrap_or(true)
        }
        _ => false,
    }
}

/// True only when we hold a bearer that the control plane accepts.
pub async fn p2p_authorized(control_base: &str) -> bool {
    let Some(token) = control_token() else {
        return false;
    };
    if !p2p_healthy(control_base).await {
        return false;
    }
    let url = format!("{}/v1/node/status", control_base.trim_end_matches('/'));
    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
    else {
        return false;
    };
    match client
        .get(&url)
        .bearer_auth(&token)
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => true,
        _ => false,
    }
}

pub async fn ensure_node_running(control_base: &str) -> Result<(), String> {
    // Restore persisted token before any attach decision (fail closed without ownership).
    if control_token().is_none() {
        if let Some(token) = load_persisted_token() {
            set_control_token(token);
        }
    }

    if p2p_authorized(control_base).await {
        return Ok(());
    }

    // Healthy listener we cannot authorize → refuse hijack attach.
    if p2p_healthy(control_base).await {
        clear_control_token();
        return Err(
            "p2p control is listening but our control token was rejected — refuse attach (fail closed)"
                .into(),
        );
    }

    {
        let mut guard = NODE_CHILD
            .lock()
            .map_err(|_| "p2p node lock poisoned".to_string())?;
        if let Some(child) = guard.as_mut() {
            match child.try_wait() {
                Ok(None) => {}
                _ => {
                    *guard = None;
                    clear_control_token();
                }
            }
        }
        if guard.is_none() {
            let bin = find_node_binary().ok_or_else(|| {
                "tuffswarm-node binary not found — build with `cargo build -p tuffswarm-node` or add it to PATH"
                    .to_string()
            })?;
            let control = control_base
                .trim_start_matches("http://")
                .trim_start_matches("https://")
                .trim_end_matches('/');

            let token = new_ephemeral_token();
            set_control_token(token.clone());
            persist_token(&token)?;

            let mut cmd = Command::new(&bin);
            // Pass token via env (not CLI) so it is not visible in process listings.
            cmd.arg("--control")
                .arg(control)
                .arg("--listen")
                .arg("/ip4/0.0.0.0/tcp/0")
                .env("TUFFSWARM_CONTROL_TOKEN", &token)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            let swarm = integrations::swarm_settings();
            let boot = swarm.p2p_bootstrap.trim();
            if !boot.is_empty() {
                cmd.arg("--bootstrap").arg(boot);
            }
            if swarm.p2p_relay_server {
                cmd.arg("--relay-server");
            }
            if swarm.volunteer_diagnose {
                cmd.env("TUFFSWARM_DIAGNOSE_VOLUNTEER", "1");
            }
            if swarm.creation_worker {
                cmd.env("TUFFSWARM_CREATION_WORKER", "1");
            }
            if swarm.advertised_vram_mb > 0 {
                cmd.arg("--vram-mb").arg(swarm.advertised_vram_mb.to_string());
            }
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                cmd.creation_flags(CREATE_NO_WINDOW);
            }
            let child = cmd
                .spawn()
                .map_err(|e| format!("failed to spawn {}: {e}", bin.display()))?;
            *guard = Some(child);
        }
    }

    // Wall-clock deadline instead of a fixed iteration count: every
    // p2p_authorized check can itself block up to its 2s HTTP timeout, so the
    // old 40-iteration loop could stall ~90s against a dead control URL and
    // freeze the diagnose cascade step that called us. 8s keeps attach useful.
    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    loop {
        tokio::time::sleep(Duration::from_millis(250)).await;
        if p2p_authorized(control_base).await {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            break;
        }
        // Bail out early when the child we spawned already died — polling a
        // corpse for the remaining deadline is pure freeze time.
        let mut guard = NODE_CHILD
            .lock()
            .map_err(|_| "p2p node lock poisoned".to_string())?;
        if let Some(child) = guard.as_mut() {
            if child
                .try_wait()
                .map_err(|e| format!("p2p node wait failed: {e}"))?
                .is_some()
            {
                break;
            }
        }
    }
    Err("tuffswarm-node did not become healthy/authorized in time".into())
}

fn find_node_binary() -> Option<PathBuf> {
    let bin_name = if cfg!(windows) {
        "tuffswarm-node.exe"
    } else {
        "tuffswarm-node"
    };

    // Explicit override for packagers / CI.
    if let Ok(p) = std::env::var("TUFFBOX_SWARM_NODE") {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Some(path);
        }
    }
    if let Ok(dir) = std::env::var("TUFFBOX_SWARM_NODE_DIR") {
        let path = PathBuf::from(dir).join(bin_name);
        if path.is_file() {
            return Some(path);
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            // Same folder as desktop exe (packaged resources / copied sidecar).
            for rel in [
                PathBuf::from(bin_name),
                PathBuf::from("resources").join(bin_name),
                PathBuf::from("tuffswarm-bin").join(bin_name),
                PathBuf::from("resources").join("tuffswarm-bin").join(bin_name),
                PathBuf::from("../Resources").join(bin_name),
                PathBuf::from("../Resources")
                    .join("tuffswarm-bin")
                    .join(bin_name),
                PathBuf::from("binaries").join(bin_name),
            ] {
                let cand = dir.join(rel);
                if cand.is_file() {
                    return Some(cand);
                }
            }
            for profile in ["debug", "release"] {
                let cand = dir.join("../../../target").join(profile).join(bin_name);
                if cand.is_file() {
                    return Some(cand);
                }
            }
            // Workspace cargo target (dev).
            for profile in ["debug", "release"] {
                let cand = dir
                    .join("../../../../target")
                    .join(profile)
                    .join(bin_name);
                if cand.is_file() {
                    return Some(cand);
                }
            }
        }
    }
    if let Ok(td) = std::env::var("CARGO_TARGET_DIR") {
        for profile in ["debug", "release"] {
            let cand = PathBuf::from(&td).join(profile).join(bin_name);
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    // Dev: src-tauri/binaries after build.rs copy.
    if let Ok(manifest_hint) = std::env::var("CARGO_MANIFEST_DIR") {
        let cand = PathBuf::from(manifest_hint).join("binaries").join(bin_name);
        if cand.is_file() {
            return Some(cand);
        }
    }
    which("tuffswarm-node")
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let cand = dir.join(if cfg!(windows) {
            format!("{name}.exe")
        } else {
            name.to_string()
        });
        if cand.is_file() {
            return Some(cand);
        }
    }
    None
}

#[tauri::command(rename_all = "camelCase")]
pub async fn get_p2p_node_status() -> Result<Value, String> {
    let swarm = integrations::swarm_settings();
    if !swarm.enabled || !swarm.p2p_enabled {
        return Ok(serde_json::json!({
            "enabled": false,
            "healthy": false,
            "authorized": false,
            "controlUrl": swarm.p2p_control_url,
        }));
    }
    let base = swarm.p2p_control_url.trim().trim_end_matches('/').to_string();
    let healthy = p2p_healthy(&base).await;
    let authorized = p2p_authorized(&base).await;
    let mut status = serde_json::json!({
        "enabled": true,
        "healthy": healthy,
        "authorized": authorized,
        "controlUrl": base,
        "tokenPresent": control_token().is_some(),
    });
    if authorized {
        let url = format!("{base}/v1/node/status");
        if let Ok(client) = reqwest::Client::builder()
            .timeout(Duration::from_secs(3))
            .build()
        {
            let mut req = client.get(&url);
            if let Some(token) = control_token() {
                req = req.bearer_auth(token);
            }
            if let Ok(resp) = req.send().await {
                if let Ok(body) = resp.json::<Value>().await {
                    status["node"] = body;
                }
            }
        }
    }
    Ok(status)
}

/// Lookup capsules across Supabase, then P2P, then hub; merge hits.
///
/// `bases` must come from a single [`capsule_transport_bases`] probe done by
/// the caller: resolving them here re-ran `ensure_node_running` (spawn +
/// auth polling, seconds on a dead control plane) inside every cascade step.
pub async fn lookup_across_transports(
    req: &tuffbox_core::crash_remote::CrashLookupRequest,
    bases: &[String],
) -> Option<tuffbox_core::crash_remote::CrashLookupResponse> {
    let mut merged: Vec<tuffbox_core::crash_remote::CrashLookupHit> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    if crate::integrations::swarm_supabase_configured() {
        let url = crate::integrations::swarm_supabase_url().unwrap();
        let anon = crate::integrations::swarm_supabase_anon_key().unwrap();
        if let Ok(resp) = tuffbox_core::swarm_supabase::lookup_capsules_supabase(
            &url,
            &anon,
            &req.fingerprint.key,
            req.loader.as_deref(),
            req.mc_version.as_deref(),
            req.limit,
        )
        .await
        {
            for hit in resp.hits {
                if seen.insert(hit.id.clone()) {
                    merged.push(hit);
                }
            }
        }
    }

    for base in bases {
        let token = auth_token_for_base(base);
        if let Ok(resp) =
            tuffbox_core::crash_remote::lookup_remote_async(base, token.as_deref(), req).await
        {
            for hit in resp.hits {
                if seen.insert(hit.id.clone()) {
                    merged.push(hit);
                }
            }
        }
    }
    if merged.is_empty() {
        None
    } else {
        Some(tuffbox_core::crash_remote::CrashLookupResponse {
            hits: merged,
            kb_version: None,
        })
    }
}

/// Try diagnose on each transport (P2P has no diagnose — hub/KB will succeed if present).
/// `bases` must come from a single [`capsule_transport_bases`] probe by the caller.
pub async fn diagnose_across_transports(
    req: &tuffbox_core::crash_remote::CrashDiagnoseRequest,
    bases: &[String],
) -> Result<tuffbox_core::crash_remote::CrashDiagnoseResponse, String> {
    if bases.is_empty() {
        return Err("no swarm transport (enable P2P node or set hub URL)".into());
    }
    let mut last_err = "diagnose failed on all transports".to_string();
    for base in bases {
        let token = auth_token_for_base(base);
        match tuffbox_core::crash_remote::diagnose_remote_async(base, token.as_deref(), req).await
        {
            Ok(resp) => return Ok(resp),
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

/// L2 Fog: ask a P2P volunteer via node control HTTP.
/// Returns Err when P2P is down, no capable peers, timeout, or invalid plan — caller falls through to L3.
pub async fn diagnose_via_volunteer(
    fingerprint: &tuffbox_core::crash_kb::CrashFingerprint,
    ai_ctx: &tuffbox_core::ai_explanation::CrashAiContext,
    excerpt: &str,
) -> Result<tuffbox_core::action_plan::ActionPlan, String> {
    let swarm = integrations::swarm_settings();
    if !swarm.enabled || !swarm.p2p_enabled {
        return Err("p2p not enabled".into());
    }
    let base = swarm.p2p_control_url.trim().trim_end_matches('/').to_string();
    if base.is_empty() {
        return Err("p2p control url empty".into());
    }
    ensure_node_running(&base).await?;
    let token = control_token();
    let url = format!("{base}/v1/crash/diagnose-volunteer");
    let body = serde_json::json!({
        "schemaVersion": 1,
        "jobId": uuid_v4_simple(),
        "fingerprint": fingerprint,
        "excerpt": excerpt,
        "context": {
            "suspectedMods": ai_ctx.suspected_mods,
            "mcVersion": ai_ctx.mc_version,
            "loader": ai_ctx.loader,
            "fingerprintKey": ai_ctx.fingerprint_key,
        },
        "deadlineMs": 45_000u64,
    });
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(50))
        .build()
        .map_err(|e| e.to_string())?;
    let mut req = client.post(&url).json(&body);
    if let Some(t) = token.filter(|t| !t.trim().is_empty()) {
        req = req.bearer_auth(t);
    }
    let response = req
        .send()
        .await
        .map_err(|e| format!("fog volunteer request failed: {e}"))?;
    let status = response.status();
    let value: Value = response
        .json()
        .await
        .map_err(|e| format!("fog volunteer invalid json: {e}"))?;
    if !status.is_success() {
        let msg = value
            .get("message")
            .or_else(|| value.get("error"))
            .and_then(|v| v.as_str())
            .unwrap_or("no volunteer available");
        return Err(format!("fog volunteer {status}: {msg}"));
    }
    let ok = value.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
    if !ok {
        let err = value
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("volunteer declined");
        return Err(err.into());
    }
    let plan_val = value
        .get("plan")
        .cloned()
        .ok_or_else(|| "fog volunteer response missing plan".to_string())?;
    let plan = tuffbox_core::action_plan::parse_action_plan_value(&plan_val)?;
    let validation = tuffbox_core::action_plan::validate_action_plan(&plan);
    if !validation.ok {
        return Err(format!(
            "fog volunteer plan invalid: {}",
            validation.errors.join("; ")
        ));
    }
    Ok(plan)
}

fn uuid_v4_simple() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("job-{t:x}")
}

#[tauri::command(rename_all = "camelCase")]
pub async fn ensure_p2p_node() -> Result<Value, String> {
    integrations::require_swarm_enabled()?;
    let swarm = integrations::set_swarm_p2p(true, None, None)?;
    let base = swarm.p2p_control_url.trim().trim_end_matches('/').to_string();
    ensure_node_running(&base).await?;
    maybe_start_volunteer_poller();
    maybe_start_creation_poller();
    get_p2p_node_status().await
}

/// Kill spawned node (if any) and clear token so next ensure respawns with fresh env
/// (e.g. TUFFSWARM_DIAGNOSE_VOLUNTEER).
#[tauri::command(rename_all = "camelCase")]
pub async fn restart_p2p_node() -> Result<Value, String> {
    {
        let mut guard = NODE_CHILD
            .lock()
            .map_err(|_| "p2p node lock poisoned".to_string())?;
        if let Some(mut child) = guard.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
    clear_control_token();
    VOLUNTEER_GEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let swarm = integrations::swarm_settings();
    if !swarm.enabled || !swarm.p2p_enabled {
        return Ok(serde_json::json!({
            "ok": true,
            "restarted": false,
            "reason": "p2p disabled",
        }));
    }
    let base = swarm.p2p_control_url.trim().trim_end_matches('/').to_string();
    ensure_node_running(&base).await?;
    maybe_start_volunteer_poller();
    maybe_start_creation_poller();
    get_p2p_node_status().await
}

pub fn maybe_start_volunteer_poller() {
    use std::sync::atomic::Ordering;
    if VOLUNTEER_POLLER.swap(true, Ordering::SeqCst) {
        return;
    }
    tauri::async_runtime::spawn(async {
        volunteer_poll_loop().await;
    });
}

async fn volunteer_poll_loop() {
    use std::sync::atomic::Ordering;
    let mut last_gen = VOLUNTEER_GEN.load(Ordering::SeqCst);
    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let gen = VOLUNTEER_GEN.load(Ordering::SeqCst);
        if gen != last_gen {
            last_gen = gen;
        }
        let swarm = integrations::swarm_settings();
        if !swarm.enabled || !swarm.p2p_enabled || !swarm.volunteer_diagnose {
            continue;
        }
        let base = swarm.p2p_control_url.trim().trim_end_matches('/').to_string();
        if base.is_empty() {
            continue;
        }
        if let Err(e) = ensure_node_running(&base).await {
            eprintln!("[tuffswarm] fog volunteer poller: node not ready: {e}");
            continue;
        }
        if let Err(e) = process_one_pending_job(&base).await {
            eprintln!("[tuffswarm] fog volunteer job: {e}");
        }
    }
}

async fn process_one_pending_job(base: &str) -> Result<(), String> {
    let token = control_token();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| e.to_string())?;
    let pending_url = format!("{base}/v1/node/jobs/pending");
    let mut req = client.get(&pending_url);
    if let Some(t) = token.clone().filter(|t| !t.trim().is_empty()) {
        req = req.bearer_auth(t);
    }
    let response = req.send().await.map_err(|e| e.to_string())?;
    if response.status() == reqwest::StatusCode::NO_CONTENT {
        return Ok(());
    }
    if !response.status().is_success() {
        return Err(format!("pending status {}", response.status()));
    }
    let job: Value = response.json().await.map_err(|e| e.to_string())?;
    let job_id = job
        .get("jobId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if job_id.is_empty() {
        return Err("pending job missing jobId".into());
    }

    let result = match run_volunteer_ai(&job).await {
        Ok(plan) => {
            serde_json::json!({
                "schemaVersion": 1,
                "jobId": job_id,
                "ok": true,
                "plan": plan,
                "error": null,
            })
        }
        Err(e) => {
            serde_json::json!({
                "schemaVersion": 1,
                "jobId": job_id,
                "ok": false,
                "plan": null,
                "error": humanize_local_ai_err(&e),
            })
        }
    };

    let complete_url = format!("{base}/v1/node/jobs/{job_id}/complete");
    let mut complete = client.post(&complete_url).json(&result);
    if let Some(t) = token.filter(|t| !t.trim().is_empty()) {
        complete = complete.bearer_auth(t);
    }
    let resp = complete.send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("complete status {}", resp.status()));
    }
    Ok(())
}

async fn run_volunteer_ai(job: &Value) -> Result<Value, String> {
    let settings = integrations::get_integration_status().settings;
    let ctx = fog_job_to_ai_context(job);
    let prompt = if tuffbox_core::ai_explanation::prefers_compact_crash_prompt(
        &settings.ai.provider,
        &settings.ai.model,
    ) {
        tuffbox_core::ai_explanation::build_compact_crash_prompt(&ctx)
    } else {
        tuffbox_core::ai_explanation::build_crash_prompt(&ctx)
    };
    let raw = integrations::call_ai_crash_explain(&settings.ai, &prompt)
        .await
        .map_err(|e| humanize_local_ai_err(&e))?;
    let plan = tuffbox_core::action_plan::parse_action_plan_value(&raw)?;
    let validation = tuffbox_core::action_plan::validate_action_plan(&plan);
    if !validation.ok {
        return Err(format!(
            "volunteer AI plan invalid: {}",
            validation.errors.join("; ")
        ));
    }
    let mut plan = plan;
    plan.source = Some("swarm_volunteer".into());
    serde_json::to_value(&plan).map_err(|e| e.to_string())
}

fn truncate_err(s: &str, max: usize) -> String {
    let t = s.trim();
    if t.len() <= max {
        t.to_string()
    } else {
        format!("{}…", &t[..max])
    }
}

/// Short actionable copy for Ollama / local AI failures (Fog + Creation).
fn humanize_local_ai_err(raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();
    if lower.contains("connection refused")
        || lower.contains("failed to fetch")
        || lower.contains("unreachable")
        || lower.contains("tcp")
        || (lower.contains("ollama")
            && (lower.contains("unavailable") || lower.contains("not running")))
    {
        return "Ollama unavailable — install/start it and set Settings → AI".into();
    }
    if lower.contains("not installed")
        || lower.contains("no model")
        || lower.contains("model not found")
        || (lower.contains("pull") && lower.contains("model"))
    {
        return "AI model missing — install a model in Settings → AI".into();
    }
    truncate_err(raw, 160)
}

fn fog_job_to_ai_context(job: &Value) -> tuffbox_core::ai_explanation::CrashAiContext {
    let ctx = job.get("context").cloned().unwrap_or(Value::Null);
    if let Ok(parsed) =
        serde_json::from_value::<tuffbox_core::ai_explanation::CrashAiContext>(ctx.clone())
    {
        if !parsed.mc_version.is_empty()
            || parsed.group_test.is_some()
            || parsed.trail_covering.is_some()
            || !parsed.fingerprint_key.is_empty()
        {
            return parsed;
        }
    }
    let fingerprint = job.get("fingerprint").cloned().unwrap_or(Value::Null);
    let mc = ctx
        .get("mcVersion")
        .and_then(|v| v.as_str())
        .or_else(|| fingerprint.get("mcMajor").and_then(|v| v.as_str()))
        .unwrap_or("")
        .to_string();
    let loader = ctx
        .get("loader")
        .and_then(|v| v.as_str())
        .or_else(|| fingerprint.get("loader").and_then(|v| v.as_str()))
        .unwrap_or("")
        .to_string();
    let suspected: Vec<String> = ctx
        .get("suspectedMods")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let fp_key = ctx
        .get("fingerprintKey")
        .and_then(|v| v.as_str())
        .or_else(|| fingerprint.get("key").and_then(|v| v.as_str()))
        .unwrap_or("")
        .to_string();
    let excerpt = job
        .get("excerpt")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    tuffbox_core::ai_explanation::CrashAiContext {
        mc_version: mc,
        loader: loader.clone(),
        loader_version: String::new(),
        java_version: String::new(),
        os: std::env::consts::OS.to_string(),
        installed_mods: suspected.clone(),
        installed_mod_count: suspected.len() as u32,
        crash_report_excerpt: excerpt.clone(),
        latest_log_excerpt: String::new(),
        suspected_mods: suspected,
        culprit_details: vec![],
        crash_assistant_findings: vec![],
        recent_changes: vec![],
        graph_diagnostics: vec![],
        similar_cases: vec![],
        fingerprint_key: fp_key,
        report_id: None,
        inventory: None,
        group_test: None,
        trail_covering: None,
    }
}

static CREATION_POLLER: Lazy<std::sync::atomic::AtomicBool> =
    Lazy::new(|| std::sync::atomic::AtomicBool::new(false));

pub fn maybe_start_creation_poller() {
    use std::sync::atomic::Ordering;
    if CREATION_POLLER.swap(true, Ordering::SeqCst) {
        return;
    }
    tauri::async_runtime::spawn(async {
        creation_poll_loop().await;
    });
}

async fn creation_poll_loop() {
    use std::sync::atomic::Ordering;
    let mut last_gen = VOLUNTEER_GEN.load(Ordering::SeqCst);
    loop {
        tokio::time::sleep(Duration::from_secs(3)).await;
        let gen = VOLUNTEER_GEN.load(Ordering::SeqCst);
        if gen != last_gen {
            last_gen = gen;
        }
        let swarm = integrations::swarm_settings();
        if !swarm.enabled || !swarm.p2p_enabled || !swarm.creation_worker {
            continue;
        }
        let base = swarm.p2p_control_url.trim().trim_end_matches('/').to_string();
        if base.is_empty() {
            continue;
        }
        if let Err(e) = ensure_node_running(&base).await {
            eprintln!("[tuffswarm] creation worker poller: node not ready: {e}");
            continue;
        }
        if let Err(e) = process_one_creation_job(&base).await {
            eprintln!("[tuffswarm] creation job: {e}");
        }
    }
}

fn creation_kind_uses_ai(kind: &str) -> bool {
    let k = kind.trim();
    k.starts_with("kubejs_") || k == "quest_scripts" || k == "recipe_balance"
}

fn parse_ai_creation_artifacts(
    value: &Value,
) -> Result<Vec<tuffbox_core::creation_marketplace::CreationArtifact>, String> {
    let arr = value
        .get("artifacts")
        .and_then(|a| a.as_array())
        .ok_or_else(|| "AI JSON missing artifacts[]".to_string())?;
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let path = item
            .get("path")
            .and_then(|p| p.as_str())
            .ok_or_else(|| "artifact missing path".to_string())?
            .to_string();
        let content = item
            .get("content")
            .and_then(|c| c.as_str())
            .ok_or_else(|| "artifact missing content".to_string())?
            .to_string();
        out.push(tuffbox_core::creation_marketplace::CreationArtifact { path, content });
    }
    if out.is_empty() {
        return Err("AI returned empty artifacts".into());
    }
    Ok(out)
}

async fn generate_creation_artifacts_ai(
    job: &tuffbox_core::creation_marketplace::CreationJob,
) -> Result<Vec<tuffbox_core::creation_marketplace::CreationArtifact>, String> {
    let settings = integrations::read_settings().ai;
    let system = "You are TuffBox Creation Marketplace worker. Return ONLY JSON with shape {\"artifacts\":[{\"path\":\"relative/posix/path\",\"content\":\"file body\"}]}. Paths must be relative (no ..). Prefer kubejs/, config/, or quests/ trees. Do not invent absolute paths or .jar files.";
    let mods = if job.constraints.mod_ids.is_empty() {
        "(none listed)".to_string()
    } else {
        job.constraints.mod_ids.join(", ")
    };
    let user = format!(
        "kind: {}\nmcVersion: {}\nloader: {}\nmodIds: {}\nbrief:\n{}\n\nProduce 1-{} artifacts.",
        job.kind,
        job.constraints.mc_version,
        job.constraints.loader,
        mods,
        job.brief.trim(),
        tuffbox_core::creation_marketplace::MAX_ARTIFACTS
    );
    let value = integrations::call_ai_messages(
        &settings,
        system,
        &[serde_json::json!({"role": "user", "content": user})],
        true,
    )
    .await?;
    parse_ai_creation_artifacts(&value)
}

async fn process_one_creation_job(base: &str) -> Result<(), String> {
    let token = control_token();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(150))
        .build()
        .map_err(|e| e.to_string())?;
    let pending_url = format!("{base}/v1/node/creation/jobs/pending");
    let mut req = client.get(&pending_url);
    if let Some(t) = token.clone().filter(|t| !t.trim().is_empty()) {
        req = req.bearer_auth(t);
    }
    let response = req.send().await.map_err(|e| e.to_string())?;
    if response.status() == reqwest::StatusCode::NO_CONTENT {
        return Ok(());
    }
    if !response.status().is_success() {
        return Err(format!("creation pending status {}", response.status()));
    }
    let job_val: Value = response.json().await.map_err(|e| e.to_string())?;
    let job: tuffbox_core::creation_marketplace::CreationJob =
        serde_json::from_value(job_val).map_err(|e| format!("invalid CreationJob: {e}"))?;
    job.validate()?;

    let mut source = "scaffold";
    let mut claimed = 0.35_f64;
    let mut fallback_note: Option<String> = None;
    let mut artifacts = if creation_kind_uses_ai(&job.kind) {
        match generate_creation_artifacts_ai(&job).await {
            Ok(arts) => {
                let mut draft = tuffbox_core::creation_marketplace::CreationResult {
                    schema_version: 1,
                    job_id: job.job_id.clone(),
                    worker_peer_id: None,
                    worker_signer_public_key: None,
                    ok: true,
                    artifacts: arts.clone(),
                    claimed_confidence: 0.55,
                    error: None,
                };
                let report =
                    tuffbox_core::creation_marketplace::verify_creation_result(&job, &draft);
                if report.passed {
                    source = "ai";
                    claimed = 0.55;
                    arts
                } else {
                    let detail = report
                        .checks
                        .iter()
                        .filter(|c| !c.ok)
                        .map(|c| c.detail.as_str())
                        .collect::<Vec<_>>()
                        .join("; ");
                    eprintln!(
                        "[tuffswarm] creation AI verify failed for {}: {}; falling back to scaffold",
                        job.job_id, detail
                    );
                    fallback_note = Some(format!(
                        "AI output failed verify ({}); used scaffold. Check Settings → AI if this keeps happening.",
                        truncate_err(&detail, 120)
                    ));
                    draft.artifacts.clear();
                    tuffbox_core::creation_marketplace::scaffold_creation_artifacts(&job)
                }
            }
            Err(e) => {
                eprintln!(
                    "[tuffswarm] creation AI failed for {}: {}; falling back to scaffold",
                    job.job_id, e
                );
                fallback_note = Some(format!(
                    "AI unavailable ({}); used scaffold. Open Settings → AI (Ollama/model).",
                    humanize_local_ai_err(&e)
                ));
                tuffbox_core::creation_marketplace::scaffold_creation_artifacts(&job)
            }
        }
    } else {
        tuffbox_core::creation_marketplace::scaffold_creation_artifacts(&job)
    };

    let worker_signer = tuffbox_core::swarm::device_signer_public_key_b64().ok();
    let mut result = tuffbox_core::creation_marketplace::CreationResult {
        schema_version: 1,
        job_id: job.job_id.clone(),
        worker_peer_id: None,
        worker_signer_public_key: worker_signer,
        ok: true,
        artifacts: std::mem::take(&mut artifacts),
        claimed_confidence: claimed,
        error: fallback_note,
    };
    let report = tuffbox_core::creation_marketplace::verify_creation_result(&job, &result);
    if !report.passed {
        if source == "ai" {
            // Should not happen (pre-verified), but keep safe.
            eprintln!(
                "[tuffswarm] creation unexpected AI fail after accept; scaffolding {}",
                job.job_id
            );
            result.artifacts =
                tuffbox_core::creation_marketplace::scaffold_creation_artifacts(&job);
            result.claimed_confidence = 0.35;
            result.error = Some(
                "AI result failed late verify; used scaffold. Check Settings → AI.".into(),
            );
            let report2 =
                tuffbox_core::creation_marketplace::verify_creation_result(&job, &result);
            if !report2.passed {
                result.ok = false;
                result.artifacts.clear();
                result.error = Some(format!(
                    "scaffold failed verify: {}",
                    report2
                        .checks
                        .iter()
                        .filter(|c| !c.ok)
                        .map(|c| c.detail.as_str())
                        .collect::<Vec<_>>()
                        .join("; ")
                ));
            }
        } else {
            result.ok = false;
            result.artifacts.clear();
            result.error = Some(format!(
                "local scaffold failed verify: {}",
                report
                    .checks
                    .iter()
                    .filter(|c| !c.ok)
                    .map(|c| c.detail.as_str())
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
    }

    let complete_url = format!("{base}/v1/node/creation/jobs/{}/complete", job.job_id);
    let mut complete = client.post(&complete_url).json(&result);
    if let Some(t) = token.filter(|t| !t.trim().is_empty()) {
        complete = complete.bearer_auth(t);
    }
    let resp = complete.send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("creation complete status {}", resp.status()));
    }
    Ok(())
}

/// Customer: submit CreationJob to a peer worker; verify artifacts locally before returning.
#[tauri::command(rename_all = "camelCase")]
pub async fn submit_creation_job(
    job: tuffbox_core::creation_marketplace::CreationJob,
) -> Result<serde_json::Value, String> {
    job.validate()?;
    let swarm = integrations::swarm_settings();
    if !swarm.enabled || !swarm.p2p_enabled {
        return Err("p2p not enabled".into());
    }
    let base = swarm.p2p_control_url.trim().trim_end_matches('/').to_string();
    if base.is_empty() {
        return Err("p2p control url empty".into());
    }
    ensure_node_running(&base).await?;
    let token = control_token();
    let url = format!("{base}/v1/creation/submit");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(140))
        .build()
        .map_err(|e| e.to_string())?;
    let mut req = client.post(&url).json(&job);
    if let Some(t) = token.filter(|t| !t.trim().is_empty()) {
        req = req.bearer_auth(t);
    }
    let response = req
        .send()
        .await
        .map_err(|e| format!("creation submit failed: {e}"))?;
    let status = response.status();
    let value: Value = response
        .json()
        .await
        .map_err(|e| format!("creation submit invalid json: {e}"))?;
    if !status.is_success() {
        let msg = value
            .get("error")
            .or_else(|| value.get("message"))
            .and_then(|v| v.as_str())
            .unwrap_or("no creation worker available");
        return Err(format!("creation submit {status}: {msg}"));
    }
    let result: tuffbox_core::creation_marketplace::CreationResult =
        serde_json::from_value(value.clone())
            .map_err(|e| format!("invalid CreationResult: {e}"))?;
    let report = tuffbox_core::creation_marketplace::verify_creation_result(&job, &result);
    Ok(serde_json::json!({
        "result": result,
        "verification": report,
    }))
}

/// Defaults for CreationJob form: MC/loader/modIds from open project inventory.
#[tauri::command(rename_all = "camelCase")]
pub fn creation_job_defaults(path: String) -> Result<serde_json::Value, String> {
    use crate::helpers::{manifest_parent, resolve_manifest_path};
    use tuffbox_core::swarm::pack_mod_ids;
    use tuffbox_core::ProjectManifest;

    let manifest_path = resolve_manifest_path(&path)?;
    let manifest = ProjectManifest::load_from_path(&manifest_path).map_err(|e| e.to_string())?;
    let project_dir = manifest_parent(&path)?;
    Ok(serde_json::json!({
        "mcVersion": manifest.minecraft.version,
        "loader": manifest.loader.kind.as_str(),
        "modIds": pack_mod_ids(&manifest),
        "projectDir": project_dir.to_string_lossy(),
        "kinds": tuffbox_core::creation_marketplace::KNOWN_CREATION_KINDS,
    }))
}

/// Path-safe write of Creation artifacts into the open project (after UI confirm).
#[tauri::command(rename_all = "camelCase")]
pub fn apply_creation_artifacts(
    path: String,
    artifacts: Vec<tuffbox_core::creation_marketplace::CreationArtifact>,
) -> Result<serde_json::Value, String> {
    use crate::helpers::manifest_parent;
    let project_dir = manifest_parent(&path)?;
    let written = tuffbox_core::creation_marketplace::apply_creation_artifacts_to_dir(
        &project_dir,
        &artifacts,
    )?;
    Ok(serde_json::json!({ "written": written }))
}
