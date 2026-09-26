use serde::Deserialize;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use thiserror::Error;

const POPULAR_MINECRAFT: &[&str] = &["1.12.2", "1.16.5", "1.20.1", "1.21.1", "26.1.2", "26.2"];
const VERSION_MANIFEST_URL: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

#[derive(Debug, Error)]
pub enum VersionsError {
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("parse error: {0}")]
    ParseError(String),
    #[error("{0}")]
    Other(String),
}

#[derive(Debug, Deserialize)]
struct Manifest {
    #[serde(default)]
    latest: Option<LatestVersion>,
    versions: Vec<ManifestVersion>,
}

#[derive(Debug, Deserialize)]
struct LatestVersion {
    release: String,
    snapshot: String,
}

#[derive(Debug, Deserialize)]
struct ManifestVersion {
    id: String,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct MinecraftVersion {
    pub id: String,
    pub popular: bool,
}

pub fn fetch_minecraft_versions() -> Vec<MinecraftVersion> {
    // Process-global TTL cache: the Mojang manifest is large and rarely
    // changes; re-fetching it on every Setup-tab open made the stage load
    // for seconds. Stale entries are still served after TTL expiry so the
    // tab opens instantly even when offline / rate-limited.
    crate::api_cache::get_or_insert_with_ttl(
        "versions:minecraft",
        std::time::Duration::from_secs(60 * 60),
        || -> Vec<MinecraftVersion> {
            let popular: HashSet<String> =
                POPULAR_MINECRAFT.iter().map(|s| s.to_string()).collect();
            let mut popular_versions: Vec<MinecraftVersion> = POPULAR_MINECRAFT
                .iter()
                .map(|id| MinecraftVersion {
                    id: id.to_string(),
                    popular: true,
                })
                .collect();

            let manifest: Manifest = match crate::http::get_json_with_context(VERSION_MANIFEST_URL)
            {
                Ok(m) => m,
                Err(e) => return fallback_popular_only(popular_versions, e),
            };
            let mut rest: Vec<MinecraftVersion> = manifest
                .versions
                .into_iter()
                .filter(|v| v.kind == "release" && !popular.contains(&v.id))
                .map(|v| MinecraftVersion {
                    id: v.id,
                    popular: false,
                })
                .collect();

            rest.sort_by(|a, b| compare_versions(&b.id, &a.id));

            popular_versions.append(&mut rest);
            popular_versions
        },
    )
}

/// Offline / failed-manifest fallback: return at least the popular pins so
/// the Setup dropdown is usable; the error is logged to stderr.
fn fallback_popular_only(
    popular_versions: Vec<MinecraftVersion>,
    e: String,
) -> Vec<MinecraftVersion> {
    eprintln!("[versions] manifest fetch failed, serving popular pins only: {e}");
    popular_versions
}

fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let parse = |s: &str| {
        s.split('.')
            .filter_map(|p| p.parse::<u32>().ok())
            .collect::<Vec<_>>()
    };
    parse(a).cmp(&parse(b))
}

/// Resolves a Minecraft version *alias* to a concrete version id.
///
/// Mojang's launcher and many importers accept the symbolic versions
/// `latest` / `release` / `latest-release` (newest stable release) and
/// `snapshot` / `latest-snapshot` (newest snapshot). TuffBox stored these
/// verbatim in the past, and `install_game` then did an exact `.find()`
/// against the version manifest and died with *"Minecraft latest not found
/// in manifest"*. Resolving up front means a project pinned to `release`
/// always launches the current stable build, and never trips that cryptic
/// error. Concrete version strings (e.g. `1.20.1`) pass through untouched,
/// so no network call is made for the common case.
pub fn resolve_minecraft_version_alias(alias: &str) -> Result<String, VersionsError> {
    match alias {
        "latest" | "release" | "latest-release" => Ok(latest_release()?),
        "snapshot" | "latest-snapshot" => Ok(latest_snapshot()?),
        other => Ok(other.to_string()),
    }
}

fn fetch_manifest_v2() -> Result<Manifest, VersionsError> {
    crate::http::get_json_with_context(VERSION_MANIFEST_URL).map_err(VersionsError::Other)
}

fn latest_release() -> Result<String, VersionsError> {
    let manifest = fetch_manifest_v2()?;
    manifest
        .latest
        .map(|l| l.release)
        .ok_or_else(|| VersionsError::Other("version manifest has no 'latest.release'".into()))
}

fn latest_snapshot() -> Result<String, VersionsError> {
    let manifest = fetch_manifest_v2()?;
    manifest
        .latest
        .map(|l| l.snapshot)
        .ok_or_else(|| VersionsError::Other("version manifest has no 'latest.snapshot'".into()))
}

