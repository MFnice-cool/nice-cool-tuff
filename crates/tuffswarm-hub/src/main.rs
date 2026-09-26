//! TuffSwarm Hub — shared HTTP store for ExperienceCapsules.
//!
//! Clients publish sanitized crash→fix capsules (fingerprint + solution + actions).
//! Raw crash logs and author notes are rejected / stripped.
//!
//! Endpoints:
//! - GET  /health
//! - POST /v1/crash/capsules
//! - POST /v1/crash/lookup
//! - POST /v1/crash/diagnose
//! - POST /v1/mods/cooccurrence
//! - GET  /v1/mods/cooccurrence?version=&loader=&limit=
//! - GET  /v1/mods/modpacks?query=&page=&limit=&categoryId=&version= (MPI proxy)
//! - GET  /v1/mods/modpack-categories (pack themes + MPI merge, cached)

mod mpi_analytics;

use axum::{
    extract::{Query, Request, State},
    http::{header::AUTHORIZATION, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tuffbox_core::crash_remote::{
    CrashDiagnoseRequest, CrashDiagnoseResponse, CrashLookupRequest, CrashLookupResponse,
};
use tuffbox_core::swarm::{CapsuleLibrary, CooccurrenceStore, ExperienceCapsule, ModPairStat};

const MPI_CACHE_TTL: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, Parser)]
#[command(name = "tuffswarm-hub")]
#[command(about = "Shared TuffSwarm hub for durable crash→fix capsules (no raw logs)")]
struct Args {
    /// Listen address (default localhost — use 0.0.0.0 only behind a firewall + hub token).
    #[arg(long, default_value = "127.0.0.1:8787")]
    bind: String,
    /// Bearer token for `/v1/*` (or env `TUFFSWARM_HUB_TOKEN`). Required for non-loopback binds.
    #[arg(long)]
    hub_token: Option<String>,
    /// Directory for durable JSONL capsule store.
    #[arg(long)]
    data_dir: Option<PathBuf>,
    /// Run one Modpack Index → Supabase co-occurrence sync and exit (requires SUPABASE_* env).
    #[arg(long, default_value_t = false)]
    mpi_sync_once: bool,
    /// Packs per version×loader target for MPI sync (default 20).
    #[arg(long, default_value_t = 20)]
    mpi_packs: u32,
}

struct CacheEntry {
    at: Instant,
    body: Value,
}

