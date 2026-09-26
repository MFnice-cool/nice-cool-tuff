//! Automatic heuristics for config scanning, ore generation detection,
//! item classification, and tag normalization.
//!
//! This works without any pre-existing metadata — it uses pattern
//! matching to figure out what config keys control ore generation,
//! which items belong to which mods, and how to normalize tags between
//! forge: and c: namespaces.

use std::collections::HashMap;

/// Result of scanning a config file for ore-gen related keys.
#[derive(Debug, Clone)]
pub struct HeuristicOreGen {
    pub resource_name: String,
    pub config_file: String,
    pub enabled_key: String,
    pub enabled_value: String,
    pub vein_size: Option<(String, String)>,
    pub min_height: Option<(String, String)>,
    pub max_height: Option<(String, String)>,
    pub spawns_per_chunk: Option<(String, String)>,
    pub confidence: HeuristicConfidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeuristicConfidence {
    /// Exact match from knowledge base
    High,
    /// Pattern match with strong signal
    Medium,
    /// Weak pattern match — user should verify
    Low,
}

/// Strong compound key patterns (safe as substrings — specific enough to avoid noise).
const STRONG_KEY_PATTERNS: &[&str] = &[
    "shouldgenerate",
    "oregeneration",
    "generateore",
    "enableoregen",
    "enableworldgen",
    "worldgen",
    "spawnore",
    "orespawn",
    "generateinworld",
    "shouldspawn",
    "cangenerate",
    "allowgeneration",
    "disableore",
    "disablezincore",
];

/// Config path markers that are almost never ore-generation settings.
const SKIP_PATH_MARKERS: &[&str] = &[
    "almostunified",
    "unify.json",
    "jei",
    "emi",
    "rei",
    "jade",
    "theoneprobe",
    "ftbquests",
    "ftbteams",
    "ftbchunks",
    "xaero",
    "sound",
    "recipe",
    "tags/",
    "/tags",
    "loot",
    "advancement",
];

/// Known ore materials used for resource inference and matching.
const RESOURCE_PATTERNS: &[&str] = &[
    "copper",
    "tin",
    "lead",
    "silver",
    "nickel",
    "zinc",
    "osmium",
    "uranium",
    "aluminum",
    "aluminium",
    "bauxite",
    "cobalt",
    "ruby",
    "sapphire",
    "amethyst",
    "topaz",
    "peridot",
    "tungsten",
    "platinum",
    "iridium",
    "titanium",
    "chromium",
    "quartz",
    "certus",
    "fluorite",
    "sulfur",
    "saltpeter",
    "coal",
    "iron",
    "gold",
    "diamond",
    "emerald",
    "redstone",
    "lapis",
    "netherite",
    "ancient_debris",
];

/// Known suffixes that identify vein-size keys.
const VEIN_SIZE_SUFFIXES: &[&str] = &[
    "veinsize",
    "vein_size",
    "clustersize",
    "cluster_size",
    "maxveinsize",
    "veincount",
    "countpervein",
    "pervein",
    "maxpercluster",
    "maxsize",
];

/// Known suffixes for height-range keys.
const HEIGHT_SUFFIXES: &[(&[&str], &[&str])] = &[(
    &[
        "minheight",
        "min_height",
        "miny",
        "min_y",
        "bottomy",
        "bottomoffset",
        "minworldheight",
        "minimumheight",
        "starty",
    ],
    &[
        "maxheight",
        "max_height",
        "maxy",
        "max_y",
        "topy",
        "topoffset",
        "maxworldheight",
        "maximumheight",
        "endy",
    ],
)];

/// Known suffixes for frequency keys.
const FREQUENCY_SUFFIXES: &[&str] = &[
    "spawnsperchunk",
    "spawns_per_chunk",
    "perchunk",
    "spawnchance",
    "spawnrate",
    "veinperchunk",
    "countperchunk",
];

/// Whether a config relative path should be considered for heuristic ore scanning.
pub fn is_plausible_ore_config_path(path: &str) -> bool {
    let lower = path.replace('\\', "/").to_lowercase();
    if SKIP_PATH_MARKERS.iter().any(|m| lower.contains(m)) {
        return false;
    }
    // Client-only configs almost never control worldgen.
    if lower.contains("-client.") || lower.contains("_client.") || lower.ends_with("/client.toml") {
        return false;
    }
    true
}

/// Scans a flat map of (config_file_path, content) for ore-generation
/// keys using heuristic patterns without any pre-existing knowledge base.
pub fn scan_configs_for_ore_gen(config_contents: &[(String, String)]) -> Vec<HeuristicOreGen> {
    let mut results = Vec::new();

    for (file_path, content) in config_contents {
        if !is_plausible_ore_config_path(file_path) {
            continue;
        }

        let lines: Vec<&str> = content.lines().collect();
        // Track `[tin]` / `[world.copper]` so bare `shouldGenerate` resolves to a material.
        let mut toml_section: Option<String> = None;

        for (line_no, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
                continue;
            }

            if let Some(section) = parse_toml_section(trimmed) {
                toml_section = Some(section);
                continue;
            }

            let (key, value) = if let Some((k, v)) = parse_toml_kv(trimmed) {
                (k, v)
            } else if let Some((k, v)) = parse_json_kv(trimmed) {
                (k, v)
            } else if let Some((k, v)) = parse_cfg_kv(trimmed) {
                (k, v)
            } else {
                continue;
            };

            if !is_ore_gen_key(key) {
                continue;
            }

            // Skip non-toggle values for enable-like keys (arrays/objects are noise).
            if looks_like_structure_value(value) {
                continue;
            }

            let Some(resource_name) = infer_resource_name(key, file_path).or_else(|| {
                toml_section
                    .as_deref()
                    .and_then(|s| infer_resource_name(s, file_path))
            }) else {
                continue;
            };

            let vein_size =
                find_related_key(&lines, line_no, VEIN_SIZE_SUFFIXES, &resource_name, true);
            let frequency =
                find_related_key(&lines, line_no, FREQUENCY_SUFFIXES, &resource_name, true);
            let (min_height, max_height) = find_height_range(&lines, line_no, &resource_name, true);

            let key_lower = key.to_lowercase();
            let confidence = if STRONG_KEY_PATTERNS.iter().any(|p| key_lower.contains(p))
                || RESOURCE_PATTERNS.iter().any(|r| key_lower.contains(r))
            {
                HeuristicConfidence::Medium
            } else {
                HeuristicConfidence::Low
            };

            results.push(HeuristicOreGen {
                resource_name,
                config_file: file_path.clone(),
                enabled_key: key.to_string(),
                enabled_value: value.to_string(),
                vein_size,
                min_height,
                max_height,
                spawns_per_chunk: frequency,
                confidence,
            });
        }
    }

