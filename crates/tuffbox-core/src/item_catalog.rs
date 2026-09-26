//! Full item catalog from vanilla + mod JARs (models + lang), not only recipe I/O.

use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zip::ZipArchive;

use crate::manifest::ProjectManifest;

/// Cached catalogs with very few entries usually mean the vanilla client jar was missing.
const MIN_TRUSTED_CATALOG_ITEMS: usize = 64;

const CATALOG_CACHE_VERSION: &str = "item-catalog-v4";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogItemEntry {
    pub id: String,
    pub name: String,
    pub mod_ns: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CatalogCache {
    fingerprint: String,
    items: Vec<CatalogItemEntry>,
}

/// Build a catalog of item ids from jar assets (models, 1.21.4+ item defs, lang) plus extras.
/// Merges any extra ids (e.g. from recipes) that do not appear in jars.
pub fn build_item_catalog(
    project_dir: &Path,
    mc_version: &str,
    extra_ids: impl IntoIterator<Item = String>,
    extra_vanilla_roots: &[PathBuf],
) -> Result<Vec<CatalogItemEntry>, String> {
    let jars = catalog_jar_sources(project_dir, mc_version, extra_vanilla_roots);
    let fingerprint = catalog_fingerprint(mc_version, &jars);
    let cache_path = item_catalog_cache_path(project_dir);

    let mut by_id: BTreeMap<String, CatalogItemEntry> = BTreeMap::new();

    if let Ok(raw) = std::fs::read_to_string(&cache_path) {
        if let Ok(cached) = serde_json::from_str::<CatalogCache>(&raw) {
            if cached.fingerprint == fingerprint
                && catalog_cache_is_trustworthy(&cached.items, &jars)
            {
                by_id = cached
                    .items
                    .into_iter()
                    .map(|e| (e.id.clone(), e))
                    .collect();
            }
        }
    }

    if by_id.is_empty() {
        let mut lang: HashMap<String, String> = HashMap::new();
        for jar in &jars {
            let Ok(file) = std::fs::File::open(jar) else {
                continue;
            };
            let Ok(mut archive) = ZipArchive::new(file) else {
                continue;
            };
            collect_lang_from_archive(&mut archive, &mut lang);
            collect_models_from_archive(&mut archive, &mut by_id, &lang);
            collect_item_defs_from_archive(&mut archive, &mut by_id, &lang);
        }
        collect_ids_from_lang(&lang, &mut by_id);

        if !by_id.is_empty() {
            if let Some(parent) = cache_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let payload = CatalogCache {
                fingerprint: fingerprint.clone(),
                items: by_id.values().cloned().collect(),
            };
            if let Ok(json) = serde_json::to_string(&payload) {
                let _ = std::fs::write(&cache_path, json);
            }
        }
    }

    for id in extra_ids {
        let id = id.trim().to_string();
        if id.is_empty() || id.starts_with('#') || id == "unknown:unknown" {
            continue;
        }
        by_id.entry(id.clone()).or_insert_with(|| CatalogItemEntry {
            name: prettify_item_id(&id),
            mod_ns: namespace_of(&id),
            id,
        });
    }

    Ok(by_id.into_values().collect())
}

/// Resolve project dir + mc version from a manifest path, then build the catalog.
pub fn build_item_catalog_for_manifest(
    manifest_path: &Path,
    extra_ids: impl IntoIterator<Item = String>,
    extra_vanilla_roots: &[PathBuf],
) -> Result<Vec<CatalogItemEntry>, String> {
    let project_dir = manifest_path
        .parent()
        .ok_or_else(|| "manifest path has no parent".to_string())?;
    let manifest = ProjectManifest::load_from_path(manifest_path).map_err(|e| e.to_string())?;
    build_item_catalog(
        project_dir,
        &manifest.minecraft.version,
        extra_ids,
        extra_vanilla_roots,
    )
}

fn item_catalog_cache_path(project_dir: &Path) -> PathBuf {
    project_dir
        .join(".tuffbox")
        .join("cache")
        .join(format!("{CATALOG_CACHE_VERSION}.json"))
}

/// Last written catalog, if any. Does not open jars or check fingerprints —
/// click-path callers should treat this as possibly stale until a background warm.
pub fn load_cached_item_catalog(project_dir: &Path) -> Option<Vec<CatalogItemEntry>> {
    let raw = std::fs::read_to_string(item_catalog_cache_path(project_dir)).ok()?;
    let cached: CatalogCache = serde_json::from_str(&raw).ok()?;
    if cached.items.is_empty() {
        return None;
    }
    Some(cached.items)
}

fn catalog_cache_is_trustworthy(items: &[CatalogItemEntry], jars: &[PathBuf]) -> bool {
    if items.is_empty() {
        return false;
    }
    if jars.is_empty() {
        return true;
    }
    items.len() >= MIN_TRUSTED_CATALOG_ITEMS
}

/// Built-in launcher directories searched for an installed vanilla client jar.
pub fn default_vanilla_jar_roots() -> Vec<PathBuf> {
    default_vanilla_roots()
}

/// Merge TuffBox runtime / launcher paths with the built-in vanilla jar search roots.
pub fn merge_vanilla_jar_roots(extra_roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots = default_vanilla_roots();
    for root in extra_roots {
        if !roots.iter().any(|existing| existing == root) {
            roots.push(root.clone());
        }
    }
    roots
}

/// Resolve installed vanilla `{version}/{version}.jar` paths for a Minecraft version id or alias.
pub fn resolve_vanilla_client_jars(mc_version: &str, extra_roots: &[PathBuf]) -> Vec<PathBuf> {
    vanilla_client_jars(mc_version, extra_roots)
}

fn catalog_jar_sources(
    project_dir: &Path,
    mc_version: &str,
    extra_vanilla_roots: &[PathBuf],
) -> Vec<PathBuf> {
    // Detection only — never download here. The desktop UI prompts the user
    // and then calls [`download_vanilla_client_jar`] explicitly.
    let mut jars = vanilla_client_jars(mc_version, extra_vanilla_roots);
    let mods_dir = project_dir.join("mods");
    if mods_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&mods_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name.ends_with(".jar") && !name.ends_with(".disabled") {
                    jars.push(path);
                }
            }
        }
    }
    jars
}

