//! Launch-time cosmetics stack: CustomSkinLoader + optional tuffbox-cosmetics jar.
//!
//! Writes CSL loadlist (TuffBox → Mojang → Ely → TLauncher) and a session JSON
//! for the client mod. Runtime jars are cleaned up on process exit (JEI pattern).

use crate::{LoaderKind, ProjectManifest};
use serde::Serialize;
use serde_json::json;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const BUILTIN_COSMETICS_API_BASE: &str = crate::swarm::BUILTIN_SUPABASE_URL;
pub const BUILTIN_COSMETICS_ANON_KEY: &str = crate::swarm::BUILTIN_SUPABASE_ANON_KEY;

/// Override via env if the built-in anon key is wrong for the linked project.
fn cosmetics_anon_key() -> String {
    std::env::var("TUFFBOX_COSMETICS_ANON_KEY")
        .or_else(|_| std::env::var("TUFFSWARM_SUPABASE_ANON_KEY"))
        .unwrap_or_else(|_| BUILTIN_COSMETICS_ANON_KEY.to_string())
}

fn cosmetics_api_base() -> String {
    std::env::var("TUFFBOX_COSMETICS_API_BASE")
        .or_else(|_| std::env::var("TUFFSWARM_SUPABASE_URL"))
        .unwrap_or_else(|_| BUILTIN_COSMETICS_API_BASE.to_string())
}

