//! Category-aware classification of mods, used by the mod-conflict resolver to
//! decide which side of a conflict is safely replaceable (can be disabled)
//! versus which side represents content the user wants to keep.
//!
//! Categories are deliberate, serializable enums (never free-form strings) so
//! the classification survives JSON round-trips into the launcher/UI.

use crate::knowledge::ModKnowledgeEntry;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ModCategory {
    /// Core API / library another mod needs (fabric-api, cloth-config, …).
    Library,
    /// A compat bridge / companion (indium bridges Fabric API & Sodium; …).
    ApiCompanion,
    /// Content the player consciously added (quest packs, maps, additions).
    Content,
    /// Pure performance mod (sodium, lithium, modernfix, …).
    Optimization,
    /// Rendering / shader system (iris, oculus, optifine, …).
    RenderCompat,
    /// Worldgen (terrablender, biomes-o-plenty, …).
    Worldgen,
    /// Machine / tech mods (create, mekanism, AE2, …).
    Tech,
    /// Magic mods (botania, …).
    Magic,
    /// Decor / building mods (quark, …).
    Decor,
    /// Quality-of-life / UI (jei, emi, rei, minimaps, …).
    Qol,
    /// Abandoned / built for another stack / old build.
    Legacy,
    /// Two copies of the same mod.
    Duplicate,
    /// Not classified.
    Unknown,
}

/// How safe it is to disable a mod of this category (0 = do not disable,
/// 100 = trivially replaceable). Drives the resolver's first-choice default.
pub fn replaceability(cat: ModCategory) -> u8 {
    match cat {
        ModCategory::Optimization => 90,
        ModCategory::Duplicate => 95,
        ModCategory::RenderCompat => 85,
        ModCategory::Legacy => 80,
        ModCategory::ApiCompanion => 60,
        ModCategory::Qol => 45,
        ModCategory::Worldgen => 40,
        ModCategory::Unknown => 40,
        ModCategory::Library => 30,
        ModCategory::Tech => 20,
        ModCategory::Magic => 20,
        ModCategory::Decor => 15,
        ModCategory::Content => 10,
    }
}

/// True when disabling a mod of this category is considered safe/reversible
/// enough to be offered as an automatic first-choice fix.
pub fn is_safe_to_disable(cat: ModCategory) -> bool {
    replaceability(cat) >= 60
}

/// The resolver's default initial action for a category available as a target.
pub fn default_action(cat: ModCategory) -> &'static str {
    match cat {
        ModCategory::Library => "update",
        ModCategory::Content => "keep",
        ModCategory::Duplicate
        | ModCategory::Optimization
        | ModCategory::RenderCompat
        | ModCategory::Legacy
        | ModCategory::ApiCompanion => "disable",
        _ => "review",
    }
}

/// Map a `knowledge/builtin` string category onto the enum.
fn from_knowledge_category(raw: &str) -> Option<ModCategory> {
    Some(match raw.to_ascii_lowercase().as_str() {
        "optimization" => ModCategory::Optimization,
        "shader" | "render" => ModCategory::RenderCompat,
        "technology" => ModCategory::Tech,
        "worldgen" => ModCategory::Worldgen,
        "magic" => ModCategory::Magic,
        "decoration" | "decor" => ModCategory::Decor,
        "food" | "adventure" | "game-mechanics" => ModCategory::Content,
        "library" | "api" => ModCategory::Library,
        "utility" => ModCategory::Qol,
        _ => return None,
    })
}

const LIBRARY_SLUGS: &[&str] = &[
    "fabric-api",
    "cloth-config",
    "clothconfig",
    "architectury",
    "architectury-api",
    "geckolib",
    "forge-config-api-port",
    "forgeconfigapiport",
    "yacl",
    "yet-another-config-lib",
    "mixin-extras",
    "mixinextras",
    "sponge-mixin",
    "fabric-language-kotlin",
    "boosted-yaml",
    "owo-lib",
    "owo-lib-impl",
    "playeranimator",
    "smartbrainlib",
    "cristel-lib",
    "puzzles-lib",
    "bclib",
    "followerscompatibility",
    "packmenu-api",
    "trinkets",
    "structures-api",
];

const OPTIMIZATION_SLUGS: &[&str] = &[
    "sodium",
    "sodium-extra",
    "lithium",
    "phosphor",
    "ferritecore",
    "ferrite-core",
    "modernfix",
    "embeddium",
    "rubidium",
    "starlight",
    "krypton",
    "canary",
    "lazydfu",
    "lazy-dfu",
    "entityculling",
    "immediatelyfast",
    "dynamic-fps",
    "exordium",
    "memoryleakfix",
    "voxy",
    "distant-horizons",
    "smoothboot",
    "threadtweak",
    "badoptimizations",
    "noisium",
    "c2me",
    "bobby",
    "very-many-players-fabric",
];

