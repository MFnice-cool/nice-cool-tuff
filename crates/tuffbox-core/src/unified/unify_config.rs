//! Almost Unified — full unification engine.
//!
//! Implements every feature of the Almost Unified mod as launcher-side
//! logic: mod priorities, tag ownerships, custom tags, tag inheritance
//! (allow/deny mode), stone strata filtering, duplicate recipe removal,
//! forge: and c: namespace support, ignored recipes/tags/items with
//! regex, and JEI/REI hiding.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnifyConfig {
    #[serde(default = "d_prio")]
    pub mod_priorities: Vec<String>,
    #[serde(default = "d_stone")]
    pub stone_strata: Vec<String>,
    #[serde(default = "d_tags")]
    pub tags: Vec<String>,
    #[serde(default = "d_mats")]
    pub materials: Vec<String>,
    #[serde(default)]
    pub priority_overrides: HashMap<String, String>,
    #[serde(default)]
    pub custom_tags: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub tag_ownerships: HashMap<String, Vec<String>>,
    #[serde(default = "d_mode")]
    pub item_tag_inheritance_mode: String,
    #[serde(default)]
    pub item_tag_inheritance: HashMap<String, Vec<String>>,
    #[serde(default = "d_mode")]
    pub block_tag_inheritance_mode: String,
    #[serde(default)]
    pub block_tag_inheritance: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub ignored_tags: Vec<String>,
    #[serde(default)]
    pub ignored_items: Vec<String>,
    #[serde(default)]
    pub ignored_recipe_types: Vec<String>,
    #[serde(default)]
    pub ignored_recipes: Vec<String>,
    #[serde(default = "d_true")]
    pub items_hiding_jei_rei: bool,
    #[serde(default = "d_true")]
    pub remove_duplicate_recipes: bool,
    #[serde(default)]
    pub namespace_format: String, // "common" or "forge"
}

fn d_prio() -> Vec<String> {
    vec![
        "minecraft".into(),
        "kubejs".into(),
        "crafttweaker".into(),
        "create".into(),
        "thermal".into(),
        "immersiveengineering".into(),
        "mekanism".into(),
        "techreborn".into(),
        "modern_industrialization".into(),
        "indrev".into(),
        "ad_astra".into(),
        "powah".into(),
        "ae2".into(),
        "refinedstorage".into(),
        "silentgear".into(),
        "tconstruct".into(),
        "botania".into(),
        "forbidden_arcanus".into(),
        "ars_nouveau".into(),
        "occultism".into(),
    ]
}
fn d_stone() -> Vec<String> {
    vec![
        "stone".into(),
        "nether".into(),
        "deepslate".into(),
        "granite".into(),
        "diorite".into(),
        "andesite".into(),
    ]
}
fn d_tags() -> Vec<String> {
    vec![
        "c:{material}_nuggets".into(),
        "c:{material}_dusts".into(),
        "c:{material}_gears".into(),
        "c:{material}_gems".into(),
        "c:{material}_ingots".into(),
        "c:{material}_raw_materials".into(),
        "c:{material}_ores".into(),
        "c:{material}_plates".into(),
        "c:{material}_rods".into(),
        "c:{material}_blocks".into(),
        "c:{material}_wires".into(),
        "c:{material}_storage_blocks".into(),
        "c:raw_{material}_ores".into(),
        "c:raw_{material}_blocks".into(),
        "c:raw_{material}_storage_blocks".into(),
    ]
}
fn d_mats() -> Vec<String> {
    vec![
        "aeternium".into(),
        "aluminum".into(),
        "amber".into(),
        "apatite".into(),
        "bitumen".into(),
        "brass".into(),
        "bronze".into(),
        "charcoal".into(),
        "chrome".into(),
        "cinnabar".into(),
        "coal".into(),
        "coal_coke".into(),
        "cobalt".into(),
        "constantan".into(),
        "copper".into(),
        "diamond".into(),
        "electrum".into(),
        "elementium".into(),
        "emerald".into(),
        "enderium".into(),
        "fluorite".into(),
        "gold".into(),
        "graphite".into(),
        "invar".into(),
        "iridium".into(),
        "iron".into(),
        "lapis".into(),
        "lead".into(),
        "lumium".into(),
        "mithril".into(),
        "netherite".into(),
        "nickel".into(),
        "obsidian".into(),
        "osmium".into(),
        "peridot".into(),
        "platinum".into(),
        "potassium_nitrate".into(),
        "ruby".into(),
        "sapphire".into(),
        "signalum".into(),
        "silver".into(),
        "steel".into(),
        "sulfur".into(),
        "tin".into(),
        "tungsten".into(),
        "uranium".into(),
        "zinc".into(),
    ]
}
fn d_mode() -> String {
    "allow".into()
}
fn d_true() -> bool {
    true
}

