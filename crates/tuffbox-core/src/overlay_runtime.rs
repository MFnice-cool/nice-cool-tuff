//! Overlay launch support: session JSON for the GL-hook DLL (+ optional legacy JVM jar).
//!
//! Primary path (any MC / any loader): write `.tuffbox/overlay-session.json` and let the
//! desktop inject `tuffbox_overlay_hook.dll`. The Fabric/NeoForge jar is **not** injected
//! by default (set `TUFFBOX_OVERLAY_JVM=1` to force the old 1.21.1 jar path).

use crate::{LoaderKind, McVersion, ProjectManifest};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

pub const BUILTIN_OVERLAY_API_BASE: &str = crate::swarm::BUILTIN_SUPABASE_URL;
pub const BUILTIN_OVERLAY_ANON_KEY: &str = crate::swarm::BUILTIN_SUPABASE_ANON_KEY;

fn overlay_anon_key() -> String {
    std::env::var("TUFFBOX_OVERLAY_ANON_KEY")
        .or_else(|_| std::env::var("TUFFBOX_COSMETICS_ANON_KEY"))
        .or_else(|_| std::env::var("TUFFSWARM_SUPABASE_ANON_KEY"))
        .unwrap_or_else(|_| BUILTIN_OVERLAY_ANON_KEY.to_string())
}

fn overlay_api_base() -> String {
    std::env::var("TUFFBOX_OVERLAY_API_BASE")
        .or_else(|_| std::env::var("TUFFBOX_COSMETICS_API_BASE"))
        .or_else(|_| std::env::var("TUFFSWARM_SUPABASE_URL"))
        .unwrap_or_else(|_| BUILTIN_OVERLAY_API_BASE.to_string())
}

/// When true, also copy the legacy 1.21.1 Fabric/NeoForge overlay jar (WATERMeDIA path).
fn jvm_jar_inject_enabled() -> bool {
    matches!(
        std::env::var("TUFFBOX_OVERLAY_JVM").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE") | Ok("yes")
    )
}

#[derive(Debug, Clone)]
pub struct OverlayBridgeLaunch {
    pub cleanup_paths: Vec<PathBuf>,
    pub message: String,
    /// Absolute path to `.tuffbox/overlay-session.json` (hook + proxy read this).
    pub session_path: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlaySessionFile {
    pub username: String,
    pub uuid: String,
    pub api_base: String,
    pub anon_key: String,
    #[serde(default)]
    pub write_secret: String,
    #[serde(default)]
    pub pack_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlayAnchor {
    pub version: &'static str,
    pub loaders: &'static [&'static str],
}

/// Legacy JVM overlay anchors (opt-in via `TUFFBOX_OVERLAY_JVM=1`). Exact match only.
pub const OVERLAY_ANCHORS: &[OverlayAnchor] = &[OverlayAnchor {
    version: "1.21.1",
    loaders: &["fabric", "neoforge"],
}];

fn overlay_loader_tag(loader: &LoaderKind) -> Option<&'static str> {
    match loader {
        LoaderKind::Fabric | LoaderKind::Quilt => Some("fabric"),
        LoaderKind::Neoforge => Some("neoforge"),
        _ => None,
    }
}

/// Exact MC 1.21.1 + Fabric/Quilt/NeoForge only (no silent fallback to newer MC).
pub fn resolve_overlay_artifact(
    mc: &str,
    loader: &LoaderKind,
) -> Option<(&'static str, &'static str)> {
    let want = McVersion::parse(mc)?;
    let tag = overlay_loader_tag(loader)?;
    for a in OVERLAY_ANCHORS {
        if !a.loaders.contains(&tag) {
            continue;
        }
        let av = McVersion::parse(a.version)?;
        if av == want {
            return Some((a.version, tag));
        }
    }
    None
}

fn find_overlay_jar(mc: &str, loader: &LoaderKind) -> Option<(PathBuf, &'static str)> {
    let (anchor, loader_name) = resolve_overlay_artifact(mc, loader)?;
    let needle = format!("tuffbox-overlay-{anchor}-{loader_name}");
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(dir) = std::env::var("TUFFBOX_OVERLAY_DIR") {
        candidates.push(PathBuf::from(dir));
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../bridges/overlay");
    candidates.push(root.join("prebuilt"));
    for mod_name in ["fabric", "neoforge"] {
        candidates.push(root.join(mod_name).join("build").join("libs"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join("resources").join("overlay"));
            candidates.push(parent.join("overlay"));
        }
    }
    for dir in candidates {
        if !dir.is_dir() {
            continue;
        }
        if let Ok(rd) = fs::read_dir(&dir) {
            for ent in rd.flatten() {
                let name = ent.file_name().to_string_lossy().to_string();
                if name.ends_with(".jar")
                    && name.contains(&needle)
                    && !name.contains("-sources")
                    && !name.contains("-dev")
                    && !name.contains("-javadoc")
                {
                    return Some((ent.path(), anchor));
                }
            }
        }
    }
    None
}

const WATERMEDIA_URL: &str =
    "https://cdn.modrinth.com/data/G922NeHS/versions/xp27BzFX/watermedia-2.1.1.jar";
const WATERMEDIA_SHA256: &str = "23c7e9ae03f28b234cfdb4b8a642a67f7dd1a7c336d87ced19e617dce9f8b58a";
const WATERMEDIA_DEST: &str = "watermedia-2.1.1.jar";

fn watermedia_url() -> String {
    std::env::var("TUFFBOX_OVERLAY_WATERMEDIA_URL").unwrap_or_else(|_| WATERMEDIA_URL.to_string())
}

fn watermedia_sha256() -> String {
    std::env::var("TUFFBOX_OVERLAY_WATERMEDIA_SHA256")
        .unwrap_or_else(|_| WATERMEDIA_SHA256.to_string())
}

fn overlay_cache_root() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("TuffBox")
        .join("overlay")
}