const RENDER_SLUGS: &[&str] = &[
    "iris",
    "oculus",
    "optifine",
    "optifabric",
    "canvas",
    "vulkanmod",
    "sodium-fabric",
    "sodium-forge",
    "embeddiumplus",
];

const API_COMPANION_SLUGS: &[&str] = &[
    "indium",
    "continuity",
    "indium-fabric",
    "fabricskyboxes",
    "fabricskyboxes-interop",
    "sodiumextras",
    "reeses-sodium-options",
    "command-structures-components",
    "iris-compat",
];

const WORLDGEN_SLUGS: &[&str] = &[
    "terrablender",
    "biomes-o-plenty",
    "terralith",
    "tectonic",
    "nature's-spirit",
    "natures-spirit",
    "betterend",
    "betternether",
    "promenade",
    "regions-unexplored",
];

const TECH_SLUGS: &[&str] = &[
    "create",
    "mekanism",
    "thermal-expansion",
    "thermal-foundation",
    "immersive-engineering",
    "tconstruct",
    "applied-energistics-2",
    "refined-storage",
    "industrial-(re)craft",
    "littlelogistics",
    "framedblocks",
];

const MAGIC_SLUGS: &[&str] = &[
    "botania",
    "ars-nouveau",
    "malum",
    "occultism",
    "allthetweaks",
];

const DECOR_SLUGS: &[&str] = &["quark", "decorative-blocks", "charm", "supplementaries"];

const QOL_SLUGS: &[&str] = &[
    "jei",
    "jem",
    "emi",
    "rei",
    "roughlyenoughitems",
    "xaeros-minimap",
    "journeymap",
    "worldedit",
    "sodium-rendering",
    "appleclient",
];

const LEGACY_MARKERS: &[&str] = &[
    "legacy",
    "abandoned",
    "deprecated",
    "old",
    "orphan",
    "1.12",
    "1.16",
    "1.18",
    "pre-rework",
];

/// Classify a mod by slug (and name as a fallback). Exact matches and known
/// aliases win; then knowledge/builtin categories; then word-based heuristics.
pub fn classify(slug: &str, name: &str) -> ModCategory {
    let slug_l = slug.to_ascii_lowercase();
    let key = normalize_mod_key(&slug_l);

    if OPTIMIZATION_SLUGS.iter().any(|s| key == *s)
        || RENDER_SLUGS
            .iter()
            .any(|s| key.starts_with(s) || prefix_variant(&key, s))
    {
        if RENDER_SLUGS.iter().any(|s| key == *s || key.starts_with(s)) {
            return ModCategory::RenderCompat;
        }
        return ModCategory::Optimization;
    }
    if LibraryMatch::matches(&key)
        || LIBRARY_SLUGS
            .iter()
            .any(|s| key == *s || key.starts_with(s))
    {
        return ModCategory::Library;
    }
    if API_COMPANION_SLUGS
        .iter()
        .any(|s| key == *s || key.starts_with(s))
    {
        return ModCategory::ApiCompanion;
    }
    if WORLDGEN_SLUGS.iter().any(|s| key == *s) {
        return ModCategory::Worldgen;
    }
    if TECH_SLUGS.iter().any(|s| key == *s) {
        return ModCategory::Tech;
    }
    if MAGIC_SLUGS.iter().any(|s| key == *s) {
        return ModCategory::Magic;
    }
    if DECOR_SLUGS.iter().any(|s| key == *s) {
        return ModCategory::Decor;
    }
    if QOL_SLUGS.iter().any(|s| key == *s) {
        return ModCategory::Qol;
    }
    // spb-revamped and friends are content packs (SP-Backrooms & alike).
    if key.starts_with("spb-revamped")
        || key.contains("spb-revamped")
        || key.ends_with("revamped")
        || key.contains("-backrooms")
        || content_keyword(&key)
    {
        return ModCategory::Content;
    }
    if LEGACY_MARKERS.iter().any(|m| key.contains(m)) {
        return ModCategory::Legacy;
    }

    // Consult the built-in knowledge base (category strings already curated).
    if let Some(entry) = ModKnowledgeEntry::lookup(slug) {
        if let Some(cat) = from_knowledge_category(&entry.category) {
            return cat;
        }
    }
    if let Some(cat) = ModKnowledgeEntry::lookup(&key) {
        if let Some(mapped) = from_knowledge_category(&cat.category) {
            return mapped;
        }
    }
    // Name-based heuristics as a last pass.
    let name_l = name.to_ascii_lowercase();
    if name_l.contains("optimiz") || name_l.contains("performance") || name_l.contains(" fps") {
        return ModCategory::Optimization;
    }
    if name_l.contains("shader") || name_l.contains("optifine") || name_l.contains("iris") {
        return ModCategory::RenderCompat;
    }
    if name_l.contains("api") || name_l.contains("library") || name_l.contains("lib") {
        return ModCategory::Library;
    }
    ModCategory::Unknown
}

