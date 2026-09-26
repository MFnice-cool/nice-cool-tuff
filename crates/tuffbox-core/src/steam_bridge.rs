//! Steam Bridge (https://github.com/Ragalikx/steam-bridge-mc) — LAN-with-friends
//! via Steam P2P/SDR, no Radmin/port-forward.
//!
//! Resolves the GitHub release jar that matches the project's Minecraft version
//! and loader (Fabric / Forge / NeoForge). Quilt uses the Fabric build.

use crate::manifest::{ContentType, LoaderKind, ModSource, ModSpec, Side, SourceKind};
use serde::Deserialize;

pub const STEAM_BRIDGE_MOD_ID: &str = "steambridge";
pub const STEAM_BRIDGE_REPO: &str = "Ragalikx/steam-bridge-mc";
pub const STEAM_BRIDGE_RELEASES_URL: &str =
    "https://api.github.com/repos/Ragalikx/steam-bridge-mc/releases/latest";

#[derive(Debug, Clone)]
pub struct SteamBridgeAsset {
    pub tag: String,
    pub file_name: String,
    pub download_url: String,
    pub loader_label: String,
    pub mc_version: String,
    pub match_kind: SteamBridgeMatchKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SteamBridgeMatchKind {
    Exact,
    SameMinor,
}

#[derive(Debug, Deserialize)]
struct GhRelease {
    #[serde(default)]
    tag_name: String,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(Debug, Deserialize)]
struct GhAsset {
    #[serde(default)]
    name: String,
    #[serde(default)]
    browser_download_url: String,
}

/// Map project loader → Steam Bridge asset label in the jar name.
pub fn loader_asset_label(kind: &LoaderKind) -> Result<&'static str, String> {
    match kind {
        LoaderKind::Fabric | LoaderKind::Quilt => Ok("Fabric"),
        LoaderKind::Forge => Ok("Forge"),
        LoaderKind::Neoforge => Ok("NeoForge"),
        LoaderKind::Vanilla => {
            Err("Steam Bridge needs Fabric, Forge, or NeoForge — vanilla has no mod loader.".into())
        }
    }
}

/// Fetch latest GitHub release and pick the best jar for `mc_version` + `loader`.
pub fn resolve_steam_bridge_asset(
    mc_version: &str,
    loader: &LoaderKind,
) -> Result<SteamBridgeAsset, String> {
    let label = loader_asset_label(loader)?;
    let release: GhRelease = crate::http::get_json_with_context(STEAM_BRIDGE_RELEASES_URL)
        .map_err(|e| format!("Steam Bridge release lookup failed: {e}"))?;
    pick_asset_from_release(&release, mc_version, label)
}

fn pick_asset_from_release(
    release: &GhRelease,
    mc_version: &str,
    loader_label: &str,
) -> Result<SteamBridgeAsset, String> {
    let mc_wanted = mc_version.trim();
    if mc_wanted.is_empty() {
        return Err("Minecraft version is empty — set it on the project first.".into());
    }

    let mut candidates: Vec<(i32, SteamBridgeMatchKind, &GhAsset, String, String)> = Vec::new();
    for asset in &release.assets {
        if !asset.name.ends_with(".jar") || asset.browser_download_url.is_empty() {
            continue;
        }
        let Some((parsed_loader, parsed_mc)) = parse_asset_name(&asset.name) else {
            continue;
        };
        if !parsed_loader.eq_ignore_ascii_case(loader_label) {
            continue;
        }
        let Some((score, kind)) = score_mc_match(mc_wanted, &parsed_mc) else {
            continue;
        };
        candidates.push((score, kind, asset, parsed_loader, parsed_mc));
    }

    candidates.sort_by(|a, b| b.0.cmp(&a.0));
    if let Some((_, kind, asset, loader, mc)) = candidates.into_iter().next() {
        return Ok(SteamBridgeAsset {
            tag: release.tag_name.clone(),
            file_name: asset.name.clone(),
            download_url: asset.browser_download_url.clone(),
            loader_label: loader,
            mc_version: mc,
            match_kind: kind,
        });
    }

    let supported: Vec<String> = release
        .assets
        .iter()
        .filter_map(|a| parse_asset_name(&a.name))
        .filter(|(l, _)| l.eq_ignore_ascii_case(loader_label))
        .map(|(_, mc)| mc)
        .collect();
    Err(format!(
        "No Steam Bridge build for {loader_label} + Minecraft {mc_wanted}. Supported: {}.",
        if supported.is_empty() {
            "(none listed for this loader)".into()
        } else {
            supported.join(", ")
        }
    ))
}

/// `steambridge-1.2-Fabric-1.20.1.jar` → ("Fabric", "1.20.1")
fn parse_asset_name(name: &str) -> Option<(String, String)> {
    let stem = name.strip_suffix(".jar")?;
    // NeoForge before Forge so `-NeoForge-` is not sliced as `-Forge-`.
    for loader in ["NeoForge", "Fabric", "Forge"] {
        let needle = format!("-{loader}-");
        if let Some(idx) = stem.find(&needle) {
            let mc = &stem[idx + needle.len()..];
            if !mc.is_empty() && mc.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                return Some((loader.to_string(), mc.to_string()));
            }
        }
    }
    None
}

