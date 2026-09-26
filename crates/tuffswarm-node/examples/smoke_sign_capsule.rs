//! Print one minimal signed ExperienceCapsule JSON to stdout for capsule-smoke.ps1.
//!
//! Usage:
//!   cargo run -p tuffswarm-node --example smoke_sign_capsule
//!   cargo run -p tuffswarm-node --example smoke_sign_capsule -- --fingerprint "smoke|b3|fabric"

use clap::Parser;
use tuffbox_core::crash_kb::{CrashCase, CrashFingerprint};
use tuffbox_core::swarm::ExperienceCapsule;

#[derive(Debug, Parser)]
struct Args {
    /// Fingerprint key embedded in the capsule (unique per smoke run helps debugging).
    #[arg(long, default_value = "smoke|b3-capsule|fabric")]
    fingerprint: String,
}

fn main() {
    let args = Args::parse();
    let case = CrashCase {
        id: format!("smoke-{}", tuffbox_core::time_util::compact_now()),
        fingerprint: CrashFingerprint {
            exception: "SmokeCapsuleError".into(),
            frames: vec!["tuffswarm.smoke".into()],
            mod_file: None,
            mixin: None,
            mc_major: "1.20".into(),
            loader: "fabric".into(),
            key: args.fingerprint,
            blame_mod_ids: Vec::new(),
        },
        symptoms: vec![],
        suspected_mods: vec![],
        solution: "B3 capsule-smoke fixture - ignore in production".into(),
        actions: vec![],
        launcher_actions: vec![],
        notes: None,
        source: "authored".into(),
        success_count: 1,
        fail_count: 0,
    };
    let mut capsule = ExperienceCapsule::from_crash_case(&case).sanitized_for_network();
    let sk = ExperienceCapsule::generate_signing_key();
    capsule
        .sign_ed25519(&sk, "smoke-sign-capsule")
        .expect("sign");
    let public = capsule.to_public_json();
    println!("{}", serde_json::to_string(&public).expect("json"));
}
