//! User-built mod presets for the Optimize flow on Content tab.
//!
//! Presets are local collections of Modrinth / CurseForge project references.
//! They let a player assemble a custom mod list once and install the whole set
//! into any instance (MC version is resolved per-instance at install time).
//! Storage follows the same pattern as `presence.rs`: one JSON file in the
//! app config dir.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PresetModEntry {
    /// "modrinth" | "curseforge"
    pub provider: String,
    /// Modrinth project id/slug or CurseForge numeric project id (as string).
    pub project_id: String,
    #[serde(default)]
    pub slug: String,
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModPreset {
    pub id: String,
    pub name: String,
    #[serde(default = "now_millis_string")]
    pub created_at: String,
    #[serde(default)]
    pub mods: Vec<PresetModEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModPresetsStore {
    #[serde(default)]
    pub presets: Vec<ModPreset>,
}

fn now_millis_string() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis().to_string())
        .unwrap_or_default()
}

fn presets_path() -> PathBuf {
    dirs::config_dir()
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("TuffBox")
        .join("mod_presets.json")
}

pub fn load_mod_presets() -> ModPresetsStore {
    fs::read_to_string(presets_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_mod_presets(store: &ModPresetsStore) -> Result<(), String> {
    // Basic hygiene: unique non-empty ids, non-empty names, deduped entries.
    let mut presets = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();
    for mut preset in store.presets.clone() {
        preset.id = preset.id.trim().to_string();
        preset.name = preset.name.trim().to_string();
        if preset.id.is_empty() || preset.name.is_empty() || !seen_ids.insert(preset.id.clone()) {
            continue;
        }
        let mut seen = std::collections::HashSet::new();
        preset.mods.retain(|m| {
            m.project_id.trim() != String::new()
                && seen.insert((m.provider.clone(), m.project_id.clone()))
        });
        presets.push(preset);
    }
    let clean = ModPresetsStore { presets };
    let path = presets_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(
        &path,
        serde_json::to_string_pretty(&clean).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_roundtrips_defaults() {
        let store = ModPresetsStore::default();
        let raw = serde_json::to_string(&store).unwrap();
        let back: ModPresetsStore = serde_json::from_str(&raw).unwrap();
        assert!(back.presets.is_empty());
    }

    #[test]
    fn entry_accepts_missing_slug_and_name() {
        let entry: PresetModEntry =
            serde_json::from_str(r#"{"provider":"modrinth","projectId":"AABB"}"#).unwrap();
        assert_eq!(entry.slug, "");
        assert_eq!(entry.name, "");
    }
}
