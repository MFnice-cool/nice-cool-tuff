//! Pack Diff Tool: normalize any pack source (manifest, snapshot, backup zip)
//! into a comparable state, then diff two states (mods + editable configs).
//!
//! Stays deterministic and IO-free here: source resolvers live in the desktop
//! crate and feed `pack_state_from_parts`; this module owns the data model
//! and the diff itself.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// One mod row in a normalized pack state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackModRow {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub file_name: Option<String>,
}

/// One editable config file captured from a source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigFileContent {
    pub path: String,
    pub content: String,
    /// False when the file was skipped (binary / too large) upstream.
    pub readable: bool,
}

/// Normalized snapshot of a pack: mods + editable configs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackState {
    pub name: String,
    pub mc_version: String,
    pub loader: String,
    pub mods: BTreeMap<String, PackModRow>,
    pub configs: BTreeMap<String, ConfigFileContent>,
}

/// Result of comparing two pack states.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackDiffReport {
    /// Present only in B (added).
    pub added_mods: Vec<PackModRow>,
    /// Present in both but version or fileName changed.
    pub updated_mods: Vec<ModUpdate>,
    /// Present only in A (removed).
    pub removed_mods: Vec<PackModRow>,
    /// Editable config paths whose content differs (or that exist on one side only).
    pub changed_config_paths: Vec<String>,
    /// Identity block for the UI header.
    pub name_a: String,
    pub name_b: String,
    pub mc_a: String,
    pub mc_b: String,
    pub loader_a: String,
    pub loader_b: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModUpdate {
    pub id: String,
    pub name: String,
    pub from: PackModRow,
    pub to: PackModRow,
}

/// Extensions eligible for inline config diff (mirrors the desktop
/// `is_editable_config_path` semantics; kept in core so tests are portable).
pub fn is_diffable_config(rel_path: &str) -> bool {
    let lower = rel_path.to_ascii_lowercase();
    [
        ".json", ".toml", ".properties", ".cfg", ".conf", ".txt", ".yaml", ".yml", ".zs", ".js",
    ]
    .iter()
    .any(|ext| lower.ends_with(ext))
}

/// Build a [`PackState`] from manifest JSON text + (relative path, content)
/// pairs for config files. Shared by every source kind.
pub fn pack_state_from_parts(
    manifest_text: &str,
    config_files: impl IntoIterator<Item = (String, String)>,
) -> Result<PackState, String> {
    let json: serde_json::Value =
        serde_json::from_str(manifest_text).map_err(|e| format!("manifest parse: {e}"))?;
    let mut mods = BTreeMap::new();
    for m in json
        .get("mods")
        .and_then(|m| m.as_array())
        .into_iter()
        .flatten()
    {
        let id = m
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        if id.is_empty() {
            continue;
        }
        mods.insert(
            id.clone(),
            PackModRow {
                id,
                name: m
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                version: m
                    .get("version")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                file_name: m.get("fileName").and_then(|v| v.as_str()).map(String::from),
            },
        );
    }
    let mut configs = BTreeMap::new();
    for (path, content) in config_files {
        configs.insert(
            path.clone(),
            ConfigFileContent {
                path,
                content,
                readable: true,
            },
        );
    }
    Ok(PackState {
        name: json
            .pointer("/project/name")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .into(),
        mc_version: json
            .pointer("/minecraft/version")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .into(),
        loader: json
            .pointer("/loader/kind")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .into(),
        mods,
        configs,
    })
}