fn score_mc_match(wanted: &str, asset_mc: &str) -> Option<(i32, SteamBridgeMatchKind)> {
    if wanted == asset_mc {
        return Some((1000, SteamBridgeMatchKind::Exact));
    }
    let w = parse_semver_parts(wanted)?;
    let a = parse_semver_parts(asset_mc)?;
    // Same major.minor (e.g. 1.21.4 → 1.21.1 build).
    if w.0 == a.0 && w.1 == a.1 {
        let dist = (w.2 as i32 - a.2 as i32).unsigned_abs() as i32;
        return Some((500 - dist, SteamBridgeMatchKind::SameMinor));
    }
    None
}

fn parse_semver_parts(v: &str) -> Option<(u32, u32, u32)> {
    let mut parts = v.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    Some((major, minor, patch))
}

/// Build a manifest [`ModSpec`] for the resolved GitHub asset.
pub fn build_steam_bridge_mod_spec(asset: &SteamBridgeAsset) -> ModSpec {
    ModSpec {
        id: STEAM_BRIDGE_MOD_ID.into(),
        name: "Steam Bridge".into(),
        source: ModSource {
            kind: SourceKind::Github,
            project_id: Some(STEAM_BRIDGE_REPO.into()),
            file_id: Some(asset.tag.clone()),
            url: Some(asset.download_url.clone()),
            path: None,
            icon_url: None,
            categories: vec!["multiplayer".into(), "utility".into()],
        },
        version: format!("{}+mc{}", asset.tag, asset.mc_version),
        file_name: Some(asset.file_name.clone()),
        hashes: None,
        side: Side::Both,
        dependencies: vec![],
        status: vec!["ok".into()],
        content_type: ContentType::Mod,
        authors: vec!["Ragalikx".into()],
        option: None,
    }
}

/// True if the project already lists Steam Bridge (by id or jar name).
pub fn project_has_steam_bridge(mods: &[ModSpec]) -> bool {
    mods.iter().any(|m| {
        m.id.eq_ignore_ascii_case(STEAM_BRIDGE_MOD_ID)
            || m.id.eq_ignore_ascii_case("steam-bridge")
            || m.id.eq_ignore_ascii_case("steam_bridge")
            || m.file_name
                .as_deref()
                .map(|f| f.to_ascii_lowercase().starts_with("steambridge"))
                .unwrap_or(false)
            || m.source
                .project_id
                .as_deref()
                .map(|p| p.eq_ignore_ascii_case(STEAM_BRIDGE_REPO))
                .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release_fixture() -> GhRelease {
        let names = [
            "steambridge-1.2-Fabric-1.16.5.jar",
            "steambridge-1.2-Fabric-1.19.2.jar",
            "steambridge-1.2-Fabric-1.20.1.jar",
            "steambridge-1.2-Fabric-1.21.1.jar",
            "steambridge-1.2-Forge-1.20.1.jar",
            "steambridge-1.2-NeoForge-1.20.1.jar",
            "steambridge-1.2-NeoForge-1.21.1.jar",
            "SHA256SUMS.txt",
        ];
        GhRelease {
            tag_name: "1.2".into(),
            assets: names
                .iter()
                .map(|n| GhAsset {
                    name: (*n).into(),
                    browser_download_url: format!(
                        "https://github.com/{STEAM_BRIDGE_REPO}/releases/download/1.2/{n}"
                    ),
                })
                .collect(),
        }
    }

    #[test]
    fn parses_asset_names() {
        assert_eq!(
            parse_asset_name("steambridge-1.2-Fabric-1.20.1.jar"),
            Some(("Fabric".into(), "1.20.1".into()))
        );
        assert_eq!(
            parse_asset_name("steambridge-1.2-NeoForge-1.21.1.jar"),
            Some(("NeoForge".into(), "1.21.1".into()))
        );
        assert!(parse_asset_name("SHA256SUMS.txt").is_none());
    }

    #[test]
    fn picks_exact_fabric_version() {
        let r = release_fixture();
        let a = pick_asset_from_release(&r, "1.20.1", "Fabric").unwrap();
        assert_eq!(a.file_name, "steambridge-1.2-Fabric-1.20.1.jar");
        assert_eq!(a.match_kind, SteamBridgeMatchKind::Exact);
    }

    #[test]
    fn falls_back_same_minor() {
        let r = release_fixture();
        let a = pick_asset_from_release(&r, "1.21.4", "Fabric").unwrap();
        assert_eq!(a.file_name, "steambridge-1.2-Fabric-1.21.1.jar");
        assert_eq!(a.match_kind, SteamBridgeMatchKind::SameMinor);
    }

    #[test]
    fn picks_neoforge() {
        let r = release_fixture();
        let a = pick_asset_from_release(&r, "1.21.1", "NeoForge").unwrap();
        assert!(a.file_name.contains("NeoForge"));
    }

    #[test]
    fn errors_when_unsupported() {
        let r = release_fixture();
        let err = pick_asset_from_release(&r, "1.18.2", "Fabric").unwrap_err();
        assert!(err.contains("No Steam Bridge"));
        assert!(err.contains("1.20.1"));
    }

    #[test]
    fn quilt_maps_to_fabric_label() {
        assert_eq!(loader_asset_label(&LoaderKind::Quilt).unwrap(), "Fabric");
    }

    #[test]
    fn detects_installed() {
        let spec = build_steam_bridge_mod_spec(&SteamBridgeAsset {
            tag: "1.2".into(),
            file_name: "steambridge-1.2-Fabric-1.20.1.jar".into(),
            download_url: "https://example.com/x.jar".into(),
            loader_label: "Fabric".into(),
            mc_version: "1.20.1".into(),
            match_kind: SteamBridgeMatchKind::Exact,
        });
        assert!(project_has_steam_bridge(&[spec]));
    }
}