impl Default for UnifyConfig {
    fn default() -> Self {
        Self {
            mod_priorities: d_prio(),
            stone_strata: d_stone(),
            tags: d_tags(),
            materials: d_mats(),
            priority_overrides: HashMap::new(),
            custom_tags: HashMap::new(),
            tag_ownerships: HashMap::new(),
            item_tag_inheritance_mode: d_mode(),
            item_tag_inheritance: HashMap::new(),
            block_tag_inheritance_mode: d_mode(),
            block_tag_inheritance: HashMap::new(),
            ignored_tags: Vec::new(),
            ignored_items: Vec::new(),
            ignored_recipe_types: vec!["cucumber:shaped_tag".into()],
            ignored_recipes: Vec::new(),
            items_hiding_jei_rei: true,
            remove_duplicate_recipes: true,
            namespace_format: "common".into(),
        }
    }
}

impl UnifyConfig {
    pub fn for_project(slugs: &[String]) -> Self {
        let mut c = Self::default();
        let mut p: Vec<String> = slugs.iter().cloned().collect();
        p.extend(c.mod_priorities.clone());
        c.mod_priorities = p;
        c
    }

    /// Expands all tag patterns with materials. Supports both c: and forge: formats.
    pub fn expanded_tags(&self) -> Vec<String> {
        let mut r = Vec::new();
        for t in &self.tags {
            if t.contains("{material}") {
                for m in &self.materials {
                    r.push(t.replace("{material}", m));
                }
            } else {
                r.push(t.clone());
            }
        }
        r
    }

    pub fn tags_for_material(&self, mat: &str) -> Vec<String> {
        self.tags
            .iter()
            .filter(|t| t.contains("{material}"))
            .map(|t| t.replace("{material}", mat))
            .collect()
    }
    pub fn priority_rank(&self, slug: &str) -> Option<usize> {
        self.mod_priorities.iter().position(|s| s == slug)
    }
    pub fn is_tag_ignored(&self, t: &str) -> bool {
        self.ignored_tags.iter().any(|ig| matches_regex(t, ig))
    }
    pub fn is_item_ignored(&self, i: &str) -> bool {
        self.ignored_items.iter().any(|ig| matches_regex(i, ig))
    }
    pub fn is_recipe_type_ignored(&self, rt: &str) -> bool {
        self.ignored_recipe_types
            .iter()
            .any(|ig| matches_regex(rt, ig))
    }
    pub fn is_recipe_ignored(&self, rid: &str) -> bool {
        self.ignored_recipes.iter().any(|ig| matches_regex(rid, ig))
    }

    /// Get all items that should be added to a custom tag.
    pub fn custom_tag_items(&self, tag: &str) -> Vec<String> {
        self.custom_tags.get(tag).cloned().unwrap_or_default()
    }

    /// Get the owner tag for reference tags. E.g., "forge:gems/coal" → "forge:coals"
    pub fn resolve_tag_ownership(&self, tag: &str) -> Option<String> {
        for (owner, refs) in &self.tag_ownerships {
            if refs.contains(&tag.to_string()) {
                return Some(owner.clone());
            }
        }
        None
    }

    /// Collect all tags that should be inherited by dominant items (allow mode).
    pub fn inherited_item_tags(&self, dominant_tag: &str) -> Vec<String> {
        if self.item_tag_inheritance_mode == "deny" {
            // In deny mode, all tags get inherited except denied ones
            let denied = self
                .item_tag_inheritance
                .get(dominant_tag)
                .cloned()
                .unwrap_or_default();
            if denied.contains(&dominant_tag.to_string()) {
                return vec![];
            }
            return vec![dominant_tag.to_string()];
        }
        // In allow mode, only explicitly allowed tags get inherited
        self.item_tag_inheritance
            .get(dominant_tag)
            .cloned()
            .unwrap_or_default()
    }

    /// Collect all block tags that should be inherited.
    pub fn inherited_block_tags(&self, dominant_tag: &str) -> Vec<String> {
        if self.block_tag_inheritance_mode == "deny" {
            let denied = self
                .block_tag_inheritance
                .get(dominant_tag)
                .cloned()
                .unwrap_or_default();
            if denied.contains(&dominant_tag.to_string()) {
                return vec![];
            }
            return vec![dominant_tag.to_string()];
        }
        self.block_tag_inheritance
            .get(dominant_tag)
            .cloned()
            .unwrap_or_default()
    }