/// Like [`resolve_minecraft_version_alias`], but when the network is
/// unavailable it falls back to scanning `<launcher_dir>/versions` for an
/// already-installed version that satisfies the alias. This lets a project
/// pinned to `latest` / `snapshot` still launch fully offline, provided it
/// has been installed before — without this, the manifest fetch at the top
/// of `install_game` would fail before the on-disk cache could be used.
pub fn resolve_minecraft_version_alias_offline(
    alias: &str,
    launcher_dir: &Path,
) -> Result<String, VersionsError> {
    match resolve_minecraft_version_alias(alias) {
        Ok(v) => Ok(v),
        Err(net_err) => {
            // `resolve_minecraft_version_alias` only returns Err for real
            // aliases (concrete versions never hit the network), so a failure
            // here means the manifest fetch failed — try the local cache.
            let kind = if matches!(alias, "snapshot" | "latest-snapshot") {
                "snapshot"
            } else {
                "release"
            };
            resolve_alias_offline(alias, launcher_dir, kind).map_err(|_| net_err)
        }
    }
}

/// Scans locally installed version JSONs and returns the newest one whose
/// Mojang `type` matches `kind` (`release` or `snapshot`).
fn resolve_alias_offline(
    alias: &str,
    launcher_dir: &Path,
    kind: &str,
) -> Result<String, VersionsError> {
    #[derive(Deserialize)]
    struct MiniVersion {
        id: String,
        #[serde(rename = "type")]
        kind: Option<String>,
    }

    let versions_dir = launcher_dir.join("versions");
    let mut best: Option<(String, String)> = None; // (id, id-for-sorting)
    let Ok(entries) = fs::read_dir(&versions_dir) else {
        return Err(VersionsError::Other(format!(
            "no locally installed versions under {} (offline)",
            versions_dir.display()
        )));
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().into_owned();
        let json_path = path.join(format!("{id}.json"));
        let Ok(contents) = fs::read_to_string(&json_path) else {
            continue;
        };
        let Ok(mini) = serde_json::from_str::<MiniVersion>(&contents) else {
            continue;
        };
        if mini.kind.as_deref() != Some(kind) {
            continue;
        }
        match &best {
            None => best = Some((mini.id.clone(), id)),
            Some((_, best_id)) => {
                if compare_versions(&id, best_id) == std::cmp::Ordering::Greater {
                    best = Some((mini.id.clone(), id));
                }
            }
        }
    }
    best.map(|(id, _)| id).ok_or_else(|| {
        VersionsError::Other(format!(
            "no locally installed {kind} version found for alias '{alias}' (offline)"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concrete_version_passes_through_without_network() {
        // This must not hit the network: `resolve_minecraft_version_alias`
        // only queries the version manifest for real aliases, so a concrete
        // id is returned verbatim and offline launches stay offline.
        for v in ["1.20.1", "1.12.2", "1.21.1", "26.2"] {
            assert_eq!(resolve_minecraft_version_alias(v).unwrap(), v);
        }
    }

    #[test]
    fn unknown_aliases_are_not_special_cased() {
        // Anything that isn't a recognized alias passes through untouched
        // (e.g. a loader-ish string someone typo'd into the version field).
        assert_eq!(resolve_minecraft_version_alias("fabric").unwrap(), "fabric");
    }

    #[test]
    #[ignore = "hits the network (Mojang version manifest)"]
    fn known_aliases_are_recognized() {
        // We can't assert the resolved value (it changes over time / needs
        // network), but we can assert each known alias maps to *some*
        // concrete version and not back to the alias string.
        for alias in [
            "latest",
            "release",
            "latest-release",
            "snapshot",
            "latest-snapshot",
        ] {
            let resolved = resolve_minecraft_version_alias(alias).unwrap();
            assert_ne!(
                resolved, alias,
                "alias {alias} should resolve to a concrete version"
            );
        }
    }

    #[test]
    fn offline_alias_picks_newest_installed_of_kind() {
        // Deterministically exercises the local-scan fallback (no network).
        let base = std::env::temp_dir().join(format!("tuffbox_va_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let versions_dir = base.join("versions");
        for (id, kind) in [
            ("1.20.1", "release"),
            ("1.21.5", "release"),
            ("25w14craftmine", "snapshot"),
        ] {
            let dir = versions_dir.join(id);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join(format!("{id}.json")),
                format!("{{\"id\":\"{id}\",\"type\":\"{kind}\"}}"),
            )
            .unwrap();
        }

        // newest installed release wins for `latest`
        assert_eq!(
            resolve_alias_offline("latest", &base, "release").unwrap(),
            "1.21.5"
        );
        // snapshots are picked from the snapshot pool only
        assert_eq!(
            resolve_alias_offline("snapshot", &base, "snapshot").unwrap(),
            "25w14craftmine"
        );
        // a kind with no installed matches errors rather than cross-contaminating
        let empty = std::env::temp_dir().join(format!("tuffbox_va_empty_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&empty);
        std::fs::create_dir_all(empty.join("versions")).unwrap();
        assert!(resolve_alias_offline("latest", &empty, "release").is_err());
        let _ = std::fs::remove_dir_all(&empty);

        let _ = std::fs::remove_dir_all(&base);
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct LoaderVersion {
    pub id: String,
    pub stable: bool,
}

pub fn fetch_loader_versions(loader: &str, minecraft_version: &str) -> Vec<LoaderVersion> {
    // Loader metadata changes rarely; cache per loader+MC so switching back
    // and forth between Setup tabs (or changing MC version) doesn't re-hit
    // the meta endpoints on every open.
    let key = format!("versions:loader:{loader}:{minecraft_version}");
    let loader = loader.to_string();
    let mc = minecraft_version.to_string();
    crate::api_cache::get_or_insert_with_ttl(
        key.as_str(),
        std::time::Duration::from_secs(10 * 60),
        move || -> Vec<LoaderVersion> {
            match loader.as_str() {
                "fabric" => fetch_fabric_versions(&mc),
                "quilt" => fetch_quilt_versions(&mc),
                "forge" => fetch_forge_versions(&mc),
                "neoforge" => fetch_neoforge_versions(&mc),
                _ => Ok(Vec::new()),
            }
            .unwrap_or_else(|e| {
                eprintln!(
                    "[versions] loader fetch failed for {loader}/{mc}, serving empty list: {e}"
                );
                Vec::new()
            })
        },
    )
}

fn fetch_fabric_versions(mc: &str) -> Result<Vec<LoaderVersion>, VersionsError> {
    let url = format!("https://meta.fabricmc.net/v2/versions/loader/{mc}");
    let entries: Vec<FabricLoaderEntry> =
        crate::http::get_json_with_context(&url).map_err(VersionsError::Other)?;
    Ok(entries
        .into_iter()
        .map(|e| LoaderVersion {
            id: e.loader.version,
            stable: e.loader.stable,
        })
        .collect())
}

#[derive(Debug, Deserialize)]
struct FabricLoaderEntry {
    loader: FabricLoader,
}

#[derive(Debug, Deserialize)]
struct FabricLoader {
    version: String,
    stable: bool,
}

fn fetch_quilt_versions(mc: &str) -> Result<Vec<LoaderVersion>, VersionsError> {
    let url = format!("https://meta.quiltmc.org/v3/versions/loader/{mc}");
    let entries: Vec<QuiltLoaderEntry> =
        crate::http::get_json_with_context(&url).map_err(VersionsError::Other)?;
    Ok(entries
        .into_iter()
        .map(|e| LoaderVersion {
            id: e.loader.version,
            stable: true,
        })
        .collect())
}

#[derive(Debug, Deserialize)]
struct QuiltLoaderEntry {
    loader: QuiltLoader,
}

#[derive(Debug, Deserialize)]
struct QuiltLoader {
    version: String,
}

#[derive(Debug, Deserialize)]
struct ModrinthLoaderManifest {
    game_versions: Vec<ModrinthGameVersion>,
}

#[derive(Debug, Deserialize)]
struct ModrinthGameVersion {
    id: String,
    #[serde(default)]
    #[allow(dead_code)]
    stable: bool,
    loaders: Vec<ModrinthLoaderEntry>,
}

#[derive(Debug, Deserialize)]
struct ModrinthLoaderEntry {
    id: String,
    #[allow(dead_code)]
    url: String,
    #[serde(default)]
    #[allow(dead_code)]
    stable: bool,
}

fn fetch_forge_versions(mc: &str) -> Result<Vec<LoaderVersion>, VersionsError> {
    let url = "https://launcher-meta.modrinth.com/forge/v0/manifest.json";
    let manifest: ModrinthLoaderManifest =
        crate::http::get_json_with_context(url).map_err(VersionsError::Other)?;
    Ok(manifest
        .game_versions
        .into_iter()
        .find(|v| v.id == mc)
        .map(|v| {
            v.loaders
                .into_iter()
                .map(|e| LoaderVersion {
                    id: e.id,
                    stable: e.stable,
                })
                .collect()
        })
        .unwrap_or_default())
}

fn fetch_neoforge_versions(mc: &str) -> Result<Vec<LoaderVersion>, VersionsError> {
    let prefix = neoforge_prefix(mc)?;
    let url = "https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml";
    let text = crate::http::get_text(url)?;
    let mut versions: Vec<LoaderVersion> = text
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if let Some(start) = line.find("<version>") {
                let end = line.find("</version>")?;
                let ver = &line[start + 9..end];
                if ver.starts_with(&prefix) {
                    return Some(LoaderVersion {
                        id: ver.to_string(),
                        stable: !ver.contains("-beta"),
                    });
                }
            }
            None
        })
        .collect();
    versions.reverse();
    Ok(versions)
}

fn neoforge_prefix(mc: &str) -> Result<String, VersionsError> {
    let parts: Vec<&str> = mc.split('.').collect();
    if parts.len() < 2 {
        return Err(VersionsError::ParseError(format!(
            "Cannot derive NeoForge prefix from Minecraft version {mc}"
        )));
    }
    let minor = parts[1].parse::<u32>().map_err(|_| {
        VersionsError::ParseError(format!("Invalid Minecraft minor version in {mc}"))
    })?;
    let patch = parts
        .get(2)
        .and_then(|p| p.parse::<u32>().ok())
        .unwrap_or(0);
    Ok(format!("{minor}.{patch}."))
}