struct HubState {
    library: Mutex<CapsuleLibrary>,
    cooccur_path: PathBuf,
    supabase: Option<mpi_analytics::SupabaseCreds>,
    /// Short TTL cache for MPI proxy responses (modpacks / categories).
    mpi_cache: Mutex<HashMap<String, CacheEntry>>,
    /// Empty = auth disabled (only allowed on loopback).
    hub_token: Arc<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "tuffswarm_hub=info,tower_http=info".into()),
        )
        .init();

    let args = Args::parse();
    let supabase = mpi_analytics::creds_from_env();

    if args.mpi_sync_once {
        let Some(creds) = supabase.as_ref() else {
            anyhow::bail!("--mpi-sync-once requires SUPABASE_URL and SUPABASE_SERVICE_ROLE_KEY");
        };
        mpi_analytics::run_mpi_sync_once(creds, args.mpi_packs)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
        return Ok(());
    }

    let hub_token = args
        .hub_token
        .filter(|t| !t.trim().is_empty())
        .or_else(|| {
            std::env::var("TUFFSWARM_HUB_TOKEN")
                .ok()
                .filter(|t| !t.trim().is_empty())
        })
        .unwrap_or_default();

    let addr: SocketAddr = args.bind.parse()?;
    let is_loopback = addr.ip().is_loopback();
    if !is_loopback && hub_token.is_empty() {
        anyhow::bail!(
            "refusing to bind {addr} without --hub-token / TUFFSWARM_HUB_TOKEN (use 127.0.0.1 for local-only)"
        );
    }
    if hub_token.is_empty() {
        tracing::warn!(
            "hub token unset — /v1/* is open on loopback only; set TUFFSWARM_HUB_TOKEN for production"
        );
    } else {
        tracing::info!("hub bearer auth enabled for /v1/*");
    }

    let data_dir = args.data_dir.unwrap_or_else(default_data_dir);
    std::fs::create_dir_all(&data_dir)?;
    let capsules_path = data_dir.join("capsules.jsonl");
    let cooccur_path = data_dir.join("cooccurrence.json");
    tracing::info!(?data_dir, "TuffSwarm hub data directory");

    if let Some(creds) = supabase.clone() {
        tracing::info!("Supabase configured — scheduling daily MPI analytics sync");
        mpi_analytics::spawn_daily_loop(creds);
    } else {
        tracing::info!("SUPABASE_URL / SUPABASE_SERVICE_ROLE_KEY unset — MPI sync disabled");
    }

    let state = Arc::new(HubState {
        library: Mutex::new(CapsuleLibrary::open(capsules_path)),
        cooccur_path,
        supabase,
        mpi_cache: Mutex::new(HashMap::new()),
        hub_token: Arc::new(hub_token),
    });

    let protected = Router::new()
        .route("/v1/crash/capsules", post(publish_capsule))
        .route("/v1/crash/lookup", post(lookup))
        .route("/v1/crash/diagnose", post(diagnose))
        .route(
            "/v1/mods/cooccurrence",
            post(cooccurrence).get(cooccurrence_get),
        )
        .route("/v1/mods/modpacks", get(modpacks_get))
        .route("/v1/mods/modpack-categories", get(modpack_categories_get))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_bearer,
        ));

    let app = Router::new()
        .route("/health", get(health))
        .merge(protected)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    tracing::info!(%addr, "listening — point TuffBox Settings → TuffSwarm hub URL here");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn require_bearer(
    State(state): State<Arc<HubState>>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let expected = state.hub_token.as_str();
    if expected.is_empty() {
        return Ok(next.run(req).await);
    }
    let header = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let token = header
        .strip_prefix("Bearer ")
        .or_else(|| header.strip_prefix("bearer "))
        .unwrap_or("");
    if token == expected {
        Ok(next.run(req).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

fn default_data_dir() -> PathBuf {
    dirs::data_dir()
        .or_else(dirs::config_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("TuffBox")
        .join("tuffswarm-hub")
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Health {
    ok: bool,
    service: &'static str,
    capsule_count: usize,
    /// Co-occurrence graph writers: `launcher` (POST / client reports) + `mpi` (hub crawl).
    cooccurrence_sources: &'static [&'static str],
    supabase_configured: bool,
}

async fn health(State(state): State<Arc<HubState>>) -> impl IntoResponse {
    let count = state
        .library
        .lock()
        .map(|lib| lib.load_all().len())
        .unwrap_or(0);
    Json(Health {
        ok: true,
        service: "tuffswarm-hub",
        capsule_count: count,
        // last_source in launcher table: "launcher"
        // MPI graph is separate table mpi_mod_cooccurrence_pairs
        cooccurrence_sources: &["launcher", "mpi"],
        supabase_configured: state.supabase.is_some(),
    })
}

async fn publish_capsule(
    State(state): State<Arc<HubState>>,
    Json(body): Json<Value>,
) -> Result<impl IntoResponse, (StatusCode, Json<Value>)> {
    // Reject payloads that try to ship raw logs.
    if body.get("rawLog").is_some()
        || body.get("rawLogs").is_some()
        || body.get("crashReport").is_some()
        || body.get("latestLog").is_some()
        || body.pointer("/privacy/rawLogs").and_then(|v| v.as_bool()) == Some(true)
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "raw logs are not accepted — publish ExperienceCapsule only (fingerprint + solution + actions)"
            })),
        ));
    }
    if body.get("notes").is_some() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "author notes must not be published to the swarm hub" })),
        ));
    }

    let capsule = ExperienceCapsule::from_public_value(&body)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))))?;

    match capsule.verify_signature() {
        Ok(true) => {}
        Ok(false) => {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "unsigned capsule rejected — Ed25519 signature required" })),
            ));
        }
        Err(e) => {
            return Err((StatusCode::BAD_REQUEST, Json(json!({ "error": e }))));
        }
    }

    let stored = {
        let lib = state.library.lock().map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "hub lock poisoned" })),
            )
        })?;
        lib.publish(&capsule)
            .map_err(|e| (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))))?
    };

    tracing::info!(id = %stored.id, key = %stored.fingerprint.key, "capsule stored");
    Ok((
        StatusCode::OK,
        Json(json!({
            "ok": true,
            "id": stored.id,
            "fingerprintKey": stored.fingerprint.key,
            "successScore": stored.success_score,
            "successCount": stored.success_count,
        })),
    ))
}