    // Deduplicate by (resource_name, config_file) — keep first (stronger) hit.
    let mut seen = std::collections::HashSet::new();
    results.retain(|r| seen.insert((r.resource_name.clone(), r.config_file.clone())));

    results
}

/// Token-aware ore-gen key detection.
///
/// Intentionally does **not** use bare substrings like `"ore"` / `"gen"` / `"enable"`,
/// which false-positive on keys such as `ignoredItems` (`ign**ore**d`) and bare `enabled`.
fn is_ore_gen_key(key: &str) -> bool {
    let lower = key.to_lowercase();

    // Noise prefixes / recipe-tag bookkeeping — never ore gen.
    if lower.starts_with("ignored")
        || lower.starts_with("blacklist")
        || lower.starts_with("whitelist")
        || lower.contains("recipe")
        || lower.contains("tag")
    {
        return false;
    }

    if STRONG_KEY_PATTERNS.iter().any(|p| lower.contains(p)) {
        return true;
    }

    let tokens = tokenize_identifier(key);
    if tokens.is_empty() {
        return false;
    }

    let has_ore_token = tokens.iter().any(|t| t == "ore" || t == "ores");
    let has_gen_token = tokens.iter().any(|t| {
        matches!(
            t.as_str(),
            "generate" | "generation" | "worldgen" | "spawn" | "spawns" | "gen"
        )
    });
    let has_toggle_token = tokens.iter().any(|t| {
        matches!(
            t.as_str(),
            "enable" | "enabled" | "disable" | "disabled" | "should" | "allow" | "can"
        )
    });
    let has_material = tokens
        .iter()
        .any(|t| RESOURCE_PATTERNS.iter().any(|r| r == t))
        || RESOURCE_PATTERNS.iter().any(|r| lower.contains(r));

    // e.g. enableCopperOre, copperOreEnabled, generateTin
    if has_ore_token && (has_gen_token || has_toggle_token || has_material) {
        return true;
    }
    // e.g. tinShouldGenerate, generateOsmium (material + gen verb)
    if has_material && has_gen_token {
        return true;
    }
    // e.g. zincWorldGen — material already covered; worldgen is a strong token alone with material
    if has_material && tokens.iter().any(|t| t == "worldgen") {
        return true;
    }

    false
}

