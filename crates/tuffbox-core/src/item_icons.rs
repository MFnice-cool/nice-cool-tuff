//! Resolves Minecraft item icons from mod jars and the installed client jar.

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use zip::ZipArchive;

pub fn item_icon_cache_dir(project_dir: &Path) -> PathBuf {
    project_dir
        .join(".tuffbox")
        .join("cache")
        .join("item-icons")
}

fn cache_file_for(project_dir: &Path, item_id: &str) -> PathBuf {
    let safe = item_id
        .replace(':', "__")
        .replace('#', "_tag_")
        .replace('/', "_");
    item_icon_cache_dir(project_dir).join(format!("{safe}.png"))
}

fn parse_item_id(item_id: &str) -> Option<(String, String)> {
    let trimmed = item_id.trim();
    let id = trimmed.strip_prefix('#').unwrap_or(trimmed);
    if id.is_empty() || id.contains(' ') {
        return None;
    }
    let (namespace, path) = id.split_once(':')?;
    if namespace.is_empty() || path.is_empty() {
        return None;
    }
    Some((namespace.to_string(), path.to_string()))
}

fn is_png(bytes: &[u8]) -> bool {
    bytes.len() >= 8 && bytes.starts_with(&[137, 80, 78, 71, 13, 10, 26, 10])
}

fn read_zip_entry(archive: &mut ZipArchive<File>, path: &str) -> Option<Vec<u8>> {
    let mut file = archive.by_name(path).ok()?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).ok()?;
    if is_png(&buf) {
        Some(buf)
    } else {
        None
    }
}

fn read_zip_text(archive: &mut ZipArchive<File>, path: &str) -> Option<String> {
    let mut file = archive.by_name(path).ok()?;
    let mut content = String::new();
    file.read_to_string(&mut content).ok()?;
    Some(content)
}

fn split_namespaced<'a>(value: &'a str, default_ns: &'a str) -> (&'a str, &'a str) {
    if let Some((ns, path)) = value.split_once(':') {
        (ns, path)
    } else {
        (default_ns, value)
    }
}

/// `minecraft:item/generated` → `assets/minecraft/models/item/generated.json`
/// `item/generated` → `assets/<ns>/models/item/generated.json`
#[cfg(test)]
fn model_zip_path(default_ns: &str, model_ref: &str) -> String {
    let (ns, path) = split_namespaced(model_ref, default_ns);
    if path.starts_with("item/") || path.starts_with("block/") {
        format!("assets/{ns}/models/{path}.json")
    } else {
        format!("assets/{ns}/models/item/{path}.json")
    }
}

fn texture_zip_path(default_ns: &str, texture_ref: &str) -> Vec<String> {
    let (ns, path) = split_namespaced(texture_ref, default_ns);
    let mut candidates = Vec::new();
    if path.starts_with("item/") || path.starts_with("block/") {
        candidates.push(format!("assets/{ns}/textures/{path}.png"));
    } else {
        candidates.push(format!("assets/{ns}/textures/item/{path}.png"));
        candidates.push(format!("assets/{ns}/textures/block/{path}.png"));
        candidates.push(format!("assets/{ns}/textures/{path}.png"));
    }
    candidates
}

fn read_png_any(archives: &mut [ZipArchive<File>], zip_path: &str) -> Option<Vec<u8>> {
    for archive in archives.iter_mut() {
        if let Some(png) = read_zip_entry(archive, zip_path) {
            return Some(png);
        }
    }
    None
}

fn read_text_any(archives: &mut [ZipArchive<File>], zip_path: &str) -> Option<String> {
    for archive in archives.iter_mut() {
        if let Some(text) = read_zip_text(archive, zip_path) {
            return Some(text);
        }
    }
    None
}

fn resolve_from_model(
    archives: &mut [ZipArchive<File>],
    namespace: &str,
    item_path: &str,
    depth: u8,
) -> Option<Vec<u8>> {
    if depth > 10 {
        return None;
    }

    // Prefer 1.21.4+ item definitions (`assets/<ns>/items/<id>.json`), then classic models.
    let item_def = read_text_any(
        archives,
        &format!("assets/{namespace}/items/{item_path}.json"),
    );
    if let Some(content) = item_def {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(png) = resolve_modern_item_def(archives, namespace, &json, depth) {
                return Some(png);
            }
        }
    }

    resolve_model_file(archives, namespace, &format!("item/{item_path}"), depth)
}

