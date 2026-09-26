//! Local control HTTP — same paths as tuffswarm-hub so the desktop bridge is uniform.
//! `/health` is open; `/v1/*` requires `Authorization: Bearer <control-token>`.

use axum::{
    extract::{Path, State},
    http::{header::AUTHORIZATION, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;
use tuffbox_core::crash_remote::{CrashLookupRequest, CrashLookupResponse};
use tuffbox_core::swarm::{CapsuleLibrary, ExperienceCapsule};

use crate::creation_jobs::PendingCreationJobs;
use crate::diagnose::{DiagnoseJob, DiagnoseResult};
use crate::jobs::PendingJobs;
use crate::p2p::P2pHandle;
use tuffbox_core::creation_marketplace::{CreationJob, CreationResult};

#[derive(Clone)]
struct AppState {
    library: Arc<Mutex<CapsuleLibrary>>,
    p2p: P2pHandle,
    pending_jobs: Arc<Mutex<PendingJobs>>,
    pending_creation: Arc<Mutex<PendingCreationJobs>>,
    control_token: Arc<String>,
    relay_server: bool,
    vram_mb: u32,
    max_jobs: u32,
}

pub async fn serve(
    addr: SocketAddr,
    library: Arc<Mutex<CapsuleLibrary>>,
    p2p: P2pHandle,
    pending_jobs: Arc<Mutex<PendingJobs>>,
    pending_creation: Arc<Mutex<PendingCreationJobs>>,
    control_token: String,
    relay_server: bool,
    vram_mb: u32,
    max_jobs: u32,
) -> anyhow::Result<()> {
    let state = AppState {
        library,
        p2p,
        pending_jobs,
        pending_creation,
        control_token: Arc::new(control_token),
        relay_server,
        vram_mb,
        max_jobs,
    };
    let protected = Router::new()
        .route("/v1/crash/capsules", post(publish_capsule))
        .route("/v1/crash/lookup", post(lookup))
        .route("/v1/crash/diagnose-volunteer", post(diagnose_volunteer))
        .route("/v1/node/jobs/pending", get(jobs_pending))
        .route("/v1/node/jobs/:id/complete", post(jobs_complete))
        .route("/v1/creation/submit", post(creation_submit))
        .route("/v1/node/creation/jobs/pending", get(creation_jobs_pending))
        .route(
            "/v1/node/creation/jobs/:id/complete",
            post(creation_jobs_complete),
        )
        .route("/v1/node/status", get(node_status))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_bearer,
        ));

    let app = Router::new()
        .route("/health", get(health))
        .merge(protected)
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn require_bearer(
    State(state): State<AppState>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let expected = state.control_token.as_str();
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Health {
    ok: bool,
    service: &'static str,
    capsule_count: usize,
    peers: usize,
}

async fn health(State(state): State<AppState>) -> impl IntoResponse {
    let count = state.library.lock().await.load_all().len();
    let peers = state.p2p.peer_count().await;
    Json(Health {
        ok: true,
        service: "tuffswarm-node",
        capsule_count: count,
        peers,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NodeStatus {
    peers: usize,
    listen_addrs: Vec<String>,
    capsule_count: usize,
    volunteer_peers: Vec<String>,
    creation_peers: Vec<String>,
    relay_server: bool,
    circuit_listen_addrs: Vec<String>,
    gossip_published: u64,
    gossip_received: u64,
    gossip_last_error: String,
    vram_mb: u32,
    max_jobs: u32,
}

async fn node_status(State(state): State<AppState>) -> impl IntoResponse {
    let capsule_count = state.library.lock().await.load_all().len();
    let listen_addrs = state.p2p.listen_addrs().await;
    let circuit_listen_addrs: Vec<String> = listen_addrs
        .iter()
        .filter(|a| a.contains("p2p-circuit"))
        .cloned()
        .collect();
    let gossip = state.p2p.gossip_stats_snapshot();
    Json(NodeStatus {
        peers: state.p2p.peer_count().await,
        listen_addrs,
        capsule_count,
        volunteer_peers: state.p2p.list_volunteer_peers().await,
        creation_peers: state.p2p.list_creation_peers().await,
        relay_server: state.relay_server,
        circuit_listen_addrs,
        gossip_published: gossip.published,
        gossip_received: gossip.received,
        gossip_last_error: gossip.last_error,
        vram_mb: state.vram_mb,
        max_jobs: state.max_jobs,
    })
}

async fn publish_capsule(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<impl IntoResponse, (StatusCode, Json<Value>)> {
    if body.get("rawLog").is_some()
        || body.get("rawLogs").is_some()
        || body.get("crashReport").is_some()
        || body.get("latestLog").is_some()
        || body.pointer("/privacy/rawLogs").and_then(|v| v.as_bool()) == Some(true)
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "raw logs are not accepted — ExperienceCapsule only"
            })),
        ));
    }
    if body.get("notes").is_some() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "author notes must not be published" })),
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
        let lib = state.library.lock().await;
        lib.publish(&capsule).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e })),
            )
        })?
    };

    let gossip = match state.p2p.publish_capsule(stored.clone()).await {
        Ok(()) => json!({ "ok": true }),
        Err(e) => {
            tracing::warn!(error = %e, "gossip publish failed (capsule still stored locally)");
            json!({ "ok": false, "error": e })
        }
    };

    Ok(Json(json!({
        "ok": true,
        "stored": true,
        "id": stored.id,
        "fingerprintKey": stored.fingerprint.key,
        "contentHash": stored.content_hash,
        "transport": "p2p+local",
        "gossip": gossip,
    })))
}