fn looks_like_structure_value(value: &str) -> bool {
    let v = value.trim();
    v.starts_with('[') || v.starts_with('{') || v == "null" || v.is_empty()
}

/// Split `enableCopperOre` / `tin_should_generate` into lowercase tokens.
fn tokenize_identifier(key: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();

    for (i, ch) in key.chars().enumerate() {
        if !ch.is_alphanumeric() {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current).to_lowercase());
            }
            continue;
        }
        let is_boundary = ch.is_uppercase()
            && i > 0
            && key
                .chars()
                .nth(i - 1)
                .map(|p| p.is_lowercase() || p.is_ascii_digit())
                .unwrap_or(false);
        if is_boundary && !current.is_empty() {
            tokens.push(std::mem::take(&mut current).to_lowercase());
        }
        current.push(ch);
    }
    if !current.is_empty() {
        tokens.push(current.to_lowercase());
    }
    tokens
}

fn parse_toml_section(line: &str) -> Option<String> {
    let t = line.trim();
    if !(t.starts_with('[') && t.ends_with(']') && !t.starts_with("[[")) {
        return None;
    }
    let inner = t.trim_start_matches('[').trim_end_matches(']').trim();
    if inner.is_empty() {
        return None;
    }
    // Prefer the leaf: `[world.tin]` → tin
    Some(inner.split('.').last().unwrap_or(inner).to_string())
}

fn parse_toml_kv(line: &str) -> Option<(&str, &str)> {
    let eq_pos = line.find('=')?;
    let key = line[..eq_pos].trim();
    let value = line[eq_pos + 1..].trim().trim_matches('"');
    if key.is_empty() || key.contains('[') {
        return None;
    }
    Some((key, value))
}

fn parse_json_kv(line: &str) -> Option<(&str, &str)> {
    let line = line.trim().trim_end_matches(',');
    // Require a quoted key so `minecraft:iron_ore` item ids are not parsed as KV.
    if !line.starts_with('"') {
        return None;
    }
    let colon = line.find(':')?;
    let key = line[..colon].trim().trim_matches('"');
    let value = line[colon + 1..].trim().trim_matches('"');
    if key.is_empty() || key.contains(':') {
        return None;
    }
    Some((key, value))
}

fn parse_cfg_kv(line: &str) -> Option<(&str, &str)> {
    let line = line
        .trim_start_matches("B:")
        .trim_start_matches("I:")
        .trim_start_matches("S:");
    let eq_pos = line.find('=')?;
    let key = line[..eq_pos].trim();
    let value = line[eq_pos + 1..].trim();
    if key.is_empty() {
        return None;
    }
    Some((key, value))
}

fn infer_resource_name(key: &str, file_path: &str) -> Option<String> {
    let lower = key.to_lowercase();

    for res in RESOURCE_PATTERNS {
        if lower.contains(res) {
            return Some((*res).to_string());
        }
    }

    // Section-style keys: "tin.shouldGenerate" → tin
    if let Some((head, _)) = key.split_once('.') {
        let head_l = head.to_lowercase();
        if RESOURCE_PATTERNS.iter().any(|r| *r == head_l) {
            return Some(head_l);
        }
        let tokens = tokenize_identifier(head);
        if let Some(t) = tokens
            .into_iter()
            .find(|t| RESOURCE_PATTERNS.iter().any(|r| *r == t))
        {
            return Some(t);
        }
    }

    // Filename hint: config/thermal/ores/tin.toml
    let path_l = file_path.replace('\\', "/").to_lowercase();
    for res in RESOURCE_PATTERNS {
        if path_l.contains(res) {
            return Some((*res).to_string());
        }
    }

    // Without a known material, refuse weak stems like "enabled" / "ignoreditems".
    None
}

