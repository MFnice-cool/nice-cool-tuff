//! libp2p behaviour: Kademlia + Gossipsub + mDNS + Identify + AutoNAT + Relay + DCUtR
//! + Fog diagnose request-response.

use anyhow::Context;
use ed25519_dalek::SigningKey;
use futures::StreamExt;
use libp2p::gossipsub::{self, IdentTopic, MessageAuthenticity};
use libp2p::identity::Keypair;
use libp2p::kad::{
    store::MemoryStore, Behaviour as KadBehaviour, Event as KadEvent, GetRecordOk, Mode,
    QueryResult, RecordKey,
};
use libp2p::mdns;
use libp2p::request_response::{self, OutboundRequestId, ProtocolSupport, ResponseChannel};
use libp2p::swarm::{behaviour::toggle::Toggle, NetworkBehaviour, SwarmEvent};
use libp2p::{autonat, dcutr, identify, relay, Multiaddr, PeerId, StreamProtocol, SwarmBuilder};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot, Mutex};
use tuffbox_core::swarm::{CapsuleLibrary, ExperienceCapsule, MAX_CAPSULE_GOSSIP_BYTES};

use crate::creation_jobs::{PendingCreationEntry, PendingCreationJobs};
use crate::diagnose::{DiagnoseJob, DiagnoseResult, DIAGNOSE_PROTOCOL};
use crate::jobs::{PendingEntry, PendingJobs};
use tuffbox_core::creation_marketplace::{CreationJob, CreationResult, CREATION_PROTOCOL};

pub const CAPSULE_TOPIC: &str = "tuffswarm/capsules/v1";
pub const CAPABILITY_PREFIX: &str = "tuffswarm/cap/v1/";
/// Max accepted gossip capsules per remote peer inside the sliding window.
const GOSSIP_RECV_MAX_PER_PEER: usize = 30;
const GOSSIP_RECV_WINDOW: Duration = Duration::from_secs(60);

#[derive(Debug, Default)]
pub struct GossipStats {
    published: AtomicU64,
    received: AtomicU64,
    last_error: StdMutex<String>,
}

impl GossipStats {
    pub fn snapshot(&self) -> GossipStatsSnapshot {
        GossipStatsSnapshot {
            published: self.published.load(Ordering::Relaxed),
            received: self.received.load(Ordering::Relaxed),
            last_error: self
                .last_error
                .lock()
                .map(|g| g.clone())
                .unwrap_or_default(),
        }
    }

    fn note_published(&self) {
        self.published.fetch_add(1, Ordering::Relaxed);
    }

    fn note_received(&self) {
        self.received.fetch_add(1, Ordering::Relaxed);
    }

    fn note_error(&self, err: impl Into<String>) {
        if let Ok(mut g) = self.last_error.lock() {
            *g = err.into();
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GossipStatsSnapshot {
    pub published: u64,
    pub received: u64,
    pub last_error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeCapability {
    pub vram_mb: u32,
    pub rtt_ms: u32,
    pub version: String,
    #[serde(default)]
    pub ollama_ready: bool,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default = "default_max_jobs")]
    pub max_jobs: u32,
    #[serde(default)]
    pub diagnose_volunteer: bool,
    #[serde(default)]
    pub creation_worker: bool,
}

fn default_max_jobs() -> u32 {
    1
}

/// Compact Identify `agent_version` so LAN peers learn volunteer status without waiting on DHT.
pub fn capability_agent_version(cap: &NodeCapability) -> String {
    format!(
        "tuffswarm/{};volunteer={};creation={};vram={};maxJobs={}",
        cap.version,
        if cap.diagnose_volunteer { 1 } else { 0 },
        if cap.creation_worker { 1 } else { 0 },
        cap.vram_mb,
        cap.max_jobs
    )
}

/// Parse Identify agent_version written by [`capability_agent_version`].
pub fn parse_capability_agent_version(agent: &str) -> Option<NodeCapability> {
    let rest = agent.strip_prefix("tuffswarm/")?;
    let mut version = String::new();
    let mut volunteer = false;
    let mut creation = false;
    let mut vram_mb = 0u32;
    let mut max_jobs = 1u32;
    for (i, part) in rest.split(';').enumerate() {
        if i == 0 {
            version = part.to_string();
            continue;
        }
        if let Some(v) = part.strip_prefix("volunteer=") {
            volunteer = v == "1" || v.eq_ignore_ascii_case("true");
        } else if let Some(v) = part.strip_prefix("creation=") {
            creation = v == "1" || v.eq_ignore_ascii_case("true");
        } else if let Some(v) = part.strip_prefix("vram=") {
            vram_mb = v.parse().unwrap_or(0);
        } else if let Some(v) = part.strip_prefix("maxJobs=") {
            max_jobs = v.parse().unwrap_or(1).max(1);
        }
    }
    if version.is_empty() {
        return None;
    }
    Some(NodeCapability {
        vram_mb,
        rtt_ms: 0,
        version,
        ollama_ready: volunteer,
        models: Vec::new(),
        max_jobs,
        diagnose_volunteer: volunteer,
        creation_worker: creation,
    })
}

fn fog_retryable_decline(result: &DiagnoseResult) -> bool {
    matches!(
        result.error.as_deref(),
        Some("not a volunteer") | Some("busy")
    )
}

fn creation_retryable_decline(result: &CreationResult) -> bool {
    matches!(
        result.error.as_deref(),
        Some("not a creation worker") | Some("busy")
    )
}

pub enum P2pCommand {
    PublishCapsule {
        capsule: ExperienceCapsule,
        reply: oneshot::Sender<Result<(), String>>,
    },
    PeerCount {
        reply: oneshot::Sender<usize>,
    },
    ListenAddrs {
        reply: oneshot::Sender<Vec<String>>,
    },
    DiagnoseVolunteer {
        job: DiagnoseJob,
        reply: oneshot::Sender<Result<DiagnoseResult, String>>,
    },
    ListVolunteerPeers {
        reply: oneshot::Sender<Vec<String>>,
    },
    /// Desktop finished a Fog job — complete the libp2p response channel.
    CompleteDiagnoseJob {
        result: DiagnoseResult,
        reply: oneshot::Sender<Result<(), String>>,
    },
    SubmitCreation {
        job: CreationJob,
        reply: oneshot::Sender<Result<CreationResult, String>>,
    },
    ListCreationPeers {
        reply: oneshot::Sender<Vec<String>>,
    },
    CompleteCreationJob {
        result: CreationResult,
        reply: oneshot::Sender<Result<(), String>>,
    },
}

pub enum P2pEvent {
    CapsuleReceived(ExperienceCapsule),
    PeerCount(usize),
}

struct PendingOutboundDiagnose {
    reply: oneshot::Sender<Result<DiagnoseResult, String>>,
    remaining: Vec<PeerId>,
    job: DiagnoseJob,
}

struct PendingOutboundCreation {
    reply: oneshot::Sender<Result<CreationResult, String>>,
    remaining: Vec<PeerId>,
    job: CreationJob,
}

#[derive(Clone)]
pub struct P2pHandle {
    pub cmd_tx: mpsc::Sender<P2pCommand>,
    pub gossip_stats: Arc<GossipStats>,
}

impl P2pHandle {
    pub fn gossip_stats_snapshot(&self) -> GossipStatsSnapshot {
        self.gossip_stats.snapshot()
    }

    pub async fn publish_capsule(&self, capsule: ExperienceCapsule) -> Result<(), String> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(P2pCommand::PublishCapsule { capsule, reply: tx })
            .await
            .map_err(|_| "p2p node command channel closed".to_string())?;
        rx.await
            .map_err(|_| "p2p node dropped publish reply".to_string())?
    }

    pub async fn peer_count(&self) -> usize {
        let (tx, rx) = oneshot::channel();
        if self
            .cmd_tx
            .send(P2pCommand::PeerCount { reply: tx })
            .await
            .is_err()
        {
            return 0;
        }
        rx.await.unwrap_or(0)
    }

    pub async fn listen_addrs(&self) -> Vec<String> {
        let (tx, rx) = oneshot::channel();
        if self
            .cmd_tx
            .send(P2pCommand::ListenAddrs { reply: tx })
            .await
            .is_err()
        {
            return Vec::new();
        }
        rx.await.unwrap_or_default()
    }

    pub async fn diagnose_volunteer(&self, job: DiagnoseJob) -> Result<DiagnoseResult, String> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(P2pCommand::DiagnoseVolunteer { job, reply: tx })
            .await
            .map_err(|_| "p2p node command channel closed".to_string())?;
        rx.await
            .map_err(|_| "p2p node dropped diagnose reply".to_string())?
    }