/// Follow a namespaced model reference like `minecraft:item/diamond` or
/// `minecraft:block/blackstone_wall_inventory`.
fn resolve_model_ref(
    archives: &mut [ZipArchive<File>],
    default_ns: &str,
    model_ref: &str,
    depth: u8,
) -> Option<Vec<u8>> {
    let (ns, path) = split_namespaced(model_ref, default_ns);
    if path.starts_with("item/") || path.starts_with("block/") {
        resolve_model_file(archives, ns, path, depth)
    } else {
        // Bare id → assume item model
        resolve_model_file(archives, ns, &format!("item/{path}"), depth)
    }
}

fn resolve_model_file(
    archives: &mut [ZipArchive<File>],
    namespace: &str,
    model_path: &str,
    depth: u8,
) -> Option<Vec<u8>> {
    if depth > 10 {
        return None;
    }
    let zip_path = format!("assets/{namespace}/models/{model_path}.json");
    let content = read_text_any(archives, &zip_path)?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;
    extract_texture_from_model_json(archives, namespace, &json, depth)
}

/// 1.21.4+ `assets/<ns>/items/<id>.json` — walk `model` / `fallback` / select entries.
fn resolve_modern_item_def(
    archives: &mut [ZipArchive<File>],
    namespace: &str,
    json: &serde_json::Value,
    depth: u8,
) -> Option<Vec<u8>> {
    if let Some(model) = json.get("model") {
        return resolve_modern_model_node(archives, namespace, model, depth);
    }
    None
}

fn resolve_modern_model_node(
    archives: &mut [ZipArchive<File>],
    namespace: &str,
    node: &serde_json::Value,
    depth: u8,
) -> Option<Vec<u8>> {
    if depth > 10 {
        return None;
    }
    // String shortcut (rare)
    if let Some(model_ref) = node.as_str() {
        return resolve_model_ref(archives, namespace, model_ref, depth + 1);
    }
    let obj = node.as_object()?;

    if let Some(model_ref) = obj.get("model").and_then(|v| v.as_str()) {
        if let Some(png) = resolve_model_ref(archives, namespace, model_ref, depth + 1) {
            return Some(png);
        }
    }

    // minecraft:special — use the `base` item model (e.g. banners, shields, beds).
    if let Some(base_ref) = obj.get("base").and_then(|v| v.as_str()) {
        if let Some(png) = resolve_model_ref(archives, namespace, base_ref, depth + 1) {
            return Some(png);
        }
    }

    // minecraft:condition
    for key in ["on_true", "on_false"] {
        if let Some(child) = obj.get(key) {
            if let Some(png) = resolve_modern_model_node(archives, namespace, child, depth + 1) {
                return Some(png);
            }
        }
    }

    // minecraft:select — prefer fallback, then first entry
    if let Some(fallback) = obj.get("fallback") {
        if let Some(png) = resolve_modern_model_node(archives, namespace, fallback, depth + 1) {
            return Some(png);
        }
    }
    if let Some(entries) = obj.get("entries").and_then(|v| v.as_array()) {
        for entry in entries {
            if let Some(model) = entry.get("model") {
                if let Some(png) = resolve_modern_model_node(archives, namespace, model, depth + 1)
                {
                    return Some(png);
                }
            }
        }
    }

    // minecraft:composite
    if let Some(models) = obj.get("models").and_then(|v| v.as_array()) {
        for model in models {
            if let Some(png) = resolve_modern_model_node(archives, namespace, model, depth + 1) {
                return Some(png);
            }
        }
    }

    None
}