fn default_vanilla_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(appdata) = std::env::var_os("APPDATA") {
        roots.push(PathBuf::from(&appdata).join("TuffBox"));
        roots.push(PathBuf::from(appdata).join(".minecraft"));
    }
    if let Some(home) = std::env::var_os("HOME") {
        roots.push(PathBuf::from(&home).join(".local/share/TuffBox"));
        roots.push(PathBuf::from(home).join(".minecraft"));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        roots.push(PathBuf::from(local).join("TuffBox"));
    }
    if let Some(data) = dirs::data_dir() {
        roots.push(data.join("TuffBox"));
    }
    roots
}

fn resolve_mc_version_for_jars(mc_version: &str, roots: &[PathBuf]) -> String {
    for root in roots {
        if let Ok(resolved) =
            crate::versions::resolve_minecraft_version_alias_offline(mc_version, root)
        {
            return resolved;
        }
    }
    crate::versions::resolve_minecraft_version_alias(mc_version)
        .unwrap_or_else(|_| mc_version.to_string())
}

fn vanilla_client_jars(mc_version: &str, extra_roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut jars = Vec::new();
    let roots = merge_vanilla_jar_roots(extra_roots);
    let resolved = resolve_mc_version_for_jars(mc_version, &roots);
    for root in roots {
        let jar = root
            .join("versions")
            .join(&resolved)
            .join(format!("{resolved}.jar"));
        if jar.is_file() && !jars.iter().any(|p| p == &jar) {
            jars.push(jar);
        }
    }
    jars
}

#[derive(Debug, Deserialize)]
struct VanillaVersionClientJson {
    downloads: VanillaVersionDownloads,
}

#[derive(Debug, Deserialize)]
struct VanillaVersionDownloads {
    client: VanillaClientArtifact,
}

#[derive(Debug, Deserialize)]
struct VanillaClientArtifact {
    url: String,
    sha1: String,
    #[serde(default)]
    size: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct MojangVersionManifest {
    versions: Vec<MojangVersionEntry>,
}

#[derive(Debug, Deserialize)]
struct MojangVersionEntry {
    id: String,
    url: String,
}

/// Status of the installed vanilla client jar for a Minecraft version.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VanillaClientJarStatus {
    pub found: bool,
    /// Version id from the project manifest (may be an alias).
    pub version: String,
    /// Concrete version used for the jar path (aliases resolved).
    pub resolved_version: String,
    pub jar_path: Option<String>,
    /// Approximate download size in bytes when known from a cached version JSON.
    pub download_size: Option<u64>,
}