fn find_related_key(
    lines: &[&str],
    center: usize,
    suffixes: &[&str],
    ore_prefix: &str,
    allow_section_local: bool,
) -> Option<(String, String)> {
    let prefix_lower = ore_prefix.to_lowercase();
    for window in [8usize, 20] {
        let start = center.saturating_sub(window);
        let end = (center + window).min(lines.len());

        for (idx, line) in lines[start..end].iter().enumerate() {
            let absolute = start + idx;
            if section_boundary_between(lines, center, absolute) {
                continue;
            }
            if let Some((k, v)) = parse_toml_kv(line)
                .or_else(|| parse_json_kv(line))
                .or_else(|| parse_cfg_kv(line))
            {
                let kl = k.to_lowercase();
                let suffix_hit = suffixes.iter().any(|s| kl.contains(s));
                if !suffix_hit {
                    continue;
                }
                if kl.contains(&prefix_lower)
                    || (allow_section_local && same_toml_section(lines, center, absolute))
                {
                    return Some((k.to_string(), v.to_string()));
                }
            }
        }
    }
    None
}

fn find_height_range(
    lines: &[&str],
    center: usize,
    ore_prefix: &str,
    allow_section_local: bool,
) -> (Option<(String, String)>, Option<(String, String)>) {
    let prefix_lower = ore_prefix.to_lowercase();
    let (min_sfx, max_sfx) = &HEIGHT_SUFFIXES[0];
    let mut min = None;
    let mut max = None;

    for window in [10usize, 24] {
        let start = center.saturating_sub(window);
        let end = (center + window).min(lines.len());

        for (idx, line) in lines[start..end].iter().enumerate() {
            let absolute = start + idx;
            if section_boundary_between(lines, center, absolute) {
                continue;
            }
            if let Some((k, v)) = parse_toml_kv(line)
                .or_else(|| parse_json_kv(line))
                .or_else(|| parse_cfg_kv(line))
            {
                let kl = k.to_lowercase();
                let in_scope = kl.contains(&prefix_lower)
                    || (allow_section_local && same_toml_section(lines, center, absolute));
                if !in_scope {
                    continue;
                }
                if min.is_none() && min_sfx.iter().any(|s| kl.contains(s)) {
                    min = Some((k.to_string(), v.to_string()));
                }
                if max.is_none() && max_sfx.iter().any(|s| kl.contains(s)) {
                    max = Some((k.to_string(), v.to_string()));
                }
            }
        }
        if min.is_some() && max.is_some() {
            break;
        }
    }
    (min, max)
}

/// True when `a` and `b` sit under the same nearest preceding `[section]` header.
/// Returns false when either side has no section (avoids flat-file cross-talk).
fn same_toml_section(lines: &[&str], a: usize, b: usize) -> bool {
    match (section_at(lines, a), section_at(lines, b)) {
        (Some(sa), Some(sb)) => sa == sb,
        _ => false,
    }
}

fn section_at(lines: &[&str], idx: usize) -> Option<String> {
    for line in lines[..=idx.min(lines.len().saturating_sub(1))]
        .iter()
        .rev()
    {
        if let Some(s) = parse_toml_section(line.trim()) {
            return Some(s);
        }
    }
    None
}

fn section_boundary_between(lines: &[&str], a: usize, b: usize) -> bool {
    if a == b {
        return false;
    }
    let (lo, hi) = if a < b { (a + 1, b) } else { (b + 1, a) };
    for line in &lines[lo..=hi.min(lines.len().saturating_sub(1))] {
        if parse_toml_section(line.trim()).is_some() {
            return true;
        }
    }
    false
}