fn watermedia_present(mods_dir: &Path) -> bool {
    if let Ok(rd) = fs::read_dir(mods_dir) {
        for ent in rd.flatten() {
            let name = ent.file_name().to_string_lossy().to_lowercase();
            if name.starts_with("watermedia") && name.ends_with(".jar") {
                return true;
            }
        }
    }
    false
}

fn ensure_watermedia(mods_dir: &Path, cleanup: &mut Vec<PathBuf>, notes: &mut Vec<String>) {
    if watermedia_present(mods_dir) {
        notes.push("WATERMeDIA already present in mods".to_string());
        return;
    }
    let cache_root = overlay_cache_root();
    match crate::download_cache::get_or_download(
        &cache_root,
        &watermedia_url(),
        Some(&watermedia_sha256()),
    ) {
        Ok(src) => {
            let dest = mods_dir.join(WATERMEDIA_DEST);
            match fs::copy(&src, &dest) {
                Ok(_) => {
                    cleanup.push(dest);
                    notes.push("WATERMeDIA 2.1.1 injected".to_string());
                }
                Err(e) => notes.push(format!("WATERMeDIA copy failed: {e}")),
            }
        }
        Err(e) => notes.push(format!("WATERMeDIA download failed: {e}")),
    }
}

/// Write overlay session for the GL-hook / IPC proxy. Always succeeds for any MC/loader.
pub fn write_overlay_session(
    manifest: &ProjectManifest,
    game_dir: &Path,
    username: &str,
    uuid: &str,
    write_secret: &str,
) -> Result<PathBuf, String> {
    let runtime_dir = game_dir.join(".tuffbox");
    fs::create_dir_all(&runtime_dir).map_err(|e| e.to_string())?;
    let session_path = runtime_dir.join("overlay-session.json");
    let session = OverlaySessionFile {
        username: username.to_string(),
        uuid: uuid.to_string(),
        api_base: overlay_api_base(),
        anon_key: overlay_anon_key(),
        write_secret: write_secret.to_string(),
        pack_name: manifest.project.name.clone(),
    };
    let body = serde_json::to_vec_pretty(&session).map_err(|e| e.to_string())?;
    fs::write(&session_path, body).map_err(|e| e.to_string())?;
    Ok(session_path)
}

/// Prepare overlay for launch: **always** write session; optional legacy JVM jar.
pub fn prepare_overlay_bridge(
    manifest: &ProjectManifest,
    game_dir: &Path,
    username: &str,
    uuid: &str,
    write_secret: &str,
) -> Result<Option<OverlayBridgeLaunch>, String> {
    let session_path = write_overlay_session(manifest, game_dir, username, uuid, write_secret)?;
    let mut cleanup = vec![session_path.clone()];
    let mut notes = vec!["overlay session ready (GL hook)".to_string()];

    if jvm_jar_inject_enabled() {
        let mc = manifest.minecraft.version.as_str();
        let loader = &manifest.loader.kind;
        if resolve_overlay_artifact(mc, loader).is_some() {
            let mods_dir = game_dir.join("mods");
            fs::create_dir_all(&mods_dir).map_err(|e| e.to_string())?;
            match find_overlay_jar(mc, loader) {
                Some((src, anchor)) => {
                    let dest = mods_dir.join("tuffbox-overlay.runtime.jar");
                    fs::copy(&src, &dest).map_err(|e| format!("copy overlay jar: {e}"))?;
                    cleanup.push(dest);
                    notes.push(format!("legacy JVM overlay jar ({anchor})"));
                    ensure_watermedia(&mods_dir, &mut cleanup, &mut notes);
                }
                None => notes
                    .push("TUFFBOX_OVERLAY_JVM set but jar missing — build bridges/overlay".into()),
            }
        } else {
            notes.push(
                "TUFFBOX_OVERLAY_JVM set but MC/loader is not exact 1.21.1 Fabric/NeoForge".into(),
            );
        }
    }

    Ok(Some(OverlayBridgeLaunch {
        cleanup_paths: cleanup,
        message: notes.join("; "),
        session_path,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_exact_only_no_fallback() {
        assert_eq!(
            resolve_overlay_artifact("1.21.1", &LoaderKind::Fabric),
            Some(("1.21.1", "fabric"))
        );
        assert_eq!(
            resolve_overlay_artifact("1.21.1", &LoaderKind::Neoforge),
            Some(("1.21.1", "neoforge"))
        );
        assert_eq!(
            resolve_overlay_artifact("1.21.1", &LoaderKind::Quilt),
            Some(("1.21.1", "fabric"))
        );
        // No silent fallback for newer MC
        assert_eq!(
            resolve_overlay_artifact("1.21.4", &LoaderKind::Neoforge),
            None
        );
        assert_eq!(resolve_overlay_artifact("1.21.4", &LoaderKind::Quilt), None);
        assert_eq!(resolve_overlay_artifact("1.21", &LoaderKind::Fabric), None);
        assert_eq!(resolve_overlay_artifact("1.21.1", &LoaderKind::Forge), None);
        assert_eq!(
            resolve_overlay_artifact("1.20.1", &LoaderKind::Fabric),
            None
        );
    }
}