/// Look up whether the vanilla client jar for `mc_version` is already on disk.
pub fn vanilla_client_jar_status(
    mc_version: &str,
    extra_roots: &[PathBuf],
) -> VanillaClientJarStatus {
    let roots = merge_vanilla_jar_roots(extra_roots);
    let resolved = resolve_mc_version_for_jars(mc_version, &roots);
    let jars = vanilla_client_jars(mc_version, extra_roots);
    let jar_path = jars.first().map(|p| p.to_string_lossy().into_owned());
    let download_size = if jar_path.is_none() {
        read_client_size_hint(&resolved, &roots)
    } else {
        None
    };
    VanillaClientJarStatus {
        found: jar_path.is_some(),
        version: mc_version.to_string(),
        resolved_version: resolved,
        jar_path,
        download_size,
    }
}

fn read_client_size_hint(resolved: &str, roots: &[PathBuf]) -> Option<u64> {
    for root in roots {
        let json_path = root
            .join("versions")
            .join(resolved)
            .join(format!("{resolved}.json"));
        let Ok(raw) = std::fs::read_to_string(json_path) else {
            continue;
        };
        if let Ok(meta) = serde_json::from_str::<VanillaVersionClientJson>(&raw) {
            if let Some(size) = meta.downloads.client.size {
                return Some(size);
            }
        }
    }
    None
}

fn load_or_fetch_client_meta(
    resolved: &str,
    version_dir: &Path,
    search_roots: &[PathBuf],
) -> Result<(String, String), String> {
    let version_json = version_dir.join(format!("{resolved}.json"));
    if version_json.is_file() {
        let raw = std::fs::read_to_string(&version_json).map_err(|e| e.to_string())?;
        let meta: VanillaVersionClientJson =
            serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        return Ok((meta.downloads.client.url, meta.downloads.client.sha1));
    }

    for root in search_roots {
        let candidate = root
            .join("versions")
            .join(resolved)
            .join(format!("{resolved}.json"));
        if candidate == version_json || !candidate.is_file() {
            continue;
        }
        let raw = std::fs::read_to_string(&candidate).map_err(|e| e.to_string())?;
        let meta: VanillaVersionClientJson =
            serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(version_dir).map_err(|e| e.to_string())?;
        let _ = std::fs::write(&version_json, &raw);
        return Ok((meta.downloads.client.url, meta.downloads.client.sha1));
    }

    const MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
    let manifest: MojangVersionManifest = crate::http::get_json(MANIFEST_URL)
        .map_err(|e| format!("failed to fetch Mojang version manifest: {e}"))?;
    let entry = manifest
        .versions
        .into_iter()
        .find(|v| v.id == resolved)
        .ok_or_else(|| format!("Minecraft {resolved} not found in Mojang version manifest"))?;
    let raw = crate::http::get_text(&entry.url)
        .map_err(|e| format!("failed to fetch version JSON for {resolved}: {e}"))?;
    let meta: VanillaVersionClientJson = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(version_dir).map_err(|e| e.to_string())?;
    std::fs::write(&version_json, &raw).map_err(|e| e.to_string())?;
    Ok((meta.downloads.client.url, meta.downloads.client.sha1))
}

/// Download (or resume) the vanilla client jar for `mc_version` into `install_root`.
///
/// Searches `extra_search_roots` first; if a jar is already present anywhere,
/// returns that path without downloading. Otherwise writes under
/// `{install_root}/versions/{resolved}/{resolved}.jar`, resuming any stranded
/// `.tuffbox.part` via HTTP Range and verifying the Mojang sha1.
pub fn download_vanilla_client_jar(
    mc_version: &str,
    install_root: &Path,
    extra_search_roots: &[PathBuf],
) -> Result<PathBuf, String> {
    let mut search = vec![install_root.to_path_buf()];
    for root in extra_search_roots {
        if !search.iter().any(|p| p == root) {
            search.push(root.clone());
        }
    }
    let roots = merge_vanilla_jar_roots(&search);
    let resolved = resolve_mc_version_for_jars(mc_version, &roots);

    if let Some(existing) = vanilla_client_jars(mc_version, &search).into_iter().next() {
        return Ok(existing);
    }

    let version_dir = install_root.join("versions").join(&resolved);
    let jar = version_dir.join(format!("{resolved}.jar"));
    let (url, sha1) = load_or_fetch_client_meta(&resolved, &version_dir, &roots)?;
    crate::mc_install::download_with_sha1(&url, &jar, Some(&sha1))
        .map_err(|e| format!("failed to download Minecraft {resolved} client jar: {e}"))?;
    if !jar.is_file() {
        return Err(format!(
            "download finished but {} is missing",
            jar.display()
        ));
    }
    Ok(jar)
}