/// Classifies a Minecraft item name into a (material, type) pair by
/// pattern matching on common naming conventions.
///
/// Examples:
///   "tin_ingot" → ("tin", "ingot")
///   "ingotTin" → ("tin", "ingot")
///   "copper_block" → ("copper", "block")
///   "iron_nugget" → ("iron", "nugget")
pub fn classify_item(item_id: &str) -> Option<(String, String)> {
    let item_types = [
        "ingot",
        "nugget",
        "block",
        "ore",
        "dust",
        "plate",
        "gear",
        "rod",
        "gem",
        "raw_ore",
        "raw_block",
        "crystal",
        "shard",
        "clump",
        "dirty_dust",
        "slurry",
        "seed",
        "pellet",
        "deepslate_ore",
        "nether_ore",
        "end_ore",
    ];

    let id = item_id.to_lowercase();

    // Pattern: {material}_{type}
    for ty in &item_types {
        if let Some(stripped) = id.strip_suffix(&format!("_{ty}")) {
            if !stripped.is_empty() {
                return Some((stripped.to_string(), ty.to_string()));
            }
        }
    }

    // Pattern: {type}_{material} (camelCase prefix)
    for ty in &item_types {
        if let Some(stripped) = id.strip_prefix(&format!("{ty}_")) {
            if !stripped.is_empty() && !item_types.contains(&stripped) {
                return Some((stripped.to_string(), ty.to_string()));
            }
        }
    }

    // Pattern: {type}{Material} (camelCase)
    for ty in item_types {
        if let Some(stripped) = id.strip_prefix(ty) {
            if !stripped.is_empty() && stripped.chars().next().map_or(false, |c| c.is_uppercase()) {
                let material = decapitalize_first(stripped);
                return Some((material, ty.to_string()));
            }
        }
    }

    None
}

fn decapitalize_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_lowercase().collect::<String>() + c.as_str(),
    }
}

/// Groups items by their material, across different types.
/// E.g. ["copper_ingot", "copper_block", "copper_nugget", "tin_ingot", "tin_block"]
/// → {"copper": {"ingot", "block", "nugget"}, "tin": {"ingot", "block"}}
pub fn group_items_by_material(item_ids: &[String]) -> HashMap<String, Vec<String>> {
    let mut groups: HashMap<String, Vec<String>> = HashMap::new();
    for id in item_ids {
        if let Some((material, _ty)) = classify_item(id) {
            groups.entry(material).or_default().push(id.clone());
        }
    }
    groups
}

/// Detects duplicate resources across mods — if two mods both add
/// "tin_ingot", returns a grouping.
pub fn detect_duplicate_groups(mod_items: &[(String, Vec<String>)]) -> Vec<DuplicateItemGroup> {
    let mut by_material: HashMap<String, Vec<(String, String)>> = HashMap::new();

    for (mod_id, items) in mod_items {
        for item in items {
            if let Some((material, _ty)) = classify_item(item) {
                by_material
                    .entry(material)
                    .or_default()
                    .push((mod_id.clone(), item.clone()));
            }
        }
    }

    by_material
        .into_iter()
        .filter(|(_, entries)| {
            let mods: std::collections::HashSet<_> = entries.iter().map(|(m, _)| m).collect();
            mods.len() > 1
        })
        .map(|(material, entries)| DuplicateItemGroup { material, entries })
        .collect()
}

