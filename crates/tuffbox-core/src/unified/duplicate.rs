//! Duplicate resource detector for Unified recipes/items.
//!
//! When multiple mods add the same material (e.g., both Mekanism and
//! Thermal add "tin_ingot"), this module detects the duplicates,
//! scores the variants, and generates KubeJS/CraftTweaker scripts
//! to unify them into a single canonical version.

use crate::knowledge::heuristics::{classify_item, DuplicateItemGroup};

/// A group of duplicate items across mods with a recommendation for
/// which variant to keep as canonical.
#[derive(Debug, Clone)]
pub struct DedupResolution {
    pub material: String,
    pub item_type: String,
    pub keep: String,        // item id to keep
    pub remove: Vec<String>, // item ids to replace/remove
    pub script_type: ScriptTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptTarget {
    KubeJS6,
    KubeJS7,
    CraftTweaker,
    DataPack,
}

impl DedupResolution {
    /// Generates a KubeJS 6 script for server-side item replacement.
    pub fn to_kubejs6(&self) -> String {
        let mut s = format!(
            "// Unify {} for material {}\n",
            self.item_type, self.material
        );
        s.push_str("ServerEvents.recipes(event => {\n");
        for item in &self.remove {
            s.push_str(&format!("  // Replace {} with {}\n", item, self.keep));
            s.push_str(&format!(
                "  event.replaceInput({{}}, '{}', '{}')\n",
                item, self.keep
            ));
            s.push_str(&format!(
                "  event.replaceOutput({{}}, '{}', '{}')\n",
                item, self.keep
            ));
        }
        s.push_str("})\n");
        s
    }

    /// Generates a CraftTweaker script.
    pub fn to_crafttweaker(&self) -> String {
        let mut s = format!(
            "// Unify {} for material {}\n\n",
            self.item_type, self.material
        );
        for item in &self.remove {
            s.push_str(&format!("// Replace {} with {}\n", item, self.keep));
            s.push_str(&format!("craftingTable.remove(<item:{}>);\n", item));
        }
        s
    }
}

/// Analyzes duplicate item groups and recommends resolutions.
pub fn resolve_duplicates(groups: &[DuplicateItemGroup]) -> Vec<DedupResolution> {
    let mut resolutions = Vec::new();
    for group in groups {
        // Group by item type
        let mut by_type: std::collections::HashMap<String, Vec<&(String, String)>> =
            std::collections::HashMap::new();
        for entry in &group.entries {
            if let Some((_, item_type)) = classify_item(&entry.1) {
                by_type.entry(item_type).or_default().push(entry);
            }
        }
        for (itype, items) in by_type {
            if items.len() < 2 {
                continue;
            }
            // Pick the first one as canonical (simplistic — knowledge base could override)
            let (_keep_mod, keep_item) = &items[0];
            let remove: Vec<String> = items[1..].iter().map(|(_, item)| item.clone()).collect();
            resolutions.push(DedupResolution {
                material: group.material.clone(),
                item_type: itype,
                keep: keep_item.clone(),
                remove,
                script_type: ScriptTarget::KubeJS6,
            });
        }
    }
    resolutions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_duplicates() {
        let groups = vec![DuplicateItemGroup {
            material: "tin".into(),
            entries: vec![
                ("mekanism".into(), "tin_ingot".into()),
                ("thermal".into(), "tin_ingot".into()),
            ],
        }];
        let res = resolve_duplicates(&groups);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].keep, "tin_ingot");
        assert_eq!(res[0].remove.len(), 1);
    }
}