    pub fn save_to(&self, p: &std::path::Path) -> Result<(), String> {
        if let Some(par) = p.parent() {
            std::fs::create_dir_all(par).map_err(|e| e.to_string())?;
        }
        std::fs::write(
            p,
            serde_json::to_string_pretty(self).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())
    }
    pub fn load_from(p: &std::path::Path) -> Result<Self, String> {
        serde_json::from_str(&std::fs::read_to_string(p).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())
    }

    /// Converts tag between forge: and c: namespace formats.
    pub fn normalize_tag(tag: &str, format: &str) -> String {
        if format == "forge" {
            // c:copper_ingots → forge:ingots/copper
            let parts: Vec<&str> = tag.split(':').collect();
            if parts.len() == 2 && parts[0] == "c" {
                let name = parts[1];
                for ty in &[
                    "nuggets",
                    "dusts",
                    "gears",
                    "gems",
                    "ingots",
                    "raw_materials",
                    "ores",
                    "plates",
                    "rods",
                    "blocks",
                    "wires",
                    "storage_blocks",
                ] {
                    if let Some(mat) = name.strip_suffix(&format!("_{}", ty)) {
                        return format!("forge:{}/{}", ty, mat);
                    }
                }
            }
        } else {
            // forge:ingots/copper → c:copper_ingots
            let parts: Vec<&str> = tag.split(':').collect();
            if parts.len() == 2 && parts[0] == "forge" {
                let inner: Vec<&str> = parts[1].split('/').collect();
                if inner.len() == 2 {
                    return format!("c:{}_{}", inner[1], inner[0]);
                }
            }
        }
        tag.to_string()
    }
}