#[derive(Debug, Clone)]
pub struct CosmeticsBridgeLaunch {
    pub cleanup_paths: Vec<PathBuf>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CosmeticsSessionFile {
    username: String,
    uuid: String,
    api_base: String,
    anon_key: String,
    #[serde(default)]
    write_secret: String,
    wings: String,
    hat: String,
    trail: bool,
    jump_circles: bool,
    hit_particles: bool,
    hit_bubbles: bool,
    target_esp: bool,
    kill_effect: bool,
}

#[derive(Debug, Clone, Default)]
pub struct CosmeticsLaunchExtras {
    pub wings: Option<String>,
    pub hat: Option<String>,
    pub trail: bool,
    pub jump_circles: bool,
    pub hit_particles: bool,
    pub hit_bubbles: bool,
    pub target_esp: bool,
    pub kill_effect: bool,
    pub write_secret: String,
}

/// Prepare cosmetics inject for a launch. Returns None when MC/loader unsupported
/// and no CSL jar could be resolved — caller should still launch the game.
pub fn prepare_cosmetics_bridge(
    manifest: &ProjectManifest,
    game_dir: &Path,
    username: &str,
    uuid: &str,
    extras: CosmeticsLaunchExtras,
) -> Result<Option<CosmeticsBridgeLaunch>, String> {
    let mc = manifest.minecraft.version.as_str();
    let loader = &manifest.loader.kind;
    if !matches!(
        loader,
        LoaderKind::Fabric | LoaderKind::Neoforge | LoaderKind::Forge | LoaderKind::Quilt
    ) {
        return Ok(None);
    }

    let mods_dir = game_dir.join("mods");
    fs::create_dir_all(&mods_dir).map_err(|e| e.to_string())?;
    let runtime_dir = game_dir.join(".tuffbox");
    fs::create_dir_all(&runtime_dir).map_err(|e| e.to_string())?;

    let mut cleanup = Vec::new();
    let mut notes = Vec::new();

    // ── CustomSkinLoader ──────────────────────────────────────────────
    match ensure_custom_skin_loader(mc, loader, &mods_dir) {
        Ok(Some(jar)) => {
            cleanup.push(jar);
            notes.push("CustomSkinLoader injected".to_string());
        }
        Ok(None) => notes.push(format!(
            "CustomSkinLoader jar not found for {mc}/{loader:?} (skins may be vanilla-only)"
        )),
        Err(e) => notes.push(format!("CustomSkinLoader install skipped: {e}")),
    }

    write_csl_config(game_dir)?;

    // ── tuffbox-cosmetics runtime jar (optional, anchor fallback) ─────
    match find_cosmetics_jar(mc, loader) {
        Some((src, anchor)) => {
            let dest = mods_dir.join("tuffbox-cosmetics.runtime.jar");
            copy_if_unchanged(&src, &dest).map_err(|e| format!("copy cosmetics jar: {e}"))?;
            cleanup.push(dest);
            if anchor == mc {
                notes.push(format!("tuffbox-cosmetics injected ({anchor})"));
            } else {
                notes.push(format!(
                    "tuffbox-cosmetics injected (anchor {anchor} for {mc})"
                ));
            }
        }
        None => {
            if let Some((anchor, tag)) = resolve_cosmetics_artifact(mc, loader) {
                notes.push(format!(
                    "tuffbox-cosmetics jar missing for anchor {anchor}/{tag} — build bridges/cosmetics"
                ));
            } else {
                notes.push(format!(
                    "tuffbox-cosmetics: no FX anchor for {mc}/{loader:?} (CSL/session only)"
                ));
            }
        }
    }

    // ── session ───────────────────────────────────────────────────────
    let session_path = runtime_dir.join("cosmetics-session.json");
    let session = CosmeticsSessionFile {
        username: username.to_string(),
        uuid: uuid.to_string(),
        api_base: cosmetics_api_base(),
        anon_key: cosmetics_anon_key(),
        write_secret: extras.write_secret.clone(),
        wings: extras.wings.unwrap_or_default(),
        hat: extras.hat.unwrap_or_default(),
        trail: extras.trail,
        jump_circles: extras.jump_circles,
        hit_particles: extras.hit_particles,
        hit_bubbles: extras.hit_bubbles,
        target_esp: extras.target_esp,
        kill_effect: extras.kill_effect,
    };
    let body = serde_json::to_vec_pretty(&session).map_err(|e| e.to_string())?;
    fs::write(&session_path, body).map_err(|e| e.to_string())?;
    cleanup.push(session_path);

    // LocalSkin fallback for self (CSL LocalSkin)
    write_local_skin_hint(game_dir, username)?;

    Ok(Some(CosmeticsBridgeLaunch {
        cleanup_paths: cleanup,
        message: notes.join("; "),
    }))
}

fn copy_if_unchanged(src: &Path, dest: &Path) -> Result<(), String> {
    if dest.is_file() {
        if let (Ok(s), Ok(d)) = (fs::metadata(src), fs::metadata(dest)) {
            if s.len() == d.len() {
                return Ok(());
            }
        }
    }
    fs::copy(src, dest).map(|_| ()).map_err(|e| e.to_string())
}

fn write_local_skin_hint(game_dir: &Path, username: &str) -> Result<(), String> {
    let dir = game_dir
        .join("CustomSkinLoader")
        .join("LocalSkin")
        .join("skins");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // Copy from launcher cosmetics cache if present
    if let Some(cache) = cosmetics_cache_dir() {
        let src = cache.join(username).join("skin.png");
        if src.is_file() {
            let _ = fs::copy(src, dir.join(format!("{username}.png")));
        }
        let cape_src = cache.join(username).join("cape.png");
        if cape_src.is_file() {
            let cape_dir = game_dir
                .join("CustomSkinLoader")
                .join("LocalSkin")
                .join("capes");
            let _ = fs::create_dir_all(&cape_dir);
            let _ = fs::copy(cape_src, cape_dir.join(format!("{username}.png")));
        }
    }
    Ok(())
}

fn write_csl_config(game_dir: &Path) -> Result<(), String> {
    let dir = game_dir.join("CustomSkinLoader");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let api = cosmetics_api_base();
    // CustomSkinAPI root: CSL requests `{root}{username}.json`
    let tuffbox_root = format!("{}/functions/v1/cosmetics-get/", api.trim_end_matches('/'));
    let cfg = json!({
        "version": 1,
        "buildNumber": 0,
        "loadlist": [
            {
                "name": "TuffBox",
                "type": "CustomSkinAPI",
                "root": tuffbox_root
            },
            {
                "name": "Mojang",
                "type": "MojangAPI"
            },
            {
                "name": "ElyBy",
                "type": "ElyByAPI",
                "apiRoot": "http://skinsystem.ely.by/"
            },
            {
                "name": "TLauncher",
                "type": "ElyByAPI",
                "apiRoot": "https://auth.tlauncher.org/skin/"
            },
            {
                "name": "LocalSkin",
                "type": "Legacy",
                "checkPNG": true,
                "skin": "LocalSkin/skins/{USERNAME}.png",
                "cape": "LocalSkin/capes/{USERNAME}.png"
            }
        ]
    });
    let path = dir.join("CustomSkinLoader.json");
    let mut f = fs::File::create(&path).map_err(|e| e.to_string())?;
    f.write_all(serde_json::to_string_pretty(&cfg).unwrap().as_bytes())
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn cosmetics_cache_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("TuffBox").join("cosmetics"))
}