#[derive(Debug, Clone)]
pub struct DuplicateItemGroup {
    pub material: String,
    pub entries: Vec<(String, String)>, // (mod_id, item_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_ingots() {
        assert_eq!(
            classify_item("tin_ingot"),
            Some(("tin".into(), "ingot".into()))
        );
        assert_eq!(
            classify_item("copper_block"),
            Some(("copper".into(), "block".into()))
        );
        assert_eq!(
            classify_item("iron_nugget"),
            Some(("iron".into(), "nugget".into()))
        );
    }

    #[test]
    fn scans_toml_for_ore_gen() {
        let toml = "[world]
enableCopperOre = true
copperVeinSize = 8
copperMinHeight = -16
copperMaxHeight = 112
";
        let results =
            scan_configs_for_ore_gen(&[("config/mekanism/world.toml".into(), toml.into())]);
        assert!(!results.is_empty());
        assert_eq!(results[0].resource_name, "copper");
        assert_eq!(
            results[0].min_height.as_ref().map(|(_, v)| v.as_str()),
            Some("-16")
        );
        assert_eq!(
            results[0].max_height.as_ref().map(|(_, v)| v.as_str()),
            Some("112")
        );
    }

    #[test]
    fn scans_section_style_should_generate() {
        let toml = "[tin]
shouldGenerate = true
perChunk = 4
bottomOffset = -32
topOffset = 64
";
        let results =
            scan_configs_for_ore_gen(&[("config/mekanism/world.toml".into(), toml.into())]);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].resource_name, "tin");
        assert_eq!(
            results[0]
                .spawns_per_chunk
                .as_ref()
                .map(|(_, v)| v.as_str()),
            Some("4")
        );
        assert_eq!(
            results[0].min_height.as_ref().map(|(_, v)| v.as_str()),
            Some("-32")
        );
        assert_eq!(
            results[0].max_height.as_ref().map(|(_, v)| v.as_str()),
            Some("64")
        );
    }

    #[test]
    fn ignores_almostunified_unify_noise() {
        let json = r#"{
  "modPriorities": ["minecraft", "kubejs"],
  "ignoredItems": ["minecraft:iron_ore"],
  "ignoredRecipeTypes": ["minecraft:smelting"],
  "ignoredRecipes": ["minecraft:iron_ingot"],
  "enabled": true,
  "tags": true
}
"#;
        let results =
            scan_configs_for_ore_gen(&[("config/almostunified/unify.json".into(), json.into())]);
        assert!(
            results.is_empty(),
            "almostunified unify.json must not produce ore hits: {results:?}"
        );
    }

    #[test]
    fn ignores_bare_enabled_in_random_config() {
        let toml = "enabled = true\ndebug = false\n";
        let results =
            scan_configs_for_ore_gen(&[("config/some-mod/general.toml".into(), toml.into())]);
        assert!(results.is_empty());
    }

    #[test]
    fn create_disable_zinc_ore_detected() {
        let toml = "disableZincOre = false\n";
        let results =
            scan_configs_for_ore_gen(&[("config/create-common.toml".into(), toml.into())]);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].resource_name, "zinc");
    }

    #[test]
    fn tokenize_camel_case() {
        assert_eq!(
            tokenize_identifier("enableCopperOre"),
            vec!["enable", "copper", "ore"]
        );
        assert_eq!(
            tokenize_identifier("tin_should_generate"),
            vec!["tin", "should", "generate"]
        );
    }

    #[test]
    fn detect_duplicates_across_mods() {
        let mods = vec![
            (
                "mekanism".into(),
                vec!["tin_ingot".into(), "copper_ingot".into()],
            ),
            (
                "thermal".into(),
                vec!["tin_ingot".into(), "tin_block".into(), "lead_ingot".into()],
            ),
        ];
        let groups = detect_duplicate_groups(&mods);
        assert_eq!(groups.len(), 1); // tin is duplicated, copper is not
        assert_eq!(groups[0].material, "tin");
    }

    #[test]
    fn flat_file_does_not_cross_link_neighbor_ores() {
        let toml = "enableCopperOre = true
enableTinOre = true
tinVeinSize = 8
tinMinHeight = -16
tinMaxHeight = 64
";
        let results =
            scan_configs_for_ore_gen(&[("config/example-common.toml".into(), toml.into())]);
        let copper = results
            .iter()
            .find(|r| r.resource_name == "copper")
            .expect("copper");
        assert!(
            copper.vein_size.is_none(),
            "copper must not steal tinVeinSize"
        );
        assert!(copper.min_height.is_none());
        let tin = results
            .iter()
            .find(|r| r.resource_name == "tin")
            .expect("tin");
        assert_eq!(tin.vein_size.as_ref().map(|(_, v)| v.as_str()), Some("8"));
    }

    #[test]
    fn plausible_path_skips_unify() {
        assert!(!is_plausible_ore_config_path(
            "config/almostunified/unify.json"
        ));
        assert!(is_plausible_ore_config_path("config/mekanism/world.toml"));
    }
}