async fn lookup(
    State(state): State<AppState>,
    Json(req): Json<CrashLookupRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<Value>)> {
    let hay = req.excerpt.clone().unwrap_or_default();
    let limit = req.limit.max(1).min(20) as usize;
    let hits = {
        let lib = state.library.lock().await;
        lib.lookup(&req.fingerprint, &hay, limit)
    };
    Ok(Json(CrashLookupResponse {
        kb_version: Some(format!("p2p-{}", tuffbox_core::time_util::compact_now())),
        hits,
    }))
}

/// L2 Fog: route DiagnoseJob to a capable peer via libp2p request-response.
async fn diagnose_volunteer(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let job: DiagnoseJob = match serde_json::from_value(body) {
        Ok(j) => j,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "ok": false, "error": format!("invalid DiagnoseJob: {e}") })),
            );
        }
    };
    if let Err(e) = job.validate_size() {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(json!({ "ok": false, "error": e })),
        );
    }

    match state.p2p.diagnose_volunteer(job).await {
        Ok(result) => {
            let status = if result.ok {
                StatusCode::OK
            } else {
                StatusCode::SERVICE_UNAVAILABLE
            };
            (
                status,
                Json(serde_json::to_value(result).unwrap_or(json!({ "ok": false }))),
            )
        }
        Err(e) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "ok": false,
                "error": e,
            })),
        ),
    }
}

async fn jobs_pending(State(state): State<AppState>) -> impl IntoResponse {
    let mut jobs = state.pending_jobs.lock().await;
    match jobs.take_pending() {
        Some(job) => (
            StatusCode::OK,
            Json(serde_json::to_value(job).unwrap_or(json!({}))),
        )
            .into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

async fn jobs_complete(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(mut body): Json<Value>,
) -> impl IntoResponse {
    // Ensure jobId matches path.
    if body.get("jobId").and_then(|v| v.as_str()) != Some(id.as_str()) {
        body.as_object_mut()
            .map(|o| o.insert("jobId".into(), json!(id)));
    }
    let mut result: DiagnoseResult = match serde_json::from_value(body) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "ok": false, "error": format!("invalid DiagnoseResult: {e}") })),
            )
                .into_response();
        }
    };
    result.job_id = id;

    match state.p2p.complete_diagnose_job(result).await {
        Ok(()) => (StatusCode::OK, Json(json!({ "ok": true }))).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "ok": false, "error": e })),
        )
            .into_response(),
    }
}

/// Creation Marketplace: route CreationJob to a capable peer via libp2p request-response.
async fn creation_submit(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let job: CreationJob = match serde_json::from_value(body) {
        Ok(j) => j,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "ok": false, "error": format!("invalid CreationJob: {e}") })),
            );
        }
    };
    if let Err(e) = job.validate() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": e })),
        );
    }
    match state.p2p.submit_creation(job).await {
        Ok(result) => {
            let status = if result.ok {
                StatusCode::OK
            } else {
                StatusCode::SERVICE_UNAVAILABLE
            };
            (
                status,
                Json(serde_json::to_value(result).unwrap_or(json!({ "ok": false }))),
            )
        }
        Err(e) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "ok": false, "error": e })),
        ),
    }
}

async fn creation_jobs_pending(State(state): State<AppState>) -> impl IntoResponse {
    let mut jobs = state.pending_creation.lock().await;
    match jobs.take_pending() {
        Some(job) => (
            StatusCode::OK,
            Json(serde_json::to_value(job).unwrap_or(json!({}))),
        )
            .into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

async fn creation_jobs_complete(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(mut body): Json<Value>,
) -> impl IntoResponse {
    if body.get("jobId").and_then(|v| v.as_str()) != Some(id.as_str()) {
        body.as_object_mut()
            .map(|o| o.insert("jobId".into(), json!(id)));
    }
    let mut result: CreationResult = match serde_json::from_value(body) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "ok": false, "error": format!("invalid CreationResult: {e}") })),
            )
                .into_response();
        }
    };
    result.job_id = id;

    match state.p2p.complete_creation_job(result).await {
        Ok(()) => (StatusCode::OK, Json(json!({ "ok": true }))).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "ok": false, "error": e })),
        )
            .into_response(),
    }
}