async fn lookup(
    State(state): State<Arc<HubState>>,
    Json(req): Json<CrashLookupRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<Value>)> {
    let hay = req.excerpt.clone().unwrap_or_default();
    let limit = req.limit.max(1).min(20) as usize;
    let hits = {
        let lib = state.library.lock().map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "hub lock poisoned" })),
            )
        })?;
        lib.lookup(&req.fingerprint, &hay, limit)
    };
    Ok(Json(CrashLookupResponse {
        kb_version: Some(format!("hub-{}", tuffbox_core::time_util::compact_now())),
        hits,
    }))
}

async fn diagnose(
    State(state): State<Arc<HubState>>,
    Json(req): Json<CrashDiagnoseRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<Value>)> {
    let hay = req.excerpt.clone().unwrap_or_default();
    let plan = {
        let lib = state.library.lock().map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "hub lock poisoned" })),
            )
        })?;
        lib.diagnose_best(&req.fingerprint, &hay)
    };
    let Some(mut plan) = plan else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "no matching capsule for this fingerprint" })),
        ));
    };
    plan.source = Some("swarm".into());
    Ok(Json(CrashDiagnoseResponse {
        plan,
        kb_version: Some(format!("hub-{}", tuffbox_core::time_util::compact_now())),
        used_llm: false,
    }))
}