fn extract_texture_from_model_json(
    archives: &mut [ZipArchive<File>],
    namespace: &str,
    json: &serde_json::Value,
    depth: u8,
) -> Option<Vec<u8>> {
    if let Some(textures) = json.get("textures").and_then(|value| value.as_object()) {
        for key in [
            "layer0", "layer1", "particle", "all", "texture", "side", "end", "wall", "cross",
            "crop", "plant", "fire", "fan", "wool", "pane",
        ] {
            let Some(texture_ref) = textures.get(key).and_then(|value| value.as_str()) else {
                continue;
            };
            for zip_path in texture_zip_path(namespace, texture_ref) {
                if let Some(png) = read_png_any(archives, &zip_path) {
                    return Some(png);
                }
            }
        }
        for (_key, value) in textures {
            let Some(texture_ref) = value.as_str() else {
                continue;
            };
            for zip_path in texture_zip_path(namespace, texture_ref) {
                if let Some(png) = read_png_any(archives, &zip_path) {
                    return Some(png);
                }
            }
        }
    }

    let Some(parent) = json.get("parent").and_then(|value| value.as_str()) else {
        return None;
    };

    // Block-item parents often point straight at a block texture.
    if let Some(rest) = parent.strip_prefix("minecraft:block/") {
        if let Some(png) = read_png_any(
            archives,
            &format!("assets/minecraft/textures/block/{rest}.png"),
        ) {
            return Some(png);
        }
        return resolve_model_ref(
            archives,
            "minecraft",
            &format!("minecraft:block/{rest}"),
            depth + 1,
        );
    }
    if let Some(rest) = parent.strip_prefix("block/") {
        if let Some(png) = read_png_any(
            archives,
            &format!("assets/{namespace}/textures/block/{rest}.png"),
        ) {
            return Some(png);
        }
        if let Some(png) = read_png_any(
            archives,
            &format!("assets/minecraft/textures/block/{rest}.png"),
        ) {
            return Some(png);
        }
        return resolve_model_ref(archives, namespace, &format!("block/{rest}"), depth + 1);
    }

    let (parent_ns, parent_path) = split_namespaced(parent, namespace);
    // Skip abstract parents that never carry textures themselves.
    if matches!(
        parent_path,
        "item/generated"
            | "item/handheld"
            | "item/handheld_rod"
            | "item/handheld_mace"
            | "builtin/generated"
            | "builtin/entity"
    ) {
        return None;
    }

    if parent_path.starts_with("item/") || parent_path.starts_with("block/") {
        resolve_model_file(archives, parent_ns, parent_path, depth + 1)
    } else {
        resolve_model_file(
            archives,
            parent_ns,
            &format!("item/{parent_path}"),
            depth + 1,
        )
    }
}

fn find_in_archives(
    archives: &mut [ZipArchive<File>],
    namespace: &str,
    item_path: &str,
) -> Option<Vec<u8>> {
    for candidate in [
        format!("assets/{namespace}/textures/item/{item_path}.png"),
        format!("assets/{namespace}/textures/block/{item_path}.png"),
        format!("assets/{namespace}/textures/{item_path}.png"),
    ] {
        if let Some(png) = read_png_any(archives, &candidate) {
            return Some(png);
        }
    }
    resolve_from_model(archives, namespace, item_path, 0)
}

fn jar_sources(project_dir: &Path, extra_jars: &[PathBuf]) -> Vec<PathBuf> {
    let mut sources = extra_jars.to_vec();

    let mods_dir = project_dir.join("mods");
    if mods_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&mods_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map_or(false, |ext| ext == "jar") {
                    // Prefer enabled jars; skip *.jar.disabled
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if name.ends_with(".disabled") {
                        continue;
                    }
                    sources.push(path);
                }
            }
        }
    }
    sources
}

/// Locates a PNG texture for `item_id`, caches it under `.tuffbox/cache/item-icons/`,
/// and returns the cached file path when found.
pub fn resolve_item_icon_path(
    project_dir: &Path,
    item_id: &str,
    extra_jars: &[PathBuf],
) -> Result<Option<PathBuf>, String> {
    let Some((namespace, item_path)) = parse_item_id(item_id) else {
        return Ok(None);
    };

    let cache_dir = item_icon_cache_dir(project_dir);
    std::fs::create_dir_all(&cache_dir).map_err(|error| error.to_string())?;
    let cache_file = cache_file_for(project_dir, item_id);
    if cache_file.is_file() {
        return Ok(Some(cache_file));
    }

    let mut archives = open_jar_archives(project_dir, extra_jars);
    if let Some(png) = find_in_archives(&mut archives, &namespace, &item_path) {
        std::fs::write(&cache_file, png).map_err(|error| error.to_string())?;
        return Ok(Some(cache_file));
    }

    Ok(None)
}