/// Best-effort download used by callers that prefer a Vec of jars over Result.
/// Prefer [`download_vanilla_client_jar`] when the UI needs to surface errors.
pub fn ensure_vanilla_client_jars(mc_version: &str, extra_roots: &[PathBuf]) -> Vec<PathBuf> {
    let jars = vanilla_client_jars(mc_version, extra_roots);
    if !jars.is_empty() {
        return jars;
    }
    let install_root = extra_roots
        .first()
        .cloned()
        .or_else(|| default_vanilla_roots().into_iter().next());
    let Some(root) = install_root else {
        return Vec::new();
    };
    match download_vanilla_client_jar(mc_version, &root, extra_roots) {
        Ok(jar) => vec![jar],
        Err(_) => Vec::new(),
    }
}

/// Resolve project Minecraft version + jar status from a manifest path.
pub fn vanilla_client_jar_status_for_manifest(
    manifest_path: &Path,
    extra_roots: &[PathBuf],
) -> Result<VanillaClientJarStatus, String> {
    let manifest = ProjectManifest::load_from_path(manifest_path).map_err(|e| e.to_string())?;
    Ok(vanilla_client_jar_status(
        &manifest.minecraft.version,
        extra_roots,
    ))
}

/// Download the client jar for the Minecraft version declared in a project manifest.
pub fn download_vanilla_client_jar_for_manifest(
    manifest_path: &Path,
    install_root: &Path,
    extra_roots: &[PathBuf],
) -> Result<PathBuf, String> {
    let manifest = ProjectManifest::load_from_path(manifest_path).map_err(|e| e.to_string())?;
    download_vanilla_client_jar(&manifest.minecraft.version, install_root, extra_roots)
}

fn catalog_fingerprint(mc_version: &str, jars: &[PathBuf]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(CATALOG_CACHE_VERSION.as_bytes());
    hasher.update(mc_version.as_bytes());
    for jar in jars {
        hasher.update(jar.to_string_lossy().as_bytes());
        if let Ok(meta) = std::fs::metadata(jar) {
            hasher.update(meta.len().to_le_bytes());
            if let Ok(modified) = meta.modified() {
                if let Ok(dur) = modified.duration_since(std::time::UNIX_EPOCH) {
                    hasher.update(dur.as_secs().to_le_bytes());
                }
            }
        }
    }
    format!("{:x}", hasher.finalize())
}

fn normalize_zip_path(name: &str) -> String {
    name.replace('\\', "/")
}

fn collect_lang_from_archive(
    archive: &mut ZipArchive<std::fs::File>,
    lang: &mut HashMap<String, String>,
) {
    let names: Vec<String> = archive.file_names().map(|s| s.to_string()).collect();
    for name in names {
        let normalized = normalize_zip_path(&name);
        let lower = normalized.to_ascii_lowercase();
        if !lower.ends_with("/lang/en_us.json") {
            continue;
        }
        let Ok(mut entry) = archive.by_name(&name) else {
            continue;
        };
        let mut content = String::new();
        if entry.read_to_string(&mut content).is_err() || content.len() > 8 * 1024 * 1024 {
            continue;
        }
        let Ok(map) = serde_json::from_str::<HashMap<String, String>>(&content) else {
            continue;
        };
        for (k, v) in map {
            if k.starts_with("item.") || k.starts_with("block.") {
                lang.entry(k).or_insert(v);
            }
        }
    }
}

/// `item.minecraft.diamond` / `block.minecraft.stone` → `minecraft:diamond` / `minecraft:stone`.
fn lang_key_to_item_id(key: &str) -> Option<String> {
    let rest = key
        .strip_prefix("item.")
        .or_else(|| key.strip_prefix("block."))?;
    let mut parts = rest.split('.');
    let ns = parts.next()?;
    if ns.is_empty() {
        return None;
    }
    let path = parts.collect::<Vec<_>>().join("/");
    if path.is_empty() {
        return None;
    }
    Some(format!("{ns}:{path}"))
}