async fn cooccurrence(
    State(state): State<Arc<HubState>>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let mut seed_rows: Vec<Value> = Vec::new();
    if let Some(pairs) = body.get("pairs").and_then(|v| v.as_array()) {
        let mut store = load_hub_cooccur(&state.cooccur_path);
        if let Some(mc) = body.get("mcVersion").and_then(|v| v.as_str()) {
            store.mc_version = mc.to_string();
        }
        if let Some(loader) = body.get("loader").and_then(|v| v.as_str()) {
            store.loader = loader.to_string();
        }
        let mc = body
            .get("mcVersion")
            .or_else(|| body.get("version"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let loader = body
            .get("loader")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        for p in pairs {
            let a = p
                .get("modA")
                .or_else(|| p.get("mod_a"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let b = p
                .get("modB")
                .or_else(|| p.get("mod_b"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let count = p.get("count").and_then(|v| v.as_u64()).unwrap_or(1);
            if a.is_empty() || b.is_empty() {
                continue;
            }
            let key = if a <= b {
                format!("{a}||{b}")
            } else {
                format!("{b}||{a}")
            };
            *store.pairs.entry(key).or_insert(0) += count;
            // Seed row for Supabase (weight = report count).
            let mut ma = a.trim().to_ascii_lowercase();
            let mut mb = b.trim().to_ascii_lowercase();
            if ma > mb {
                std::mem::swap(&mut ma, &mut mb);
            }
            if ma != mb {
                seed_rows.push(json!({
                    "mod_a": ma,
                    "mod_b": mb,
                    "mc_version": mc,
                    "loader": loader,
                    "weight": count.max(1),
                    "last_source": "launcher",
                }));
            }
        }
        let _ = save_hub_cooccur(&state.cooccur_path, &store);
    }

    // When service role is set, mirror POST into Supabase (same table MPI uses).
    if !seed_rows.is_empty() {
        if let Some(creds) = state.supabase.as_ref() {
            match tuffbox_core::swarm_supabase::seed_cooccurrence_pairs_supabase(
                &creds.url,
                &creds.service_role_key,
                &seed_rows,
            )
            .await
            {
                Ok(n) => tracing::info!(pairs = n, "seeded launcher cooccurrence into Supabase"),
                Err(e) => tracing::warn!(error = %e, "launcher cooccurrence Supabase seed failed"),
            }
        }
    }

    let version = body
        .get("mcVersion")
        .or_else(|| body.get("version"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let loader = body.get("loader").and_then(|v| v.as_str()).unwrap_or("");
    let limit = body
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(25)
        .min(100) as u32;
    Json(resolve_cooccurrence(&state, version, loader, limit).await)
}

#[derive(Debug, Deserialize)]
struct CooccurrenceGetQuery {
    version: Option<String>,
    loader: Option<String>,
    limit: Option<u64>,
}

async fn cooccurrence_get(
    State(state): State<Arc<HubState>>,
    Query(q): Query<CooccurrenceGetQuery>,
) -> impl IntoResponse {
    let version = q.version.unwrap_or_default();
    let loader = q.loader.unwrap_or_default();
    let limit = q.limit.unwrap_or(25).min(100) as u32;
    Json(resolve_cooccurrence(&state, &version, &loader, limit).await)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModpacksGetQuery {
    query: Option<String>,
    page: Option<u32>,
    limit: Option<u32>,
    category_id: Option<u32>,
    version: Option<String>,
}

/// Proxy Modpack Index pack search through the hub (analytics UA; no end-user IP to MPI).
async fn modpacks_get(
    State(state): State<Arc<HubState>>,
    Query(q): Query<ModpacksGetQuery>,
) -> impl IntoResponse {
    let query = q.query.clone().unwrap_or_default();
    let page = q.page.unwrap_or(1);
    let limit = q.limit.unwrap_or(12).clamp(1, 40);
    let category_id = q.category_id;
    let version = q.version.clone().unwrap_or_default();
    let cache_key = format!(
        "modpacks|q={query}|p={page}|l={limit}|c={}|v={version}",
        category_id.unwrap_or(0)
    );
    if let Some(cached) = cache_get(&state, &cache_key) {
        return (StatusCode::OK, Json(cached)).into_response();
    }

    let query_opt = if query.trim().is_empty() {
        None
    } else {
        Some(query)
    };
    let version_opt = if version.trim().is_empty() {
        None
    } else {
        Some(version)
    };
    match tokio::task::spawn_blocking(move || {
        let version_id = version_opt
            .as_deref()
            .and_then(tuffbox_core::modpack_index::resolve_mc_version_id);
        tuffbox_core::modpack_index::search_modpacks_hub(
            query_opt.as_deref(),
            page,
            limit,
            category_id,
            version_id,
        )
    })
    .await
    {
        Ok(Ok((results, total))) => {
            let body = json!({
                "results": results,
                "total": total,
                "source": "mpi-hub",
            });
            cache_put(&state, cache_key, body.clone());
            (StatusCode::OK, Json(body)).into_response()
        }
        Ok(Err(e)) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "message": e, "results": [], "total": 0 })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "message": e.to_string(), "results": [], "total": 0 })),
        )
            .into_response(),
    }
}

async fn modpack_categories_get(State(state): State<Arc<HubState>>) -> impl IntoResponse {
    let cache_key = "modpack-categories".to_string();
    if let Some(cached) = cache_get(&state, &cache_key) {
        return (StatusCode::OK, Json(cached)).into_response();
    }
    // Prefer full hub list; on MPI failure still return static pack themes.
    let body =
        match tokio::task::spawn_blocking(tuffbox_core::modpack_index::list_categories_hub).await {
            Ok(Ok(cats)) => json!({ "categories": cats, "source": "mpi-hub" }),
            _ => json!({
                "categories": tuffbox_core::modpack_index::list_pack_theme_categories(),
                "source": "builtin-themes",
            }),
        };
    cache_put(&state, cache_key, body.clone());
    (StatusCode::OK, Json(body)).into_response()
}

fn cache_get(state: &HubState, key: &str) -> Option<Value> {
    let Ok(mut map) = state.mpi_cache.lock() else {
        return None;
    };
    let entry = map.get(key)?;
    if entry.at.elapsed() > MPI_CACHE_TTL {
        map.remove(key);
        return None;
    }
    Some(entry.body.clone())
}

fn cache_put(state: &HubState, key: String, body: Value) {
    if let Ok(mut map) = state.mpi_cache.lock() {
        map.insert(
            key,
            CacheEntry {
                at: Instant::now(),
                body,
            },
        );
        // ponytail: O(n) prune when map grows; upgrade to timed sweep if hub is busy.
        if map.len() > 256 {
            map.retain(|_, e| e.at.elapsed() <= MPI_CACHE_TTL);
        }
    }
}

async fn resolve_cooccurrence(state: &HubState, version: &str, loader: &str, limit: u32) -> Value {
    if let Some(creds) = &state.supabase {
        let launcher = tuffbox_core::swarm_supabase::fetch_cooccurrence_supabase(
            &creds.url,
            &creds.service_role_key,
            version,
            loader,
            limit,
        )
        .await
        .unwrap_or_default();
        let mpi = tuffbox_core::swarm_supabase::fetch_mpi_cooccurrence_supabase(
            &creds.url,
            &creds.service_role_key,
            version,
            loader,
            limit,
        )
        .await
        .unwrap_or_default();
        if !launcher.is_empty() || !mpi.is_empty() {
            // Prefer launcher pairs for `pairs`; expose MPI separately.
            let pairs = if !launcher.is_empty() {
                launcher.clone()
            } else {
                mpi.clone()
            };
            return json!({
                "mcVersion": version,
                "loader": loader,
                "pairs": pairs,
                "launcherPairs": launcher,
                "mpiPairs": mpi,
                "source": if !launcher.is_empty() { "supabase-launcher" } else { "supabase-mpi" },
            });
        }
    }
    let mut payload = cooccurrence_payload(&state.cooccur_path, limit as usize);
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("source".into(), json!("local"));
        if !version.is_empty() {
            obj.insert("mcVersion".into(), json!(version));
        }
        if !loader.is_empty() {
            obj.insert("loader".into(), json!(loader));
        }
    }
    payload
}

fn cooccurrence_payload(path: &PathBuf, limit: usize) -> Value {
    let store = load_hub_cooccur(path);
    let mut pairs: Vec<ModPairStat> = store
        .pairs
        .iter()
        .filter_map(|(k, &count)| {
            let mut parts = k.splitn(2, "||");
            Some(ModPairStat {
                mod_a: parts.next()?.to_string(),
                mod_b: parts.next()?.to_string(),
                count,
            })
        })
        .collect();
    pairs.sort_by(|a, b| b.count.cmp(&a.count));
    pairs.truncate(limit);
    json!({
        "mcVersion": store.mc_version,
        "loader": store.loader,
        "pairs": pairs,
    })
}

fn load_hub_cooccur(path: &PathBuf) -> CooccurrenceStore {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_hub_cooccur(path: &PathBuf, store: &CooccurrenceStore) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(
        path,
        serde_json::to_vec_pretty(store).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