/// Diff two normalized states. Mods are keyed by `id`; configs compare by
/// content equality (added / removed files count as changed).
pub fn diff_pack_states(a: &PackState, b: &PackState) -> PackDiffReport {
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut updated = Vec::new();
    for (id, row_b) in &b.mods {
        match a.mods.get(id) {
            None => added.push(row_b.clone()),
            Some(row_a) => {
                if row_a.version != row_b.version || row_a.file_name != row_b.file_name {
                    updated.push(ModUpdate {
                        id: id.clone(),
                        name: row_b.name.clone(),
                        from: row_a.clone(),
                        to: row_b.clone(),
                    });
                }
            }
        }
    }
    for (id, row_a) in &a.mods {
        if !b.mods.contains_key(id) {
            removed.push(row_a.clone());
        }
    }
    let mut changed_config_paths = Vec::new();
    let mut paths: std::collections::BTreeSet<&String> = a.configs.keys().collect();
    paths.extend(b.configs.keys());
    for path in paths {
        let differs = match (a.configs.get(path), b.configs.get(path)) {
            (Some(x), Some(y)) => x.content != y.content,
            _ => true, // added or removed file
        };
        if differs {
            changed_config_paths.push(path.clone());
        }
    }
    PackDiffReport {
        added_mods: added,
        removed_mods: removed,
        updated_mods: updated,
        changed_config_paths,
        name_a: a.name.clone(),
        name_b: b.name.clone(),
        mc_a: a.mc_version.clone(),
        mc_b: b.mc_version.clone(),
        loader_a: a.loader.clone(),
        loader_b: b.loader.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST_A: &str = r#"{
        "schemaVersion": "0.1.0",
        "project": {"name": "PackA"},
        "minecraft": {"version": "1.20.1"},
        "loader": {"kind": "fabric", "version": "0.15.0"},
        "mods": [
            {"id": "sodium", "name": "Sodium", "version": "0.5.3", "source": {"kind": "modrinth"}, "side": "both"},
            {"id": "lithium", "name": "Lithium", "version": "0.11.2", "source": {"kind": "modrinth"}, "side": "both"}
        ]
    }"#;
    const MANIFEST_B: &str = r#"{
        "schemaVersion": "0.1.0",
        "project": {"name": "PackA"},
        "minecraft": {"version": "1.20.1"},
        "loader": {"kind": "fabric", "version": "0.15.11"},
        "mods": [
            {"id": "sodium", "name": "Sodium", "version": "0.5.8", "source": {"kind": "modrinth"}, "side": "both"},
            {"id": "iris", "name": "Iris", "version": "1.7.0", "source": {"kind": "modrinth"}, "side": "both"}
        ]
    }"#;

    #[test]
    fn manifest_parse_collects_mods() {
        let st = pack_state_from_parts(MANIFEST_A, Vec::<(String, String)>::new()).unwrap();
        assert_eq!(st.name, "PackA");
        assert_eq!(st.mc_version, "1.20.1");
        assert_eq!(st.loader, "fabric");
        assert_eq!(st.mods.len(), 2);
        assert_eq!(st.mods["sodium"].version, "0.5.3");
        assert_eq!(st.mods["lithium"].name, "Lithium");
    }

    #[test]
    fn diff_reports_add_remove_update() {
        let a = pack_state_from_parts(MANIFEST_A, Vec::<(String, String)>::new()).unwrap();
        let b = pack_state_from_parts(MANIFEST_B, Vec::<(String, String)>::new()).unwrap();
        let d = diff_pack_states(&a, &b);
        assert_eq!(
            d.removed_mods.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            ["lithium"]
        );
        assert_eq!(
            d.added_mods.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            ["iris"]
        );
        assert_eq!(d.updated_mods.len(), 1);
        assert_eq!(d.updated_mods[0].id, "sodium");
        assert_eq!(d.updated_mods[0].from.version, "0.5.3");
        assert_eq!(d.updated_mods[0].to.version, "0.5.8");
    }

    #[test]
    fn config_diff_detects_added_removed_changed() {
        let a = pack_state_from_parts(
            MANIFEST_A,
            [("/config/a.json".into(), "{\"x\":1}".into())],
        )
        .unwrap();
        let b = pack_state_from_parts(
            MANIFEST_B,
            [
                ("/config/a.json".into(), "{\"x\":2}".into()),
                ("/config/new.toml".into(), "y = 1".into()),
            ],
        )
        .unwrap();
        let d = diff_pack_states(&a, &b);
        assert!(d.changed_config_paths.contains(&"/config/a.json".to_string()));
        assert!(d.changed_config_paths.contains(&"/config/new.toml".to_string()));
    }

    #[test]
    fn diffable_config_extensions() {
        assert!(is_diffable_config("config/sodium-options.json"));
        assert!(is_diffable_config("config/mixins.toml"));
        assert!(!is_diffable_config("mods/sodium.jar"));
        assert!(!is_diffable_config("icon.png"));
    }
}