    pub async fn complete_diagnose_job(&self, result: DiagnoseResult) -> Result<(), String> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(P2pCommand::CompleteDiagnoseJob { result, reply: tx })
            .await
            .map_err(|_| "p2p node command channel closed".to_string())?;
        rx.await
            .map_err(|_| "p2p node dropped complete reply".to_string())?
    }

    pub async fn list_volunteer_peers(&self) -> Vec<String> {
        let (tx, rx) = oneshot::channel();
        if self
            .cmd_tx
            .send(P2pCommand::ListVolunteerPeers { reply: tx })
            .await
            .is_err()
        {
            return Vec::new();
        }
        rx.await.unwrap_or_default()
    }

    pub async fn submit_creation(&self, job: CreationJob) -> Result<CreationResult, String> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(P2pCommand::SubmitCreation { job, reply: tx })
            .await
            .map_err(|_| "p2p node command channel closed".to_string())?;
        rx.await
            .map_err(|_| "p2p node dropped creation reply".to_string())?
    }

    pub async fn complete_creation_job(&self, result: CreationResult) -> Result<(), String> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(P2pCommand::CompleteCreationJob { result, reply: tx })
            .await
            .map_err(|_| "p2p node command channel closed".to_string())?;
        rx.await
            .map_err(|_| "p2p node dropped creation complete reply".to_string())?
    }

    pub async fn list_creation_peers(&self) -> Vec<String> {
        let (tx, rx) = oneshot::channel();
        if self
            .cmd_tx
            .send(P2pCommand::ListCreationPeers { reply: tx })
            .await
            .is_err()
        {
            return Vec::new();
        }
        rx.await.unwrap_or_default()
    }
}

#[derive(NetworkBehaviour)]
struct SwarmBehaviour {
    gossipsub: gossipsub::Behaviour,
    kad: KadBehaviour<MemoryStore>,
    mdns: mdns::tokio::Behaviour,
    identify: identify::Behaviour,
    relay_client: relay::client::Behaviour,
    autonat: autonat::Behaviour,
    dcutr: dcutr::Behaviour,
    relay_server: Toggle<relay::Behaviour>,
    diagnose: request_response::json::Behaviour<DiagnoseJob, DiagnoseResult>,
    creation: request_response::json::Behaviour<CreationJob, CreationResult>,
}

fn signing_key_from_libp2p(id_keys: &Keypair) -> anyhow::Result<SigningKey> {
    let ed = id_keys
        .clone()
        .try_into_ed25519()
        .map_err(|_| anyhow::anyhow!("expected ed25519 identity key"))?;
    let secret: [u8; 32] = ed.secret().as_ref().try_into()?;
    Ok(SigningKey::from_bytes(&secret))
}

pub struct SwarmOpts {
    pub listen: String,
    pub bootstraps: Vec<String>,
    pub capability: NodeCapability,
    pub relay_server: bool,
}