/// Parsed MC version: classic `1.x.y` or year-based `26.1.2` / `26.2`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct McVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl McVersion {
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        // Strip snapshot suffixes: 26.2-snapshot-5, 1.21.1-rc1
        let base = s
            .split(|c| c == '-' || c == '+' || c == '_')
            .next()
            .unwrap_or(s);
        let mut parts = base.split('.');
        let major: u32 = parts.next()?.parse().ok()?;
        let minor: u32 = parts.next()?.parse().ok()?;
        let patch: u32 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
        Some(Self {
            major,
            minor,
            patch,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CosmeticsAnchor {
    pub version: &'static str,
    pub loaders: &'static [&'static str],
}

/// Anchor jars we ship (or plan to ship). Inject picks highest `anchor <= mc` for the loader.
pub const COSMETICS_ANCHORS: &[CosmeticsAnchor] = &[
    CosmeticsAnchor {
        version: "1.12.2",
        loaders: &["forge"],
    },
    CosmeticsAnchor {
        version: "1.16.5",
        loaders: &["forge", "fabric"],
    },
    CosmeticsAnchor {
        version: "1.20.1",
        loaders: &["forge", "fabric", "neoforge"],
    },
    CosmeticsAnchor {
        version: "1.21.1",
        loaders: &["fabric", "neoforge"],
    },
    CosmeticsAnchor {
        version: "1.21.4",
        loaders: &["fabric", "neoforge"],
    },
    CosmeticsAnchor {
        version: "1.21.11",
        loaders: &["fabric", "neoforge"],
    },
    CosmeticsAnchor {
        version: "26.1.2",
        loaders: &["fabric", "neoforge"],
    },
    CosmeticsAnchor {
        version: "26.2",
        loaders: &["fabric", "neoforge"],
    },
];

fn loader_tag(loader: &LoaderKind) -> Option<&'static str> {
    match loader {
        LoaderKind::Fabric | LoaderKind::Quilt => Some("fabric"),
        LoaderKind::Neoforge => Some("neoforge"),
        LoaderKind::Forge => Some("forge"),
        _ => None,
    }
}

