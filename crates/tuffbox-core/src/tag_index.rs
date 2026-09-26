//! Offline item-tag index built from mod jars, datapacks, and the vanilla client jar.
//!
//! Used by the recipe browser to expand `#c:apples`-style ingredients into concrete
//! item ids so the UI can cycle their icons.

use crate::adapters::{FabricAdapter, ForgeAdapter, LoaderAdapter, NeoForgeAdapter};
use crate::manifest::LoaderKind;
use crate::unified::tag::UnifiedTag;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use zip::ZipArchive;

/// Maps `namespace:path` → direct tag entries (`minecraft:apple` or `#c:fruits`).
#[derive(Debug, Default, Clone)]
pub struct TagIndex {
    entries: HashMap<String, Vec<String>>,
}

impl TagIndex {
    pub fn build(project_dir: &Path, loader: LoaderKind, extra_jars: &[PathBuf]) -> Self {
        let adapter: &dyn LoaderAdapter = match loader {
            LoaderKind::Fabric | LoaderKind::Quilt => &FabricAdapter,
            LoaderKind::Forge => &ForgeAdapter,
            LoaderKind::Neoforge => &NeoForgeAdapter,
            _ => &FabricAdapter,
        };

        let mut index = TagIndex::default();

        let mods_dir = project_dir.join("mods");
        if mods_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&mods_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().map_or(false, |ext| ext == "jar") {
                        index.ingest_jar(&path, adapter);
                    }
                }
            }
        }

        for jar in extra_jars {
            index.ingest_jar(jar, adapter);
        }

        for root in datapack_roots(project_dir) {
            index.ingest_datapack_tree(&root, adapter);
        }

        let kubejs = project_dir.join("kubejs").join("data");
        if kubejs.is_dir() {
            index.ingest_datapack_tree(&kubejs, adapter);
        }

        index
    }

    fn ingest_jar(&mut self, jar_path: &Path, adapter: &dyn LoaderAdapter) {
        let Ok(file) = File::open(jar_path) else {
            return;
        };
        let Ok(mut archive) = ZipArchive::new(file) else {
            return;
        };
        let paths = adapter.item_tag_paths(&archive);
        for path in paths {
            let Ok(mut entry) = archive.by_name(&path) else {
                continue;
            };
            let mut content = String::new();
            if entry.read_to_string(&mut content).is_err() || content.len() > 256 * 1024 {
                continue;
            }
            let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else {
                continue;
            };
            if let Ok(tag) = adapter.parse_tag(&json, &path) {
                self.insert_tag(&tag);
            }
        }
    }

    fn ingest_datapack_tree(&mut self, root: &Path, adapter: &dyn LoaderAdapter) {
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                let rel = path.strip_prefix(root).unwrap_or(&path);
                let rel_str = rel.to_string_lossy().replace('\\', "/");
                let virtual_path = if let Some(idx) = rel_str.find("data/") {
                    rel_str[idx..].to_string()
                } else {
                    continue;
                };
                if !is_item_tag_rel_path(&virtual_path) {
                    continue;
                }
                let Ok(content) = std::fs::read_to_string(&path) else {
                    continue;
                };
                let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else {
                    continue;
                };
                if let Ok(tag) = adapter.parse_tag(&json, &virtual_path) {
                    self.insert_tag(&tag);
                }
            }
        }
    }

    fn insert_tag(&mut self, tag: &UnifiedTag) {
        let key = format!("{}:{}", tag.id.namespace, tag.id.path);
        let values: Vec<String> = tag.entries.iter().map(|e| e.id.clone()).collect();
        if tag.replace || !self.entries.contains_key(&key) {
            self.entries.insert(key, values);
        } else if let Some(existing) = self.entries.get_mut(&key) {
            for v in values {
                if !existing.contains(&v) {
                    existing.push(v);
                }
            }
        }
    }

    /// Direct (non-expanded) entries for a tag, as stored in JSON.
    pub fn direct_entries(&self, tag_id: &str) -> Vec<String> {
        let key = normalize_tag_key(tag_id);
        self.entries.get(&key).cloned().unwrap_or_default()
    }

    /// All known tag keys as `#namespace:path`, sorted.
    pub fn list_tag_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.entries.keys().map(|k| format!("#{k}")).collect();
        ids.sort();
        ids
    }

    /// Expand a tag id (`#c:apples` or `c:apples`) into concrete item ids.
    pub fn expand_items(&self, tag_id: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        self.expand_into(tag_id, &mut out, &mut seen, 0);
        out
    }

    fn expand_into(
        &self,
        tag_id: &str,
        out: &mut Vec<String>,
        seen: &mut HashSet<String>,
        depth: u8,
    ) {
        if depth > 12 {
            return;
        }
        let key = normalize_tag_key(tag_id);
        if key.is_empty() || !seen.insert(key.clone()) {
            return;
        }
        let Some(entries) = self.entries.get(&key) else {
            return;
        };
        for entry in entries {
            if entry.starts_with('#') {
                self.expand_into(entry, out, seen, depth + 1);
            } else if entry.contains(':') && !out.contains(entry) {
                out.push(entry.clone());
            }
        }
    }
}

fn normalize_tag_key(tag_id: &str) -> String {
    tag_id.trim().trim_start_matches('#').to_string()
}

fn is_item_tag_rel_path(rel: &str) -> bool {
    let parts: Vec<&str> = rel.split('/').collect();
    parts.len() >= 5
        && parts[0] == "data"
        && parts[2] == "tags"
        && (parts[3] == "items" || parts[3] == "item")
        && rel.ends_with(".json")
}

fn datapack_roots(project_dir: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let top = project_dir.join("datapacks");
    if top.is_dir() {
        roots.push(top);
    }
    let saves = project_dir.join("saves");
    if saves.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&saves) {
            for entry in entries.flatten() {
                let dp = entry.path().join("datapacks");
                if dp.is_dir() {
                    roots.push(dp);
                }
            }
        }
    }
    roots
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_nested_tags() {
        let mut index = TagIndex::default();
        index.entries.insert(
            "c:apples".into(),
            vec!["minecraft:apple".into(), "#c:golden_apples".into()],
        );
        index.entries.insert(
            "c:golden_apples".into(),
            vec!["minecraft:golden_apple".into()],
        );
        let items = index.expand_items("#c:apples");
        assert_eq!(
            items,
            vec![
                "minecraft:apple".to_string(),
                "minecraft:golden_apple".to_string()
            ]
        );
    }
}