pub async fn run_swarm(
    opts: SwarmOpts,
    library: Arc<Mutex<CapsuleLibrary>>,
    pending_jobs: Arc<Mutex<PendingJobs>>,
    pending_creation: Arc<Mutex<PendingCreationJobs>>,
    mut cmd_rx: mpsc::Receiver<P2pCommand>,
    event_tx: mpsc::Sender<P2pEvent>,
    gossip_stats: Arc<GossipStats>,
) -> anyhow::Result<()> {
    let id_keys = Keypair::generate_ed25519();
    let peer_id = PeerId::from(id_keys.public());
    let signing_key = signing_key_from_libp2p(&id_keys)?;
    let peer_id_str = peer_id.to_string();
    let local_is_volunteer = opts.capability.diagnose_volunteer;
    let local_is_creation_worker = opts.capability.creation_worker;
    let max_jobs = opts.capability.max_jobs;
    tracing::info!(
        %peer_id,
        relay_server = opts.relay_server,
        diagnose_volunteer = local_is_volunteer,
        creation_worker = local_is_creation_worker,
        "local peer id"
    );

    let message_id_fn = |message: &gossipsub::Message| {
        let mut hasher = DefaultHasher::new();
        message.data.hash(&mut hasher);
        gossipsub::MessageId::from(hasher.finish().to_string())
    };
    let gossipsub_config = gossipsub::ConfigBuilder::default()
        .heartbeat_interval(Duration::from_secs(10))
        .validation_mode(gossipsub::ValidationMode::Permissive)
        .max_transmit_size(MAX_CAPSULE_GOSSIP_BYTES)
        .message_id_fn(message_id_fn)
        .build()
        .map_err(|e| anyhow::anyhow!("gossipsub config: {e}"))?;

    let mut gossipsub = gossipsub::Behaviour::new(
        MessageAuthenticity::Signed(id_keys.clone()),
        gossipsub_config,
    )
    .map_err(|e| anyhow::anyhow!("gossipsub: {e}"))?;
    let topic = IdentTopic::new(CAPSULE_TOPIC);
    gossipsub
        .subscribe(&topic)
        .map_err(|e| anyhow::anyhow!("subscribe: {e}"))?;

    let store = MemoryStore::new(peer_id);
    let mut kad = KadBehaviour::new(peer_id, store);
    kad.set_mode(Some(Mode::Server));

    let mdns =
        mdns::tokio::Behaviour::new(mdns::Config::default(), peer_id).context("mdns behaviour")?;
    let identify = identify::Behaviour::new(
        identify::Config::new("/tuffswarm/1.0.0".into(), id_keys.public())
            .with_agent_version(capability_agent_version(&opts.capability)),
    );
    let autonat = autonat::Behaviour::new(peer_id, Default::default());
    let dcutr = dcutr::Behaviour::new(peer_id);
    let relay_server: Toggle<relay::Behaviour> = if opts.relay_server {
        Toggle::from(Some(relay::Behaviour::new(peer_id, Default::default())))
    } else {
        Toggle::from(None)
    };

    let mut rr_cfg = request_response::Config::default();
    rr_cfg = rr_cfg.with_request_timeout(Duration::from_secs(50));
    let diagnose = request_response::json::Behaviour::new(
        [(
            StreamProtocol::new(DIAGNOSE_PROTOCOL),
            ProtocolSupport::Full,
        )],
        rr_cfg.clone(),
    );
    let mut creation_rr_cfg = request_response::Config::default();
    creation_rr_cfg = creation_rr_cfg.with_request_timeout(Duration::from_secs(130));
    let creation = request_response::json::Behaviour::new(
        [(
            StreamProtocol::new(CREATION_PROTOCOL),
            ProtocolSupport::Full,
        )],
        creation_rr_cfg,
    );

    let mut swarm = SwarmBuilder::with_existing_identity(id_keys)
        .with_tokio()
        .with_tcp(
            libp2p::tcp::Config::default(),
            libp2p::noise::Config::new,
            libp2p::yamux::Config::default,
        )?
        .with_relay_client(libp2p::noise::Config::new, libp2p::yamux::Config::default)?
        .with_behaviour(|_key, relay_client| {
            Ok(SwarmBehaviour {
                gossipsub,
                kad,
                mdns,
                identify,
                relay_client,
                autonat,
                dcutr,
                relay_server,
                diagnose,
                creation,
            })
        })?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
        .build();

    let listen_addr: Multiaddr = opts.listen.parse().context("invalid --listen multiaddr")?;
    swarm.listen_on(listen_addr)?;
    if opts.relay_server {
        tracing::info!("relay server mode enabled — publish this node's multiaddr as --bootstrap for NAT peers");
    }

    for boot in &opts.bootstraps {
        let addr: Multiaddr = boot
            .parse()
            .with_context(|| format!("invalid bootstrap multiaddr: {boot}"))?;
        let boot_peer = extract_peer_id(&addr).unwrap_or(peer_id);
        swarm
            .behaviour_mut()
            .kad
            .add_address(&boot_peer, addr.clone());
        swarm.listen_on(addr.clone().with(libp2p::multiaddr::Protocol::P2pCircuit))?;
        if let Err(e) = swarm.dial(addr.clone()) {
            tracing::warn!(error = %e, %addr, "bootstrap dial failed");
        } else {
            tracing::info!(%addr, "dialing bootstrap");
        }
    }

    let cap_key = RecordKey::new(&format!("{CAPABILITY_PREFIX}{peer_id}"));
    let cap_bytes = serde_json::to_vec(&opts.capability)?;
    let record = libp2p::kad::Record {
        key: cap_key,
        value: cap_bytes,
        publisher: Some(peer_id),
        expires: None,
    };
    let _ = swarm
        .behaviour_mut()
        .kad
        .put_record(record, libp2p::kad::Quorum::One);

    {
        let lib = library.lock().await;
        for mut capsule in lib.load_all().into_iter().rev().take(16) {
            capsule = capsule.sanitized_for_network();
            if capsule.sign_ed25519(&signing_key, &peer_id_str).is_err() {
                continue;
            }
            let bytes = match serde_json::to_vec(&capsule.to_public_json()) {
                Ok(b) if b.len() <= MAX_CAPSULE_GOSSIP_BYTES => b,
                _ => continue,
            };
            let _ = swarm
                .behaviour_mut()
                .gossipsub
                .publish(topic.clone(), bytes);
        }
    }

    let mut connected: HashSet<PeerId> = HashSet::new();
    let mut listen_addrs: Vec<String> = Vec::new();
    let mut peer_caps: HashMap<PeerId, NodeCapability> = HashMap::new();
    peer_caps.insert(peer_id, opts.capability.clone());
    let mut outbound_pending: HashMap<OutboundRequestId, PendingOutboundDiagnose> = HashMap::new();
    let mut outbound_creation: HashMap<OutboundRequestId, PendingOutboundCreation> = HashMap::new();
    let mut gossip_recv_window: HashMap<PeerId, VecDeque<Instant>> = HashMap::new();
    let mut expire_tick = tokio::time::interval(Duration::from_secs(5));
    let mut cap_refresh = tokio::time::interval(Duration::from_secs(20));
    // First tick fires immediately — skip so we don't hammer DHT on boot.
    cap_refresh.tick().await;

    loop {
        tokio::select! {
            _ = expire_tick.tick() => {
                let expired = {
                    let mut jobs = pending_jobs.lock().await;
                    jobs.expire_overdue()
                };
                for entry in expired {
                    let job_id = entry.job.job_id.clone();
                    let _ = swarm.behaviour_mut().diagnose.send_response(
                        entry.channel,
                        DiagnoseResult::err(job_id, "timeout"),
                    );
                }
                let expired_c = {
                    let mut jobs = pending_creation.lock().await;
                    jobs.expire_overdue()
                };
                for entry in expired_c {
                    let job_id = entry.job.job_id.clone();
                    let _ = swarm.behaviour_mut().creation.send_response(
                        entry.channel,
                        CreationResult::err(job_id, "timeout"),
                    );
                }
            }
            _ = cap_refresh.tick() => {
                // Re-fetch DHT capability for connected peers we still don't know.
                for remote in connected.iter().filter(|p| *p != &peer_id) {
                    if peer_caps.contains_key(remote) {
                        continue;
                    }
                    let key = RecordKey::new(&format!("{CAPABILITY_PREFIX}{remote}"));
                    let _ = swarm.behaviour_mut().kad.get_record(key);
                }
                // Refresh our own capability record (volunteer flag may change via restart).
                let cap_key = RecordKey::new(&format!("{CAPABILITY_PREFIX}{peer_id}"));
                let cap_bytes = match serde_json::to_vec(&opts.capability) {
                    Ok(b) => b,
                    Err(_) => continue,
                };
                let record = libp2p::kad::Record {
                    key: cap_key,
                    value: cap_bytes,
                    publisher: Some(peer_id),
                    expires: None,
                };
                let _ = swarm
                    .behaviour_mut()
                    .kad
                    .put_record(record, libp2p::kad::Quorum::One);
            }
            cmd = cmd_rx.recv() => {
                let Some(cmd) = cmd else { break; };
                match cmd {
                    P2pCommand::PublishCapsule { capsule, reply } => {
                        let mut capsule = capsule.sanitized_for_network();
                        if let Err(e) = capsule.sign_ed25519(&signing_key, &peer_id_str) {
                            gossip_stats.note_error(e.clone());
                            let _ = reply.send(Err(e));
                            continue;
                        }
                        let bytes = match serde_json::to_vec(&capsule.to_public_json()) {
                            Ok(b) => b,
                            Err(e) => {
                                gossip_stats.note_error(e.to_string());
                                let _ = reply.send(Err(e.to_string()));
                                continue;
                            }
                        };
                        if bytes.len() > MAX_CAPSULE_GOSSIP_BYTES {
                            let err = format!(
                                "capsule exceeds max gossip size ({MAX_CAPSULE_GOSSIP_BYTES} bytes)"
                            );
                            gossip_stats.note_error(err.clone());
                            let _ = reply.send(Err(err));
                            continue;
                        }
                        let content_key = RecordKey::new(&capsule.dht_content_key());
                        let _ = swarm.behaviour_mut().kad.start_providing(content_key);
                        let fp_key = RecordKey::new(&format!(
                            "tuffswarm/fp/v1/{}",
                            capsule.fingerprint.key
                        ));
                        let _ = swarm.behaviour_mut().kad.start_providing(fp_key);

                        let result = swarm
                            .behaviour_mut()
                            .gossipsub
                            .publish(topic.clone(), bytes)
                            .map(|_| ())
                            .map_err(|e| format!("gossip publish: {e}"));
                        match &result {
                            Ok(()) => gossip_stats.note_published(),
                            Err(e) => gossip_stats.note_error(e.clone()),
                        }
                        let _ = reply.send(result);
                    }
                    P2pCommand::PeerCount { reply } => {
                        let _ = reply.send(connected.len());
                    }
                    P2pCommand::ListenAddrs { reply } => {
                        let _ = reply.send(listen_addrs.clone());
                    }
                    P2pCommand::ListVolunteerPeers { reply } => {
                        let list = pick_volunteer_peers(&connected, &peer_caps, &peer_id)
                            .into_iter()
                            .map(|p| p.to_string())
                            .collect();
                        let _ = reply.send(list);
                    }
                    P2pCommand::DiagnoseVolunteer { job, reply } => {
                        if let Err(e) = job.validate_size() {
                            let _ = reply.send(Err(e));
                            continue;
                        }
                        let candidates = pick_volunteer_peers(&connected, &peer_caps, &peer_id);
                        dispatch_diagnose_request(
                            &mut swarm,
                            &mut outbound_pending,
                            PendingOutboundDiagnose {
                                reply,
                                remaining: candidates,
                                job,
                            },
                        );
                    }
                    P2pCommand::CompleteDiagnoseJob { mut result, reply } => {
                        let mut jobs = pending_jobs.lock().await;
                        match jobs.complete(&result.job_id) {
                            Some(entry) => {
                                if result.worker_peer_id.is_none() {
                                    result.worker_peer_id = Some(peer_id_str.clone());
                                }
                                let ok = swarm
                                    .behaviour_mut()
                                    .diagnose
                                    .send_response(entry.channel, result)
                                    .map_err(|e| format!("send_response: {e:?}"));
                                let _ = reply.send(ok.map(|_| ()));
                            }
                            None => {
                                let _ = reply.send(Err(format!(
                                    "no inflight job {}",
                                    result.job_id
                                )));
                            }
                        }
                    }
                    P2pCommand::ListCreationPeers { reply } => {
                        let list = pick_creation_peers(&connected, &peer_caps, &peer_id)
                            .into_iter()
                            .map(|p| p.to_string())
                            .collect();
                        let _ = reply.send(list);
                    }
                    P2pCommand::SubmitCreation { job, reply } => {
                        if let Err(e) = job.validate() {
                            let _ = reply.send(Err(e));
                            continue;
                        }
                        let candidates = pick_creation_peers(&connected, &peer_caps, &peer_id);
                        dispatch_creation_request(
                            &mut swarm,
                            &mut outbound_creation,
                            PendingOutboundCreation {
                                reply,
                                remaining: candidates,
                                job,
                            },
                        );
                    }
                    P2pCommand::CompleteCreationJob { mut result, reply } => {
                        let mut jobs = pending_creation.lock().await;
                        match jobs.complete(&result.job_id) {
                            Some(entry) => {
                                if result.worker_peer_id.is_none() {
                                    result.worker_peer_id = Some(peer_id_str.clone());
                                }
                                let ok = swarm
                                    .behaviour_mut()
                                    .creation
                                    .send_response(entry.channel, result)
                                    .map_err(|e| format!("send_response: {e:?}"));
                                let _ = reply.send(ok.map(|_| ()));
                            }
                            None => {
                                let _ = reply.send(Err(format!(
                                    "no inflight creation job {}",
                                    result.job_id
                                )));
                            }
                        }
                    }
                }
            }
            event = swarm.select_next_some() => {
                match event {
                    SwarmEvent::NewListenAddr { address, .. } => {
                        let full = format!("{address}/p2p/{peer_id}");
                        tracing::info!(%full, "listening");
                        listen_addrs.push(full);
                    }
                    SwarmEvent::ConnectionEstablished { peer_id: remote, .. } => {
                        connected.insert(remote);
                        let _ = event_tx.send(P2pEvent::PeerCount(connected.len())).await;
                        swarm.behaviour_mut().gossipsub.add_explicit_peer(&remote);
                        let key = RecordKey::new(&format!("{CAPABILITY_PREFIX}{remote}"));
                        let _ = swarm.behaviour_mut().kad.get_record(key);
                    }
                    SwarmEvent::ConnectionClosed { peer_id: remote, .. } => {
                        connected.remove(&remote);
                        peer_caps.remove(&remote);
                        gossip_recv_window.remove(&remote);
                        let _ = event_tx.send(P2pEvent::PeerCount(connected.len())).await;
                        swarm.behaviour_mut().gossipsub.remove_explicit_peer(&remote);
                    }
                    SwarmEvent::Behaviour(SwarmBehaviourEvent::Mdns(mdns::Event::Discovered(list))) => {
                        for (peer, addr) in list {
                            tracing::info!(%peer, %addr, "mDNS discovered");
                            swarm.behaviour_mut().kad.add_address(&peer, addr);
                            swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer);
                            let key = RecordKey::new(&format!("{CAPABILITY_PREFIX}{peer}"));
                            let _ = swarm.behaviour_mut().kad.get_record(key);
                        }
                    }
                    SwarmEvent::Behaviour(SwarmBehaviourEvent::Gossipsub(
                        gossipsub::Event::Message { message, propagation_source, .. }
                    )) => {
                        if message.data.len() > MAX_CAPSULE_GOSSIP_BYTES {
                            tracing::warn!("dropping oversized gossip message");
                            gossip_stats.note_error("oversized gossip message dropped");
                            continue;
                        }
                        let rate_peer = message.source.unwrap_or(propagation_source);
                        if !allow_gossip_recv(&mut gossip_recv_window, rate_peer) {
                            tracing::debug!(%rate_peer, "gossip receive rate-limited");
                            gossip_stats.note_error(format!(
                                "rate-limited peer {rate_peer} (max {GOSSIP_RECV_MAX_PER_PEER}/{GOSSIP_RECV_WINDOW:?})"
                            ));
                            continue;
                        }
                        if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&message.data) {
                            if let Ok(capsule) = ExperienceCapsule::from_public_value(&value) {
                                match capsule.accept_for_p2p_gossip() {
                                    Ok(()) => {
                                        gossip_stats.note_received();
                                        let _ = event_tx.send(P2pEvent::CapsuleReceived(capsule)).await;
                                    }
                                    Err(e) => {
                                        gossip_stats.note_error(e.clone());
                                        tracing::debug!(error = %e, "dropped unsigned/invalid gossip capsule");
                                    }
                                }
                            }
                        }
                    }
                    SwarmEvent::Behaviour(SwarmBehaviourEvent::Kad(KadEvent::OutboundQueryProgressed {
                        result: QueryResult::GetRecord(Ok(GetRecordOk::FoundRecord(rec))),
                        ..
                    })) => {
                        if let Ok(cap) = serde_json::from_slice::<NodeCapability>(&rec.record.value) {
                            if let Some(publisher) = rec.record.publisher {
                                peer_caps.insert(publisher, cap);
                            } else if let Some(pid) = peer_id_from_cap_key(&rec.record.key) {
                                peer_caps.insert(pid, cap);
                            }
                        }
                    }
                    SwarmEvent::Behaviour(SwarmBehaviourEvent::Kad(KadEvent::RoutingUpdated { peer, .. })) => {
                        tracing::debug!(%peer, "kad routing updated");
                    }
                    SwarmEvent::Behaviour(SwarmBehaviourEvent::Identify(identify::Event::Received {
                        peer_id: remote,
                        info,
                        ..
                    })) => {
                        for addr in info.listen_addrs {
                            swarm.behaviour_mut().kad.add_address(&remote, addr);
                        }
                        if let Some(cap) = parse_capability_agent_version(&info.agent_version) {
                            tracing::debug!(
                                %remote,
                                volunteer = cap.diagnose_volunteer,
                                "cached capability from identify"
                            );
                            peer_caps.insert(remote, cap);
                        }
                    }
                    SwarmEvent::Behaviour(SwarmBehaviourEvent::Diagnose(
                        request_response::Event::Message { peer, message, .. }
                    )) => {
                        match message {
                            request_response::Message::Request { request, channel, .. } => {
                                handle_inbound_diagnose(
                                    &mut swarm,
                                    &pending_jobs,
                                    local_is_volunteer,
                                    max_jobs,
                                    peer,
                                    request,
                                    channel,
                                ).await;
                            }
                            request_response::Message::Response { request_id, response } => {
                                if let Some(pending) = outbound_pending.remove(&request_id) {
                                    if !response.ok
                                        && fog_retryable_decline(&response)
                                        && !pending.remaining.is_empty()
                                    {
                                        if response.error.as_deref() == Some("not a volunteer") {
                                            peer_caps
                                                .entry(peer)
                                                .and_modify(|c| c.diagnose_volunteer = false)
                                                .or_insert_with(|| NodeCapability {
                                                    vram_mb: 0,
                                                    rtt_ms: 0,
                                                    version: "unknown".into(),
                                                    ollama_ready: false,
                                                    models: Vec::new(),
                                                    max_jobs: 1,
                                                    diagnose_volunteer: false,
                                                    creation_worker: false,
                                                });
                                        }
                                        tracing::debug!(
                                            %peer,
                                            error = ?response.error,
                                            remaining = pending.remaining.len(),
                                            "fog decline — trying next peer"
                                        );
                                        dispatch_diagnose_request(
                                            &mut swarm,
                                            &mut outbound_pending,
                                            pending,
                                        );
                                    } else {
                                        let _ = pending.reply.send(Ok(response));
                                    }
                                }
                            }
                        }
                    }
                    SwarmEvent::Behaviour(SwarmBehaviourEvent::Diagnose(
                        request_response::Event::OutboundFailure { request_id, error, .. }
                    )) => {
                        if let Some(pending) = outbound_pending.remove(&request_id) {
                            if !pending.remaining.is_empty() {
                                tracing::debug!(
                                    %error,
                                    remaining = pending.remaining.len(),
                                    "fog outbound failure — trying next peer"
                                );
                                dispatch_diagnose_request(
                                    &mut swarm,
                                    &mut outbound_pending,
                                    pending,
                                );
                            } else {
                                let _ = pending
                                    .reply
                                    .send(Err(format!("fog outbound failure: {error}")));
                            }
                        }
                    }
                    SwarmEvent::Behaviour(SwarmBehaviourEvent::Diagnose(
                        request_response::Event::InboundFailure { error, .. }
                    )) => {
                        tracing::warn!(%error, "fog inbound failure");
                    }
                    SwarmEvent::Behaviour(SwarmBehaviourEvent::Creation(
                        request_response::Event::Message { peer, message, .. }
                    )) => {
                        match message {
                            request_response::Message::Request { request, channel, .. } => {
                                handle_inbound_creation(
                                    &mut swarm,
                                    &pending_creation,
                                    local_is_creation_worker,
                                    peer,
                                    request,
                                    channel,
                                )
                                .await;
                            }
                            request_response::Message::Response { request_id, response } => {
                                if let Some(pending) = outbound_creation.remove(&request_id) {
                                    if !response.ok
                                        && creation_retryable_decline(&response)
                                        && !pending.remaining.is_empty()
                                    {
                                        if response.error.as_deref() == Some("not a creation worker")
                                        {
                                            peer_caps.entry(peer).and_modify(|c| {
                                                c.creation_worker = false;
                                            });
                                        }
                                        dispatch_creation_request(
                                            &mut swarm,
                                            &mut outbound_creation,
                                            pending,
                                        );
                                    } else {
                                        let _ = pending.reply.send(Ok(response));
                                    }
                                }
                            }
                        }
                    }
                    SwarmEvent::Behaviour(SwarmBehaviourEvent::Creation(
                        request_response::Event::OutboundFailure { request_id, error, .. }
                    )) => {
                        if let Some(pending) = outbound_creation.remove(&request_id) {
                            if !pending.remaining.is_empty() {
                                dispatch_creation_request(
                                    &mut swarm,
                                    &mut outbound_creation,
                                    pending,
                                );
                            } else {
                                let _ = pending
                                    .reply
                                    .send(Err(format!("creation outbound failure: {error}")));
                            }
                        }
                    }
                    SwarmEvent::Behaviour(SwarmBehaviourEvent::Creation(
                        request_response::Event::InboundFailure { error, .. }
                    )) => {
                        tracing::warn!(%error, "creation inbound failure");
                    }
                    SwarmEvent::Behaviour(SwarmBehaviourEvent::Autonat(ev)) => {
                        tracing::debug!(?ev, "autonat");
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

fn dispatch_diagnose_request(
    swarm: &mut libp2p::Swarm<SwarmBehaviour>,
    outbound_pending: &mut HashMap<OutboundRequestId, PendingOutboundDiagnose>,
    pending: PendingOutboundDiagnose,
) {
    let PendingOutboundDiagnose {
        reply,
        mut remaining,
        job,
    } = pending;
    while let Some(target) = remaining.first().copied() {
        remaining.remove(0);
        let req_id = swarm
            .behaviour_mut()
            .diagnose
            .send_request(&target, job.clone());
        outbound_pending.insert(
            req_id,
            PendingOutboundDiagnose {
                reply,
                remaining,
                job,
            },
        );
        return;
    }
    let _ = reply.send(Err("no capable volunteer peers".into()));
}

async fn handle_inbound_diagnose(
    swarm: &mut libp2p::Swarm<SwarmBehaviour>,
    pending_jobs: &Arc<Mutex<PendingJobs>>,
    local_is_volunteer: bool,
    max_jobs: u32,
    peer: PeerId,
    request: DiagnoseJob,
    channel: ResponseChannel<DiagnoseResult>,
) {
    let job_id = request.job_id.clone();
    if !local_is_volunteer {
        let _ = swarm
            .behaviour_mut()
            .diagnose
            .send_response(channel, DiagnoseResult::err(job_id, "not a volunteer"));
        return;
    }
    if let Err(e) = request.validate_size() {
        let _ = swarm
            .behaviour_mut()
            .diagnose
            .send_response(channel, DiagnoseResult::err(job_id, e));
        return;
    }

    let entry = PendingEntry {
        job: request,
        channel,
        enqueued_at: Instant::now(),
    };
    let mut jobs = pending_jobs.lock().await;
    // Ensure max_jobs matches capability (PendingJobs created with same value).
    let _ = max_jobs;
    match jobs.enqueue(entry) {
        Ok(()) => {
            tracing::info!(%peer, %job_id, "fog diagnose job queued for desktop");
        }
        Err(entry) => {
            let _ = swarm
                .behaviour_mut()
                .diagnose
                .send_response(entry.channel, DiagnoseResult::err(job_id, "busy"));
        }
    }
}

fn dispatch_creation_request(
    swarm: &mut libp2p::Swarm<SwarmBehaviour>,
    outbound: &mut HashMap<OutboundRequestId, PendingOutboundCreation>,
    pending: PendingOutboundCreation,
) {
    let PendingOutboundCreation {
        reply,
        mut remaining,
        job,
    } = pending;
    while let Some(target) = remaining.first().copied() {
        remaining.remove(0);
        let req_id = swarm
            .behaviour_mut()
            .creation
            .send_request(&target, job.clone());
        outbound.insert(
            req_id,
            PendingOutboundCreation {
                reply,
                remaining,
                job,
            },
        );
        return;
    }
    let _ = reply.send(Err("no capable creation workers".into()));
}

async fn handle_inbound_creation(
    swarm: &mut libp2p::Swarm<SwarmBehaviour>,
    pending_creation: &Arc<Mutex<PendingCreationJobs>>,
    local_is_creation_worker: bool,
    peer: PeerId,
    request: CreationJob,
    channel: ResponseChannel<CreationResult>,
) {
    let job_id = request.job_id.clone();
    if !local_is_creation_worker {
        let _ = swarm.behaviour_mut().creation.send_response(
            channel,
            CreationResult::err(job_id, "not a creation worker"),
        );
        return;
    }
    if let Err(e) = request.validate() {
        let _ = swarm
            .behaviour_mut()
            .creation
            .send_response(channel, CreationResult::err(job_id, e));
        return;
    }

    let entry = PendingCreationEntry {
        job: request,
        channel,
        enqueued_at: Instant::now(),
    };
    let mut jobs = pending_creation.lock().await;
    match jobs.enqueue(entry) {
        Ok(()) => {
            tracing::info!(%peer, %job_id, "creation job queued for desktop");
        }
        Err(entry) => {
            let _ = swarm
                .behaviour_mut()
                .creation
                .send_response(entry.channel, CreationResult::err(job_id, "busy"));
        }
    }
}

fn pick_creation_peers(
    connected: &HashSet<PeerId>,
    caps: &HashMap<PeerId, NodeCapability>,
    self_id: &PeerId,
) -> Vec<PeerId> {
    let mut peers: Vec<(u32, u32, PeerId)> = connected
        .iter()
        .filter(|p| *p != self_id)
        .filter_map(|p| {
            let cap = caps.get(p)?;
            if cap.creation_worker {
                Some((cap.vram_mb, cap.rtt_ms, *p))
            } else {
                None
            }
        })
        .collect();
    if peers.is_empty() {
        return connected
            .iter()
            .filter(|p| *p != self_id && !caps.contains_key(p))
            .copied()
            .collect();
    }
    // Prefer higher stub VRAM, then lower RTT.
    peers.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    peers.into_iter().map(|(_, _, p)| p).collect()
}

fn pick_volunteer_peers(
    connected: &HashSet<PeerId>,
    caps: &HashMap<PeerId, NodeCapability>,
    self_id: &PeerId,
) -> Vec<PeerId> {
    let mut peers: Vec<(u32, PeerId)> = connected
        .iter()
        .filter(|p| *p != self_id)
        .filter_map(|p| {
            let cap = caps.get(p)?;
            if cap.diagnose_volunteer {
                Some((cap.rtt_ms, *p))
            } else {
                None
            }
        })
        .collect();
    if peers.is_empty() {
        // Prefer peers whose capability is still unknown (Identify/DHT pending).
        // Never spray known non-volunteers — that was the main source of
        // "not a volunteer" noise on LAN.
        return connected
            .iter()
            .filter(|p| *p != self_id && !caps.contains_key(p))
            .copied()
            .collect();
    }
    peers.sort_by_key(|(rtt, _)| *rtt);
    peers.into_iter().map(|(_, p)| p).collect()
}

fn allow_gossip_recv(windows: &mut HashMap<PeerId, VecDeque<Instant>>, peer: PeerId) -> bool {
    let now = Instant::now();
    let q = windows.entry(peer).or_default();
    while let Some(front) = q.front() {
        if now.duration_since(*front) > GOSSIP_RECV_WINDOW {
            q.pop_front();
        } else {
            break;
        }
    }
    if q.len() >= GOSSIP_RECV_MAX_PER_PEER {
        return false;
    }
    q.push_back(now);
    true
}

fn peer_id_from_cap_key(key: &RecordKey) -> Option<PeerId> {
    let s = String::from_utf8_lossy(key.as_ref());
    let id = s.strip_prefix(CAPABILITY_PREFIX)?;
    id.parse().ok()
}

fn extract_peer_id(addr: &Multiaddr) -> Option<PeerId> {
    addr.iter().find_map(|p| match p {
        libp2p::multiaddr::Protocol::P2p(peer) => Some(peer),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_peer() -> PeerId {
        PeerId::from(Keypair::generate_ed25519().public())
    }

    #[test]
    fn agent_version_roundtrip_volunteer() {
        let cap = NodeCapability {
            vram_mb: 8192,
            rtt_ms: 12,
            version: "0.1.0".into(),
            ollama_ready: true,
            models: vec![],
            max_jobs: 2,
            diagnose_volunteer: true,
            creation_worker: true,
        };
        let agent = capability_agent_version(&cap);
        let parsed = parse_capability_agent_version(&agent).expect("parse");
        assert!(parsed.diagnose_volunteer);
        assert!(parsed.creation_worker);
        assert_eq!(parsed.vram_mb, 8192);
        assert_eq!(parsed.max_jobs, 2);
        assert_eq!(parsed.version, "0.1.0");
    }

    #[test]
    fn pick_skips_known_non_volunteers() {
        let self_id = dummy_peer();
        let volunteer = dummy_peer();
        let non_vol = dummy_peer();
        let unknown = dummy_peer();
        let mut connected = HashSet::new();
        connected.insert(self_id);
        connected.insert(volunteer);
        connected.insert(non_vol);
        connected.insert(unknown);
        let mut caps = HashMap::new();
        caps.insert(
            volunteer,
            NodeCapability {
                vram_mb: 1,
                rtt_ms: 5,
                version: "t".into(),
                ollama_ready: true,
                models: vec![],
                max_jobs: 1,
                diagnose_volunteer: true,
                creation_worker: false,
            },
        );
        caps.insert(
            non_vol,
            NodeCapability {
                vram_mb: 0,
                rtt_ms: 1,
                version: "t".into(),
                ollama_ready: false,
                models: vec![],
                max_jobs: 1,
                diagnose_volunteer: false,
                creation_worker: false,
            },
        );
        let picked = pick_volunteer_peers(&connected, &caps, &self_id);
        assert_eq!(picked, vec![volunteer]);
    }

    #[test]
    fn pick_fallback_only_unknown_caps() {
        let self_id = dummy_peer();
        let non_vol = dummy_peer();
        let unknown = dummy_peer();
        let mut connected = HashSet::new();
        connected.insert(self_id);
        connected.insert(non_vol);
        connected.insert(unknown);
        let mut caps = HashMap::new();
        caps.insert(
            non_vol,
            NodeCapability {
                vram_mb: 0,
                rtt_ms: 1,
                version: "t".into(),
                ollama_ready: false,
                models: vec![],
                max_jobs: 1,
                diagnose_volunteer: false,
                creation_worker: false,
            },
        );
        let picked = pick_volunteer_peers(&connected, &caps, &self_id);
        assert_eq!(picked, vec![unknown]);
    }

    fn creation_cap(vram_mb: u32, rtt_ms: u32) -> NodeCapability {
        NodeCapability {
            vram_mb,
            rtt_ms,
            version: "t".into(),
            ollama_ready: false,
            models: vec![],
            max_jobs: 1,
            diagnose_volunteer: false,
            creation_worker: true,
        }
    }

    #[test]
    fn pick_creation_prefers_higher_vram() {
        let self_id = dummy_peer();
        let low = dummy_peer();
        let mid = dummy_peer();
        let high = dummy_peer();
        let mut connected = HashSet::new();
        connected.insert(self_id);
        connected.insert(low);
        connected.insert(mid);
        connected.insert(high);
        let mut caps = HashMap::new();
        caps.insert(low, creation_cap(0, 1));
        caps.insert(mid, creation_cap(4096, 50));
        caps.insert(high, creation_cap(8192, 100));
        let picked = pick_creation_peers(&connected, &caps, &self_id);
        assert_eq!(picked, vec![high, mid, low]);
    }

    #[test]
    fn pick_creation_vram_tie_breaks_on_rtt() {
        let self_id = dummy_peer();
        let slow = dummy_peer();
        let fast = dummy_peer();
        let mut connected = HashSet::new();
        connected.insert(self_id);
        connected.insert(slow);
        connected.insert(fast);
        let mut caps = HashMap::new();
        caps.insert(slow, creation_cap(4096, 80));
        caps.insert(fast, creation_cap(4096, 10));
        let picked = pick_creation_peers(&connected, &caps, &self_id);
        assert_eq!(picked, vec![fast, slow]);
    }
}