fn open_jar_archives(project_dir: &Path, extra_jars: &[PathBuf]) -> Vec<ZipArchive<File>> {
    jar_sources(project_dir, extra_jars)
        .into_iter()
        .filter_map(|jar_path| {
            let file = File::open(&jar_path).ok()?;
            ZipArchive::new(file).ok()
        })
        .collect()
}

/// Resolves many item icons in one pass, opening each mod jar only once.
pub fn resolve_item_icons_batch(
    project_dir: &Path,
    item_ids: &[String],
    extra_jars: &[PathBuf],
) -> Result<HashMap<String, Option<PathBuf>>, String> {
    let cache_dir = item_icon_cache_dir(project_dir);
    std::fs::create_dir_all(&cache_dir).map_err(|error| error.to_string())?;

    let mut out = HashMap::with_capacity(item_ids.len());
    let mut pending: Vec<(String, String, String)> = Vec::new();

    for item_id in item_ids {
        // Tags themselves have no single texture — callers should expand first.
        if item_id.starts_with('#') {
            out.insert(item_id.clone(), None);
            continue;
        }
        let Some((namespace, item_path)) = parse_item_id(item_id) else {
            out.insert(item_id.clone(), None);
            continue;
        };
        let cache_file = cache_file_for(project_dir, item_id);
        if cache_file.is_file() {
            out.insert(item_id.clone(), Some(cache_file));
            continue;
        }
        pending.push((item_id.clone(), namespace, item_path));
    }

    if pending.is_empty() {
        return Ok(out);
    }

    let mut archives = open_jar_archives(project_dir, extra_jars);
    for (item_id, namespace, item_path) in pending {
        let found = find_in_archives(&mut archives, &namespace, &item_path).and_then(|png| {
            let cache_file = cache_file_for(project_dir, &item_id);
            std::fs::write(&cache_file, png).ok()?;
            Some(cache_file)
        });
        out.insert(item_id, found);
    }

    Ok(out)
}

/// Reads a cached PNG and returns a `data:image/png;base64,...` URL for the WebView.
pub fn png_path_to_data_url(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    if !is_png(&bytes) {
        return None;
    }
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    Some(format!("data:image/png;base64,{}", STANDARD.encode(bytes)))
}

/// Resolves an item icon and returns an embeddable data URL (no asset protocol needed).
pub fn resolve_item_icon_data_url(
    project_dir: &Path,
    item_id: &str,
    extra_jars: &[PathBuf],
) -> Result<Option<String>, String> {
    let path = resolve_item_icon_path(project_dir, item_id, extra_jars)?;
    Ok(path.and_then(|file| png_path_to_data_url(&file)))
}

/// Batch variant of [`resolve_item_icon_data_url`].
pub fn resolve_item_icons_data_urls(
    project_dir: &Path,
    item_ids: &[String],
    extra_jars: &[PathBuf],
) -> Result<HashMap<String, Option<String>>, String> {
    let paths = resolve_item_icons_batch(project_dir, item_ids, extra_jars)?;
    Ok(paths
        .into_iter()
        .map(|(id, path)| (id, path.and_then(|file| png_path_to_data_url(&file))))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_item_ids() {
        assert_eq!(
            parse_item_id("minecraft:diamond"),
            Some(("minecraft".into(), "diamond".into()))
        );
        assert_eq!(
            parse_item_id("#forge:ingots/copper"),
            Some(("forge".into(), "ingots/copper".into()))
        );
        assert_eq!(parse_item_id("bad"), None);
    }

    #[test]
    fn model_paths_do_not_double_item_prefix() {
        assert_eq!(
            model_zip_path("minecraft", "minecraft:item/generated"),
            "assets/minecraft/models/item/generated.json"
        );
        assert_eq!(
            model_zip_path("croptopia", "item/generated"),
            "assets/croptopia/models/item/generated.json"
        );
        assert_eq!(
            model_zip_path("croptopia", "toast"),
            "assets/croptopia/models/item/toast.json"
        );
    }

    #[test]
    fn texture_refs_include_item_and_block() {
        let paths = texture_zip_path("croptopia", "croptopia:item/toast");
        assert!(paths.contains(&"assets/croptopia/textures/item/toast.png".into()));
        let bare = texture_zip_path("croptopia", "toast");
        assert!(bare.contains(&"assets/croptopia/textures/item/toast.png".into()));
    }
}