/// Resolve cosmetics artifact: highest anchor ≤ `mc` that supports `loader`.
pub fn resolve_cosmetics_artifact(
    mc: &str,
    loader: &LoaderKind,
) -> Option<(&'static str, &'static str)> {
    let want = McVersion::parse(mc)?;
    let tag = loader_tag(loader)?;
    let mut best: Option<(&'static str, McVersion)> = None;
    for a in COSMETICS_ANCHORS {
        if !a.loaders.contains(&tag) {
            continue;
        }
        let av = McVersion::parse(a.version)?;
        if av > want {
            continue;
        }
        match best {
            None => best = Some((a.version, av)),
            Some((_, bv)) if av > bv => best = Some((a.version, av)),
            _ => {}
        }
    }
    best.map(|(v, _)| (v, tag))
}

fn find_cosmetics_jar(mc: &str, loader: &LoaderKind) -> Option<(PathBuf, &'static str)> {
    let (anchor, loader_name) = resolve_cosmetics_artifact(mc, loader)?;
    let needle = format!("tuffbox-cosmetics-{anchor}-{loader_name}");
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(dir) = std::env::var("TUFFBOX_COSMETICS_DIR") {
        candidates.push(PathBuf::from(dir));
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../bridges/cosmetics");
    candidates.push(root.join("prebuilt"));
    // Per-module build outputs
    for mod_name in [
        "fabric",
        "fabric-1.21.1",
        "neoforge",
        "neoforge-1.21.1",
        "fabric-1.21.4",
        "neoforge-1.21.4",
        "fabric-1.21.11",
        "neoforge-1.21.11",
        "fabric-26.1.2",
        "neoforge-26.1.2",
        "fabric-26.2",
        "neoforge-26.2",
        "fabric-1.20.1",
        "forge-1.20.1",
        "neoforge-1.20.1",
        "fabric-1.16.5",
        "forge-1.16.5",
        "forge-1.12.2",
    ] {
        candidates.push(root.join(mod_name).join("build").join("libs"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join("resources").join("cosmetics"));
            candidates.push(parent.join("cosmetics"));
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

fn ensure_custom_skin_loader(
    mc: &str,
    loader: &LoaderKind,
    mods_dir: &Path,
) -> Result<Option<PathBuf>, String> {
    let cache = dirs::data_local_dir()
        .ok_or_else(|| "no data_local_dir".to_string())?
        .join("TuffBox")
        .join("cosmetics-cache")
        .join("csl");
    fs::create_dir_all(&cache).map_err(|e| e.to_string())?;

    let loader_tag = match loader {
        LoaderKind::Fabric | LoaderKind::Quilt => "Fabric",
        LoaderKind::Forge => "Forge",
        LoaderKind::Neoforge => "NeoForge",
        _ => "Fabric",
    };

    // Prefer cached jar matching mc. Do not wait on Modrinth during Play —
    // a cache miss used to stall launch up to 30s.
    if let Some(existing) = find_cached_csl(&cache, mc, loader_tag) {
        let dest = mods_dir.join("tuffbox-csl.runtime.jar");
        copy_if_unchanged(&existing, &dest)?;
        return Ok(Some(dest));
    }

    let cache_bg = cache.clone();
    let mc_bg = mc.to_string();
    let loader_bg = loader_tag.to_string();
    std::thread::spawn(move || {
        let _ = download_csl_from_modrinth(&mc_bg, &loader_bg, &cache_bg);
    });
    Ok(None)
}

fn find_cached_csl(cache: &Path, mc: &str, loader_tag: &str) -> Option<PathBuf> {
    let rd = fs::read_dir(cache).ok()?;
    for ent in rd.flatten() {
        let name = ent.file_name().to_string_lossy().to_ascii_lowercase();
        if !name.ends_with(".jar") {
            continue;
        }
        if name.contains(&mc.to_ascii_lowercase())
            && name.contains(&loader_tag.to_ascii_lowercase())
        {
            return Some(ent.path());
        }
        // Universal CSL bootstrap
        if name.contains("customskinloader") && name.contains("universal") {
            return Some(ent.path());
        }
    }
    None
}

fn download_csl_from_modrinth(
    mc: &str,
    loader_tag: &str,
    cache: &Path,
) -> Result<Option<PathBuf>, String> {
    let loaders = match loader_tag {
        "NeoForge" => "neoforge",
        "Forge" => "forge",
        _ => "fabric",
    };
    let url = format!(
        "https://api.modrinth.com/v2/project/idMHQ4n2/version?game_versions=[\"{mc}\"]&loaders=[\"{loaders}\"]"
    );
    let client = reqwest::blocking::Client::builder()
        .user_agent("TuffBox/cosmetics")
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client.get(&url).send().map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Ok(None);
    }
    let versions: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
    let Some(arr) = versions.as_array() else {
        return Ok(None);
    };
    let Some(first) = arr.first() else {
        return Ok(None);
    };
    let files = first
        .get("files")
        .and_then(|f| f.as_array())
        .cloned()
        .unwrap_or_default();
    let primary = files
        .iter()
        .find(|f| f.get("primary").and_then(|p| p.as_bool()) == Some(true))
        .or_else(|| files.first());
    let Some(file) = primary else {
        return Ok(None);
    };
    let dl = file
        .get("url")
        .and_then(|u| u.as_str())
        .ok_or_else(|| "csl file url missing".to_string())?;
    let fname = file
        .get("filename")
        .and_then(|u| u.as_str())
        .unwrap_or("CustomSkinLoader.jar");
    let dest = cache.join(fname);
    if dest.is_file() {
        return Ok(Some(dest));
    }
    let bytes = client
        .get(dl)
        .send()
        .map_err(|e| e.to_string())?
        .bytes()
        .map_err(|e| e.to_string())?;
    fs::write(&dest, &bytes).map_err(|e| e.to_string())?;
    Ok(Some(dest))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csl_config_mentions_tuffbox_and_mojang() {
        let dir = tempfile::tempdir().unwrap();
        write_csl_config(dir.path()).unwrap();
        let text =
            fs::read_to_string(dir.path().join("CustomSkinLoader/CustomSkinLoader.json")).unwrap();
        assert!(text.contains("TuffBox"));
        assert!(text.contains("Mojang"));
        assert!(text.contains("ElyBy"));
        assert!(text.contains("TLauncher"));
    }

    #[test]
    fn parse_classic_and_year_versions() {
        assert_eq!(
            McVersion::parse("1.12.2"),
            Some(McVersion {
                major: 1,
                minor: 12,
                patch: 2
            })
        );
        assert_eq!(
            McVersion::parse("26.2"),
            Some(McVersion {
                major: 26,
                minor: 2,
                patch: 0
            })
        );
        assert_eq!(
            McVersion::parse("26.1.2"),
            Some(McVersion {
                major: 26,
                minor: 1,
                patch: 2
            })
        );
        assert_eq!(
            McVersion::parse("26.2-snapshot-5"),
            Some(McVersion {
                major: 26,
                minor: 2,
                patch: 0
            })
        );
    }

    #[test]
    fn resolve_anchor_fallbacks() {
        assert_eq!(
            resolve_cosmetics_artifact("1.20.4", &LoaderKind::Fabric),
            Some(("1.20.1", "fabric"))
        );
        assert_eq!(
            resolve_cosmetics_artifact("1.21.3", &LoaderKind::Fabric),
            Some(("1.21.1", "fabric"))
        );
        assert_eq!(
            resolve_cosmetics_artifact("1.21.5", &LoaderKind::Neoforge),
            Some(("1.21.4", "neoforge"))
        );
        assert_eq!(
            resolve_cosmetics_artifact("1.21.0", &LoaderKind::Fabric),
            Some(("1.20.1", "fabric"))
        );
        assert_eq!(
            resolve_cosmetics_artifact("1.21.1", &LoaderKind::Fabric),
            Some(("1.21.1", "fabric"))
        );
        assert_eq!(
            resolve_cosmetics_artifact("1.21", &LoaderKind::Quilt),
            Some(("1.20.1", "fabric"))
        );
        // 26.1.0 < 26.1.2 → previous fabric anchor 1.21.11
        assert_eq!(
            resolve_cosmetics_artifact("26.1.0", &LoaderKind::Fabric),
            Some(("1.21.11", "fabric"))
        );
        assert_eq!(
            resolve_cosmetics_artifact("26.1.2", &LoaderKind::Fabric),
            Some(("26.1.2", "fabric"))
        );
        assert_eq!(
            resolve_cosmetics_artifact("26.2", &LoaderKind::Neoforge),
            Some(("26.2", "neoforge"))
        );
        assert_eq!(
            resolve_cosmetics_artifact("1.12.2", &LoaderKind::Fabric),
            None
        );
        assert_eq!(
            resolve_cosmetics_artifact("1.12.2", &LoaderKind::Forge),
            Some(("1.12.2", "forge"))
        );
    }
}