fn collect_ids_from_lang(
    lang: &HashMap<String, String>,
    by_id: &mut BTreeMap<String, CatalogItemEntry>,
) {
    for (key, display) in lang {
        let Some(id) = lang_key_to_item_id(key) else {
            continue;
        };
        let ns = namespace_of(&id);
        let name = if display.is_empty() {
            prettify_item_id(&id)
        } else {
            display.clone()
        };
        by_id.entry(id.clone()).or_insert(CatalogItemEntry {
            id,
            name,
            mod_ns: ns,
        });
    }
}

fn insert_catalog_entry(
    by_id: &mut BTreeMap<String, CatalogItemEntry>,
    lang: &HashMap<String, String>,
    ns: &str,
    path: &str,
) {
    if ns.is_empty() || ns == "_generated" || path.is_empty() {
        return;
    }
    let id = format!("{ns}:{path}");
    let name_disp = lookup_lang_name(lang, ns, path).unwrap_or_else(|| prettify_item_id(&id));
    by_id.entry(id.clone()).or_insert(CatalogItemEntry {
        id,
        name: name_disp,
        mod_ns: ns.to_string(),
    });
}

fn collect_models_from_archive(
    archive: &mut ZipArchive<std::fs::File>,
    by_id: &mut BTreeMap<String, CatalogItemEntry>,
    lang: &HashMap<String, String>,
) {
    let names: Vec<String> = archive.file_names().map(|s| s.to_string()).collect();
    for name in names {
        let normalized = normalize_zip_path(&name);
        // assets/<ns>/models/item/<path>.json — only flat item models.
        // Nested paths like models/item/banner/skull/black.json are model
        // variants (EBE etc.), not registry item ids.
        let parts: Vec<&str> = normalized.split('/').collect();
        if parts.len() != 5 || parts[0] != "assets" || parts[2] != "models" || parts[3] != "item" {
            continue;
        }
        if !normalized.ends_with(".json") {
            continue;
        }
        let ns = parts[1];
        let path = parts[4].trim_end_matches(".json");
        if path.is_empty() || path.contains('/') {
            continue;
        }
        // Skip template / abstract models that are not real items.
        if path.starts_with("template_")
            || path == "generated"
            || path == "handheld"
            || path == "handheld_rod"
            || path == "handheld_mace"
        {
            continue;
        }
        insert_catalog_entry(by_id, lang, ns, path);
    }
}

/// 1.21.4+ item definitions live under assets/<ns>/items/<path>.json.
fn collect_item_defs_from_archive(
    archive: &mut ZipArchive<std::fs::File>,
    by_id: &mut BTreeMap<String, CatalogItemEntry>,
    lang: &HashMap<String, String>,
) {
    let names: Vec<String> = archive.file_names().map(|s| s.to_string()).collect();
    for name in names {
        let normalized = normalize_zip_path(&name);
        let parts: Vec<&str> = normalized.split('/').collect();
        // Only flat registry ids: assets/<ns>/items/<id>.json (no nested folders).
        if parts.len() != 4 || parts[0] != "assets" || parts[2] != "items" {
            continue;
        }
        if !normalized.ends_with(".json") {
            continue;
        }
        let ns = parts[1];
        let path = parts[3].trim_end_matches(".json");
        if path.is_empty() {
            continue;
        }
        insert_catalog_entry(by_id, lang, ns, path);
    }
}

fn lookup_lang_name(lang: &HashMap<String, String>, ns: &str, path: &str) -> Option<String> {
    let dotted = path.replace('/', ".");
    let keys = [
        format!("item.{ns}.{dotted}"),
        format!("block.{ns}.{dotted}"),
        format!("item.{ns}.{path}"),
        format!("block.{ns}.{path}"),
    ];
    for k in keys {
        if let Some(v) = lang.get(&k) {
            if !v.is_empty() {
                return Some(v.clone());
            }
        }
    }
    None
}

fn namespace_of(id: &str) -> String {
    id.split_once(':')
        .map(|(ns, _)| ns.to_string())
        .unwrap_or_else(|| "minecraft".into())
}

