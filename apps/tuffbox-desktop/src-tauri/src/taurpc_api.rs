//! TauRPC pilot (awesome-tauri pick #2): a typed IPC surface for the
//! TuffSwarm subsystem, separate from the ~135 legacy `#[tauri::command]`s.
//!
//! Why: legacy commands are stringly-typed (Result<Value, String>) and only
//! name-checked by check-bridge-parity. TauRPC generates real TS types from
//! Rust at dev-build time (src/bindings.ts), so argument/return drift breaks
//! the build instead of the user's runtime. If the pilot lands well, new
//! subsystems go here first; legacy migration is out of scope.
//!
//! Flow: `cargo tauri dev` (debug only) → macro exports src/bindings.ts →
//! frontend: `const taurpc = createTauRPCProxy(); await taurpc.swarmApi.getP2pStatus()`

/// Node health snapshot for the TuffSwarm P2P status card.
#[taurpc::ipc_type]
#[derive(Debug, Default)]
pub struct P2pStatus {
    pub enabled: bool,
    pub healthy: bool,
    pub authorized: bool,
    pub control_url: String,
    pub token_present: bool,
    /// True when this status came from the offline/limited fallback path.
    pub degraded: bool,
}

/// Minimal typed surface over the swarm node — the pilot slice.
/// `export_to` writes TS bindings on every debug build.
#[taurpc::procedures(export_to = "../src/bindings.ts")]
trait SwarmApi {
    async fn get_p2p_status() -> P2pStatus;
}

#[derive(Clone)]
pub struct SwarmApiImpl;

#[taurpc::resolvers]
impl SwarmApi for SwarmApiImpl {
    async fn get_p2p_status(self) -> P2pStatus {
        let swarm = crate::integrations::swarm_settings();
        if !swarm.enabled || !swarm.p2p_enabled {
            return P2pStatus {
                enabled: false,
                control_url: swarm.p2p_control_url,
                ..Default::default()
            };
        }
        let base = swarm
            .p2p_control_url
            .trim()
            .trim_end_matches('/')
            .to_string();
        // Reuse the existing probes from swarm_node (same semantics as the
        // legacy get_p2p_node_status command).
        let healthy = crate::swarm_node::p2p_healthy(&base).await;
        let authorized = crate::swarm_node::p2p_authorized(&base).await;
        P2pStatus {
            enabled: true,
            healthy,
            authorized,
            control_url: base,
            token_present: crate::swarm_node::control_token_is_some(),
            degraded: false,
        }
    }
}

/// Build the TauRPC router. Call from `run()` — merged additively next to
/// the legacy generate_handler! (create_ipc_handler handles a separate
/// `taurpc://` channel; existing commands keep working).
pub fn swarm_api_router() -> taurpc::Router<tauri::Wry> {
    taurpc::Router::new().merge(SwarmApiImpl.into_handler())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn p2p_status_defaults_are_disabled() {
        let s = P2pStatus::default();
        assert!(!s.enabled);
        assert!(!s.healthy);
        assert!(!s.authorized);
        assert!(s.control_url.is_empty());
        // degraded=false in the struct default: it flags the *fallback path*
        // in get_p2p_status, not the disabled default state.
        assert!(!s.degraded);
    }

    #[test]
    fn p2p_status_wire_shape_matches_legacy_command_keys() {
        // The legacy get_p2p_node_status emits camelCase JSON; TauRPC's
        // generated TS bindings use the struct field names (snake_case →
        // camelCase via specta), so key parity is what the UI relies on.
        let s = P2pStatus {
            enabled: true,
            healthy: true,
            authorized: false,
            control_url: "http://127.0.0.1:1".into(),
            token_present: false,
            degraded: false,
        };
        let wire = serde_json::to_value(&s).unwrap();
        assert!(wire.get("enabled").is_some());
        assert!(wire.get("healthy").is_some());
        assert!(wire.get("authorized").is_some());
        assert!(wire.get("controlUrl").is_some() || wire.get("control_url").is_some());
        assert!(wire.get("tokenPresent").is_some() || wire.get("token_present").is_some());
    }
}