/// Whether a slug suggests an invented vanilla-resource mod id (noise).
fn content_keyword(key: &str) -> bool {
    key.contains("quest")
        || key.contains("adventure")
        || key.contains("pack")
        || key == "spb-revamped"
}

fn normalize_mod_key(s: &str) -> String {
    s.trim()
        .to_ascii_lowercase()
        .replace('_', "-")
        .replace(' ', "-")
}

fn prefix_variant(key: &str, slug: &str) -> bool {
    key.starts_with(&format!("{slug}-")) || key.starts_with(&format!("{slug}."))
}

/// Very small matcher so the pure slug table stays readable.
struct LibraryMatch;
impl LibraryMatch {
    fn matches(key: &str) -> bool {
        key == "api"
            || key.starts_with("fabric-api")
            || key.contains("lib")
            || key.ends_with("-lib")
            || key.starts_with("kubejs")
            || key == "fabric"
    }
}

/// True when a mod slug is flagged legacy (abandoned / old-build / fork-base).
pub fn is_legacy(slug: &str) -> bool {
    classify(slug, "") == ModCategory::Legacy
        || LEGACY_MARKERS
            .iter()
            .any(|m| slug.to_ascii_lowercase().contains(m))
}

/// Known conflict slugs for a mod (from the curated knowledge base).
pub fn known_conflicts(slug: &str) -> Vec<String> {
    ModKnowledgeEntry::lookup(slug)
        .map(|e| e.known_conflicts.clone())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(slug: &str, name: &str) -> ModCategory {
        classify(slug, name)
    }

    #[test]
    fn classifies_builtin_candidates() {
        assert_eq!(c("sodium", "Sodium"), ModCategory::Optimization);
        assert_eq!(c("sodium-extra", "Sodium Extra"), ModCategory::Optimization);
        assert_eq!(c("lithium", "Lithium"), ModCategory::Optimization);
        assert_eq!(c("modernfix", "ModernFix"), ModCategory::Optimization);
        assert_eq!(c("embeddium", "Embeddium"), ModCategory::Optimization);
    }

    #[test]
    fn classifies_bridges_and_render() {
        assert_eq!(c("indium", "Indium"), ModCategory::ApiCompanion);
        assert_eq!(c("iris", "Iris Shaders"), ModCategory::RenderCompat);
        assert_eq!(c("oculus", "Oculus"), ModCategory::RenderCompat);
        assert_eq!(c("optifine", "OptiFine"), ModCategory::RenderCompat);
    }

    #[test]
    fn classifies_libraries() {
        assert_eq!(c("fabric-api", "Fabric API"), ModCategory::Library);
        assert_eq!(c("cloth-config", "Cloth Config"), ModCategory::Library);
        assert_eq!(c("kubejs", "KubeJS"), ModCategory::Library);
    }

    #[test]
    fn classifies_content_and_legacy() {
        assert_eq!(c("spb-revamped", "SP-Backrooms"), ModCategory::Content);
        assert_eq!(
            c("spb-revamped-fabric-1.2.0", "SP-Backrooms"),
            ModCategory::Content
        );
    }

    #[test]
    fn classifies_unknown() {
        assert_eq!(c("zz-mystic", ""), ModCategory::Unknown);
        assert_eq!(c("just-unlisted-mod", "Foo"), ModCategory::Unknown);
    }

    #[test]
    fn replaceability_prefers_disableable_over_content() {
        assert!(replaceability(ModCategory::Optimization) > replaceability(ModCategory::Content));
        assert!(replaceability(ModCategory::Duplicate) >= 90);
        assert!(is_safe_to_disable(ModCategory::Optimization));
        assert!(is_safe_to_disable(ModCategory::ApiCompanion));
        assert!(!is_safe_to_disable(ModCategory::Content));
    }

    #[test]
    fn legacy_detection() {
        assert!(is_legacy("some-old-fabric-mod"));
        assert!(is_legacy("spb-revamped-legacy"));
    }

    #[test]
    fn known_conflicts_from_builtin() {
        let conflicts = known_conflicts("sodium");
        assert!(!conflicts.is_empty());
        assert!(conflicts.iter().any(|c| c.eq_ignore_ascii_case("optifine")));
    }
}