fn prettify_item_id(id: &str) -> String {
    let path = id.split_once(':').map(|(_, p)| p).unwrap_or(id);
    path.replace('_', " ").replace('/', " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prettifies_ids() {
        assert_eq!(prettify_item_id("minecraft:oak_planks"), "oak planks");
    }

    #[test]
    fn lang_keys_map_to_item_ids() {
        assert_eq!(
            lang_key_to_item_id("item.minecraft.diamond"),
            Some("minecraft:diamond".into())
        );
        assert_eq!(
            lang_key_to_item_id("block.minecraft.stone"),
            Some("minecraft:stone".into())
        );
        assert_eq!(
            lang_key_to_item_id("item.create.brass_ingot"),
            Some("create:brass_ingot".into())
        );
        assert_eq!(
            lang_key_to_item_id("item.mod.sub.item"),
            Some("mod:sub/item".into())
        );
    }

    #[test]
    fn rejects_tiny_catalog_cache_when_jars_present() {
        let items = vec![CatalogItemEntry {
            id: "mod:christmas_chest".into(),
            name: "Christmas Chest".into(),
            mod_ns: "mod".into(),
        }];
        let jars = vec![PathBuf::from("/tmp/vanilla.jar")];
        assert!(!catalog_cache_is_trustworthy(&items, &jars));
    }

    #[test]
    fn accepts_empty_jar_list_with_any_items() {
        let items = vec![CatalogItemEntry {
            id: "mod:christmas_chest".into(),
            name: "Christmas Chest".into(),
            mod_ns: "mod".into(),
        }];
        assert!(catalog_cache_is_trustworthy(&items, &[]));
    }

    #[test]
    fn load_cached_item_catalog_skips_zip_and_returns_file() {
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path().join(".tuffbox").join("cache");
        std::fs::create_dir_all(&cache_dir).unwrap();
        let items = vec![CatalogItemEntry {
            id: "minecraft:stone".into(),
            name: "Stone".into(),
            mod_ns: "minecraft".into(),
        }];
        let cached = CatalogCache {
            fingerprint: "stale".into(),
            items: items.clone(),
        };
        std::fs::write(
            item_catalog_cache_path(dir.path()),
            serde_json::to_string(&cached).unwrap(),
        )
        .unwrap();
        let loaded = load_cached_item_catalog(dir.path()).expect("cache");
        assert_eq!(loaded[0].id, "minecraft:stone");
        assert!(load_cached_item_catalog(&dir.path().join("missing")).is_none());
    }

    // A version id that can never exist in the machine's real launcher roots,
    // so tests stay hermetic even on a box with Minecraft installed.
    const TEST_VERSION: &str = "0.0.0-tuffbox-test";

    #[test]
    fn ensure_returns_existing_jar_without_network() {
        let dir = tempfile::tempdir().unwrap();
        let vdir = dir.path().join("versions").join(TEST_VERSION);
        std::fs::create_dir_all(&vdir).unwrap();
        let jar = vdir.join(format!("{TEST_VERSION}.jar"));
        std::fs::write(&jar, b"fake").unwrap();
        let roots = [dir.path().to_path_buf()];
        let found = ensure_vanilla_client_jars(TEST_VERSION, &roots);
        assert_eq!(found, vec![jar]);
    }

    #[test]
    fn ensure_swallows_failed_download_and_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let vdir = dir.path().join("versions").join(TEST_VERSION);
        std::fs::create_dir_all(&vdir).unwrap();
        // Cached version JSON whose URL refuses connections instantly (port 1).
        std::fs::write(
            vdir.join(format!("{TEST_VERSION}.json")),
            r#"{"downloads":{"client":{"url":"http://127.0.0.1:1/client.jar","sha1":"deadbeef"}}}"#,
        )
        .unwrap();
        let roots = [dir.path().to_path_buf()];
        let found = ensure_vanilla_client_jars(TEST_VERSION, &roots);
        assert!(found.is_empty());
        assert!(!vdir.join(format!("{TEST_VERSION}.jar")).exists());
    }

    #[test]
    fn status_reports_missing_jar() {
        let dir = tempfile::tempdir().unwrap();
        let roots = [dir.path().to_path_buf()];
        let status = vanilla_client_jar_status(TEST_VERSION, &roots);
        assert!(!status.found);
        assert_eq!(status.resolved_version, TEST_VERSION);
        assert!(status.jar_path.is_none());
    }

    #[test]
    fn status_reports_found_jar() {
        let dir = tempfile::tempdir().unwrap();
        let vdir = dir.path().join("versions").join(TEST_VERSION);
        std::fs::create_dir_all(&vdir).unwrap();
        let jar = vdir.join(format!("{TEST_VERSION}.jar"));
        std::fs::write(&jar, b"fake").unwrap();
        let roots = [dir.path().to_path_buf()];
        let status = vanilla_client_jar_status(TEST_VERSION, &roots);
        assert!(status.found);
        assert_eq!(status.jar_path.as_deref(), Some(jar.to_str().unwrap()));
    }
}