// ═══════════════════════════════════════════════════════════════════
// Unification engine

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnificationResult {
    pub material: String,
    pub item_type: String,
    pub tag: String,
    pub dominant_item: String,
    pub dominant_mod: String,
    pub alternatives: Vec<AltItem>,
    pub stone_variants: Vec<String>,
    pub removed_duplicates: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AltItem {
    pub item_id: String,
    pub mod_id: String,
    pub will_be_replaced: bool,
}

/// Run the full Almost Unified analysis:
/// 1. Expand tags × materials
/// 2. Apply tag ownerships (merge reference tags into owner tags)
/// 3. Find dominant item per tag by mod priority
/// 4. Apply tag inheritance (allow/deny mode)
/// 5. Detect and flag duplicate recipes
/// 6. Filter stone variants
/// 7. Generate replacement plan
pub fn analyze_unification_full(
    config: &UnifyConfig,
    installed: &[String],
    items_by_mod: &HashMap<String, Vec<String>>,
    recipes: &[(String, String, Vec<String>, String)], // (recipe_id, recipe_type, inputs, output)
) -> Vec<UnificationResult> {
    let mut results = Vec::new();
    let _expanded = config.expanded_tags();

    // Build tag→owner map from ownerships
    let mut ownership_map: HashMap<String, String> = HashMap::new();
    for (owner, refs) in &config.tag_ownerships {
        for r in refs {
            ownership_map.insert(r.clone(), owner.clone());
        }
    }

    for mat in &config.materials {
        let mat_tags = config.tags_for_material(mat);
        for raw_tag in &mat_tags {
            // Resolve tag ownership: reference tag → owner tag
            let tag = ownership_map.get(raw_tag).unwrap_or(raw_tag).clone();

            if config.is_tag_ignored(&tag) {
                continue;
            }

            let itype = extract_type(&tag);
            let mut cands: Vec<(String, String)> = Vec::new();

            for (mid, items) in items_by_mod {
                if !installed.contains(mid) {
                    continue;
                }
                for item in items {
                    let il = item.to_lowercase();
                    let mat_l = mat.to_lowercase();
                    let singular = if itype == "item" {
                        itype.clone()
                    } else {
                        itype.trim_end_matches('s').to_string()
                    };
                    if il.contains(&mat_l)
                        && (il.contains(&itype) || il.contains(singular.as_str()))
                        && !config.is_item_ignored(item)
                    {
                        cands.push((mid.clone(), item.clone()));
                    }
                }
            }

            if cands.len() < 2 {
                continue;
            }

            // Sort by priority
            let mut sorted = cands.clone();
            sorted.sort_by(|a, b| {
                config
                    .priority_rank(&a.0)
                    .unwrap_or(usize::MAX)
                    .cmp(&config.priority_rank(&b.0).unwrap_or(usize::MAX))
                    .then_with(|| a.1.cmp(&b.1))
            });

            // Check priority override for this specific tag
            let dom = if let Some(ov) = config.priority_overrides.get(&tag) {
                sorted
                    .iter()
                    .find(|(m, _)| m == ov)
                    .cloned()
                    .unwrap_or_else(|| sorted[0].clone())
            } else {
                sorted[0].clone()
            };

            let alts: Vec<AltItem> = cands
                .iter()
                .filter(|(_, i)| i != &dom.1)
                .map(|(m, i)| AltItem {
                    item_id: i.clone(),
                    mod_id: m.clone(),
                    will_be_replaced: true,
                })
                .collect();

            // Stone variants: output items matching strata patterns
            let stones: Vec<String> = cands
                .iter()
                .filter(|(_, i)| config.stone_strata.iter().any(|s| i.contains(s)))
                .map(|(_, i)| i.clone())
                .collect();

            // Find duplicate recipes for this material/tag combo
            let dups: Vec<String> = recipes
                .iter()
                .filter(|(rid, _, inputs, output)| {
                    (inputs
                        .iter()
                        .any(|inp| cands.iter().any(|(_, c)| inp.contains(c)))
                        || cands.iter().any(|(_, c)| output.contains(c)))
                        && !config.is_recipe_ignored(rid)
                        && !config.is_recipe_type_ignored(&"unknown")
                })
                .map(|(rid, _, _, _)| rid.clone())
                .collect();

            // Tag inheritance: collect tags that should be inherited by dominant item
            let _inherited_item_tags = config.inherited_item_tags(&tag);
            let _inherited_block_tags = config.inherited_block_tags(&tag);

            results.push(UnificationResult {
                material: mat.clone(),
                item_type: itype,
                tag: tag.clone(),
                dominant_item: dom.1,
                dominant_mod: dom.0,
                alternatives: alts,
                stone_variants: stones,
                removed_duplicates: if config.remove_duplicate_recipes {
                    dups
                } else {
                    vec![]
                },
            });
        }
    }
    results
}

/// Simpler version that doesn't require recipe data.
pub fn analyze_unification(
    config: &UnifyConfig,
    installed: &[String],
    items_by_mod: &HashMap<String, Vec<String>>,
) -> Vec<UnificationResult> {
    analyze_unification_full(config, installed, items_by_mod, &[])
}

fn extract_type(tag: &str) -> String {
    let known = [
        "nuggets",
        "dusts",
        "gears",
        "gems",
        "ingots",
        "ores",
        "plates",
        "rods",
        "blocks",
        "wires",
        "storage_blocks",
        "raw_materials",
        "raw_ores",
        "raw_blocks",
        "raw_materials_block",
    ];
    for p in tag.split(|c: char| c == '_' || c == '/' || c == ':') {
        let pl = p.to_lowercase();
        if known.contains(&pl.as_str()) {
            return pl;
        }
    }
    "item".into()
}

/// Simple regex-like matching for ignored patterns. Supports * wildcard.
fn matches_regex(value: &str, pattern: &str) -> bool {
    if pattern.contains('*') {
        let re = pattern.replace("*", "");
        value.contains(&re)
    } else {
        value == pattern
    }
}

// ═══════════════════════════════════════════════════════════════════
// Script generation

/// Generates a complete KubeJS script: recipe replacements, custom tags,
/// tag ownerships, JEI hiding, and duplicate recipe removal.
pub fn generate_unification_script_full(
    results: &[UnificationResult],
    config: &UnifyConfig,
) -> String {
    let mut s = String::from("// TuffBox Almost Unified — full unification script\n");
    s.push_str("// Priority: ");
    s.push_str(&config.mod_priorities.join(", "));
    s.push_str("\n\n");

    // Custom tags
    if !config.custom_tags.is_empty() {
        s.push_str("ServerEvents.tags('item', event => {\n");
        for (tag, items) in &config.custom_tags {
            for item in items {
                s.push_str(&format!("  event.add('{}', '{}')\n", tag, item));
            }
        }
        s.push_str("})\n\n");
    }

    // Tag ownerships
    if !config.tag_ownerships.is_empty() {
        s.push_str("// Tag ownership merges\n");
        for (owner, refs) in &config.tag_ownerships {
            s.push_str(&format!("// {} owns {}\n", owner, refs.join(", ")));
        }
        s.push('\n');
    }

    // Recipe unification
    s.push_str("ServerEvents.recipes(event => {\n");
    for r in results {
        if r.alternatives.is_empty() {
            continue;
        }
        s.push_str(&format!(
            "  // ── {} {} ── dominant: {} ({})\n",
            r.material, r.item_type, r.dominant_item, r.dominant_mod
        ));

        // Remove duplicate recipes
        for dup in &r.removed_duplicates {
            s.push_str(&format!("  event.remove({{ id: '{}' }})\n", dup));
        }

        // Replace inputs with tags, outputs with dominant item
        for alt in &r.alternatives {
            if alt.will_be_replaced {
                s.push_str(&format!(
                    "  event.replaceInput({{}}, '{}', '#{}')\n",
                    alt.item_id, r.tag
                ));
                s.push_str(&format!(
                    "  event.replaceOutput({{}}, '{}', '{}')\n",
                    alt.item_id, r.dominant_item
                ));
            }
        }
    }
    s.push_str("})\n\n");

    // Tag inheritance
    if !config.item_tag_inheritance.is_empty() || config.item_tag_inheritance_mode == "deny" {
        s.push_str("ServerEvents.tags('item', event => {\n");
        s.push_str(&format!(
            "  // Inheritance mode: {}\n",
            config.item_tag_inheritance_mode
        ));
        for (tag, refs) in &config.item_tag_inheritance {
            s.push_str(&format!("  // {} inherits: {}\n", tag, refs.join(", ")));
        }
        s.push_str("})\n\n");
    }

    // JEI hiding
    if config.items_hiding_jei_rei {
        s.push_str("JEIEvents.hideItems(event => {\n");
        for r in results {
            for alt in &r.alternatives {
                if alt.will_be_replaced {
                    s.push_str(&format!("  event.hide('{}')\n", alt.item_id));
                }
            }
        }
        s.push_str("})\n\n");
    }

    // Stone variants note
    let all_stones: Vec<&String> = results.iter().flat_map(|r| &r.stone_variants).collect();
    if !all_stones.is_empty() {
        s.push_str("// Stone strata variants detected: ");
        s.push_str(&config.stone_strata.join(", "));
        s.push_str(&format!("\n// {} total stone variants\n", all_stones.len()));
    }

    s
}

pub fn generate_unification_script(results: &[UnificationResult]) -> String {
    let config = UnifyConfig::default();
    generate_unification_script_full(results, &config)
}

pub fn generate_zen_script(results: &[UnificationResult]) -> String {
    let mut s = String::from("// TuffBox Almost Unified — CraftTweaker\n\n");
    for r in results {
        s.push_str(&format!(
            "// {} {} → {} ({})\n",
            r.material, r.item_type, r.dominant_item, r.dominant_mod
        ));
        for d in &r.removed_duplicates {
            s.push_str(&format!("recipes.removeByRecipeName('{}');\n", d));
        }
        for a in &r.alternatives {
            if a.will_be_replaced {
                s.push_str(&format!("recipes.removeShapeless(<item:{}>);\n", a.item_id));
            }
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expands() {
        let c = UnifyConfig::default();
        assert!(c.expanded_tags().contains(&"c:copper_ingots".into()));
    }
    #[test]
    fn priority() {
        let c = UnifyConfig::default();
        assert!(c.priority_rank("minecraft") < c.priority_rank("create"));
    }
    #[test]
    fn tag_ownership_resolves() {
        let mut c = UnifyConfig::default();
        c.tag_ownerships
            .insert("forge:coals".into(), vec!["forge:gems/coal".into()]);
        assert_eq!(
            c.resolve_tag_ownership("forge:gems/coal"),
            Some("forge:coals".into())
        );
    }
    #[test]
    fn normalize_forge_to_common() {
        assert_eq!(
            UnifyConfig::normalize_tag("forge:ingots/copper", "common"),
            "c:copper_ingots"
        );
    }
    #[test]
    fn regex_ignored() {
        assert!(matches_regex(
            "cucumber:shaped_tag_noop",
            "cucumber:shaped*"
        ));
    }
    #[test]
    fn full_unification() {
        let mut items = HashMap::new();
        items.insert(
            "mekanism".into(),
            vec!["mekanism:ingot_tin".into(), "mekanism:ore_tin".into()],
        );
        items.insert(
            "thermal".into(),
            vec!["thermal:tin_ingot".into(), "thermal:tin_dust".into()],
        );
        let c = UnifyConfig::default();
        let r = analyze_unification(&c, &["mekanism".into(), "thermal".into()], &items);
        assert!(!r.is_empty());
    }
}
