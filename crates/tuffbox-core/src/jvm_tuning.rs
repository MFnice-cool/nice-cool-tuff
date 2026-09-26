//! Loader/version-aware JVM + heap auto-tuning for launches.
//!
//! Two per-instance recommendations, computed at launch (and previewed in
//! Project Settings):
//! - JVM flags: a GC profile picked from the loader kind, the game version,
//!   and the resolved Java major. Vanilla always gets ZGC (generational on
//!   Java 21+); modded packs get a G1 tuning (Aikar-style when heavy).
//! - Heap size: a loader base plus per-mod demand from provider categories
//!   (`ModSource.categories` — the same taxonomy the dependency graph
//!   clusters by), clamped like the legacy estimator.
//!
//! The generated JVM flags NEVER contain heap-size options (`-Xmx`/`-Xms`/
//! `-Xmn`) — heap stays under the memory setting, which `-Xmx` is built from.

use serde::Serialize;
use std::collections::BTreeMap;

/// Extra heap (MB) one mod of the given provider category slug typically
/// needs. Modrinth taxonomy; CurseForge names arrive pre-normalized
/// (`normalize_mod_category`), so both providers share these weights.
pub fn category_ram_weight(category: &str) -> u32 {
    match category.trim().to_ascii_lowercase().as_str() {
        "worldgen" => 160,
        "technology" => 140,
        "magic" => 120,
        "adventure" => 100,
        "mobs" => 80,
        "game-mechanics" => 70,
        "minigame" => 60,
        "decoration" => 50,
        "equipment" | "storage" | "transportation" | "cursed" => 40,
        "food" => 30,
        "utility" => 25,
        "economy" | "social" | "management" => 20,
        "library" => 15,
        "optimization" => 10,
        _ => 60,
    }
}

/// Base heap (MB) before per-mod demand. Forge ships the heaviest runtime
/// (coremods/transformers); Fabric is leaner; vanilla needs the least.
pub fn loader_heap_base_mb(loader_kind: &str) -> u32 {
    match loader_kind.trim().to_ascii_lowercase().as_str() {
        "forge" | "neoforge" => 3072,
        "fabric" | "quilt" => 2560,
        _ => 2048,
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeapSlice {
    pub category: String,
    pub mods: usize,
    pub mb_per_mod: u32,
    pub mb: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeapRecommendation {
    pub memory_mb: u32,
    pub base_mb: u32,
    pub category_mb: u64,
    pub slices: Vec<HeapSlice>,
    pub mod_count: usize,
    pub loader: String,
    pub total_ram_mb: u64,
}

/// Category-aware heap recommendation. Each mod counts once, under its
/// heaviest category (a `technology`+`utility` mod costs 140, not 165).
/// Clamp [2048, 12288], 60%-of-RAM ceiling, 512 MB rounding — same semantics
/// as the legacy `recommend_memory_mb` estimator.
pub fn recommend_heap_mb(
    total_ram_mb: u64,
    loader_kind: &str,
    mods_categories: &[Vec<String>],
) -> HeapRecommendation {
    let base_mb = loader_heap_base_mb(loader_kind);
    let mut per_category: BTreeMap<String, usize> = BTreeMap::new();
    for cats in mods_categories {
        let mut best = ("uncategorized".to_string(), 0u32);
        for c in cats {
            let w = category_ram_weight(c);
            if w > best.1 {
                best = (c.trim().to_ascii_lowercase(), w);
            }
        }
        *per_category.entry(best.0).or_insert(0) += 1;
    }
    let mut slices = Vec::new();
    let mut category_mb: u64 = 0;
    for (category, mods) in &per_category {
        let per_mod = category_ram_weight(category);
        let mb = (*mods as u64).saturating_mul(per_mod as u64);
        category_mb = category_mb.saturating_add(mb);
        slices.push(HeapSlice {
            category: category.clone(),
            mods: *mods,
            mb_per_mod: per_mod,
            mb,
        });
    }
    slices.sort_by(|a, b| b.mb.cmp(&a.mb));
    let wanted = base_mb as u64 + category_mb;
    let memory_mb = if total_ram_mb == 0 {
        4096
    } else {
        let ceiling = (total_ram_mb * 60 / 100).max(2048);
        let mb = wanted.min(ceiling).clamp(2048, 12288);
        ((mb / 512) * 512) as u32
    };
    HeapRecommendation {
        memory_mb,
        base_mb,
        category_mb,
        slices,
        mod_count: mods_categories.len(),
        loader: loader_kind.to_string(),
        total_ram_mb,
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JvmRecommendation {
    /// `zgc-generational` | `zgc` | `g1-aikar` | `g1-balanced` | `g1-legacy` | `none`.
    pub profile: String,
    pub args: Vec<String>,
    pub java_major: u32,
    pub note: String,
}

pub fn is_vanilla_loader(loader_kind: &str) -> bool {
    matches!(
        loader_kind.trim().to_ascii_lowercase().as_str(),
        "vanilla" | "none" | ""
    )
}

/// Minor version of `1.MINOR…` (0 when unparseable).
pub fn mc_minor(mc_version: &str) -> u32 {
    mc_version
        .split('.')
        .nth(1)
        .and_then(|p| p.split('-').next())
        .and_then(|p| p.parse().ok())
        .unwrap_or(0)
}

fn loader_label(loader_kind: &str) -> String {
    match loader_kind.trim().to_ascii_lowercase().as_str() {
        "vanilla" | "none" | "" => "Vanilla".into(),
        "fabric" => "Fabric".into(),
        "forge" => "Forge".into(),
        "neoforge" => "NeoForge".into(),
        "quilt" => "Quilt".into(),
        other => {
            let mut chars = other.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => "Unknown".into(),
            }
        }
    }
}

/// Aikar-style G1 flags for heavy modded packs. Kept byte-identical to the
/// "Aikar's Optimized Flags" preset in Project Settings so applying the
/// recommendation lights up the matching preset card.
const AIKAR_G1: &[&str] = &[
    "-XX:+UseG1GC",
    "-XX:+ParallelRefProcEnabled",
    "-XX:MaxGCPauseMillis=200",
    "-XX:+UnlockExperimentalVMOptions",
    "-XX:+DisableExplicitGC",
    "-XX:+AlwaysPreTouch",
    "-XX:G1NewSizePercent=30",
    "-XX:G1MaxNewSizePercent=40",
    "-XX:G1ReservePercent=20",
    "-XX:G1HeapWastePercent=5",
    "-XX:G1MixedGCCountTarget=4",
    "-XX:InitiatingHeapOccupancyPercent=15",
    "-XX:G1MixedGCLiveThresholdPercent=90",
    "-XX:G1RSetUpdatingPauseTimePercent=5",
    "-XX:SurvivorRatio=32",
    "-XX:+PerfDisableSharedMem",
    "-XX:MaxTenuringThreshold=1",
];

/// Flags in [`AIKAR_G1`] that only exist on Java 9+ (fatal
/// "Unrecognized VM option" on Java 8) — dropped for old runtimes.
const AIKAR_JAVA9_ONLY: &[&str] = &[
    "-XX:G1MixedGCLiveThresholdPercent=90",
    "-XX:G1RSetUpdatingPauseTimePercent=5",
];

/// Balanced G1 for light modded packs (no heap-size-dependent flags).
const BALANCED_G1: &[&str] = &[
    "-XX:+UnlockExperimentalVMOptions",
    "-XX:+UseG1GC",
    "-XX:+ParallelRefProcEnabled",
    "-XX:MaxGCPauseMillis=100",
    "-XX:G1NewSizePercent=40",
];

/// Conservative G1 valid back to Java 7 (vanilla fallback below Java 15,
/// light modded packs on Java 8).
const LEGACY_G1: &[&str] = &[
    "-XX:+UseG1GC",
    "-XX:MaxGCPauseMillis=200",
    "-XX:+UnlockExperimentalVMOptions",
    "-XX:G1NewSizePercent=30",
    "-XX:+ParallelRefProcEnabled",
    "-XX:+DisableExplicitGC",
];

/// Pick GC flags for a pack. `java_major` is the resolved launch runtime
/// (at preview time, the required Java for the game version); `heavy_pack`
/// comes from the heap recommendation (big category demand or 100+ mods).
/// Never emits heap-size options — see the module docs.
pub fn recommend_jvm_args(
    loader_kind: &str,
    mc_version: &str,
    java_major: u32,
    heavy_pack: bool,
) -> JvmRecommendation {
    let label = loader_label(loader_kind);
    if java_major < 7 {
        return JvmRecommendation {
            profile: "none".into(),
            args: Vec::new(),
            java_major,
            note: format!(
                "{label} {mc_version} · Java {java_major}: too old for auto-tuning — flags left untouched."
            ),
        };
    }
    if is_vanilla_loader(loader_kind) {
        // Vanilla always runs ZGC: single-threaded-friendly, sub-ms pauses.
        if java_major >= 21 {
            return JvmRecommendation {
                profile: "zgc-generational".into(),
                args: ["-XX:+UseZGC", "-XX:+ZGenerational"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
                java_major,
                note: format!(
                    "Vanilla {mc_version} · Java {java_major}: generational ZGC for sub-millisecond pauses."
                ),
            };
        }
        if java_major >= 15 {
            return JvmRecommendation {
                profile: "zgc".into(),
                args: vec!["-XX:+UseZGC".into()],
                java_major,
                note: format!(
                    "Vanilla {mc_version} · Java {java_major}: ZGC (Java 21+ enables generational mode)."
                ),
            };
        }
        return JvmRecommendation {
            profile: "g1-legacy".into(),
            args: LEGACY_G1.iter().map(|s| s.to_string()).collect(),
            java_major,
            note: format!(
                "Vanilla {mc_version} · Java {java_major}: ZGC needs Java 15+ — G1 fallback."
            ),
        };
    }
    if heavy_pack {
        if java_major >= 9 {
            return JvmRecommendation {
                profile: "g1-aikar".into(),
                args: AIKAR_G1.iter().map(|s| s.to_string()).collect(),
                java_major,
                note: format!(
                    "Heavy {label} {mc_version} pack · Java {java_major}: Aikar-style G1 tuning."
                ),
            };
        }
        let args: Vec<String> = AIKAR_G1
            .iter()
            .filter(|f| !AIKAR_JAVA9_ONLY.contains(f))
            .map(|s| s.to_string())
            .collect();
        return JvmRecommendation {
            profile: "g1-aikar".into(),
            args,
            java_major,
            note: format!(
                "Heavy {label} {mc_version} pack · Java {java_major}: Aikar-style G1 without Java 9+ flags."
            ),
        };
    }
    if java_major >= 9 {
        return JvmRecommendation {
            profile: "g1-balanced".into(),
            args: BALANCED_G1.iter().map(|s| s.to_string()).collect(),
            java_major,
            note: format!("{label} {mc_version} · Java {java_major}: balanced G1 for a light pack."),
        };
    }
    JvmRecommendation {
        profile: "g1-legacy".into(),
        args: LEGACY_G1.iter().map(|s| s.to_string()).collect(),
        java_major,
        note: format!(
            "{label} {mc_version} · Java {java_major}: conservative G1 for an old runtime."
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cats(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn vanilla_gets_zgc() {
        let rec = recommend_jvm_args("vanilla", "1.21.4", 21, false);
        assert_eq!(rec.profile, "zgc-generational");
        assert_eq!(rec.args, vec!["-XX:+UseZGC", "-XX:+ZGenerational"]);
        let rec = recommend_jvm_args("vanilla", "1.20.1", 17, false);
        assert_eq!(rec.profile, "zgc");
        assert_eq!(rec.args, vec!["-XX:+UseZGC"]);
        // Old vanilla can't run ZGC (needs 15+) — G1 fallback, note says why.
        let rec = recommend_jvm_args("vanilla", "1.16.5", 8, false);
        assert_eq!(rec.profile, "g1-legacy");
        assert!(rec.args.contains(&"-XX:+UseG1GC".to_string()));
        assert!(rec.note.contains("Java 15+"));
    }

    #[test]
    fn modded_tiers() {
        let heavy = recommend_jvm_args("forge", "1.20.1", 17, true);
        assert_eq!(heavy.profile, "g1-aikar");
        assert_eq!(heavy.args.len(), AIKAR_G1.len());
        assert!(heavy.args.contains(&"-XX:MaxTenuringThreshold=1".to_string()));

        let heavy8 = recommend_jvm_args("forge", "1.16.5", 8, true);
        assert_eq!(heavy8.profile, "g1-aikar");
        for dropped in AIKAR_JAVA9_ONLY {
            assert!(!heavy8.args.contains(&dropped.to_string()), "{dropped} on Java 8");
        }

        let light = recommend_jvm_args("fabric", "1.21.4", 21, false);
        assert_eq!(light.profile, "g1-balanced");

        let legacy = recommend_jvm_args("forge", "1.12.2", 8, false);
        assert_eq!(legacy.profile, "g1-legacy");

        let ancient = recommend_jvm_args("forge", "1.6.4", 6, true);
        assert_eq!(ancient.profile, "none");
        assert!(ancient.args.is_empty());
    }

    #[test]
    fn no_heap_size_flags_and_unlock_order() {
        let matrix = [
            ("vanilla", "1.21.4", 21, false),
            ("vanilla", "1.20.1", 17, false),
            ("vanilla", "1.16.5", 8, false),
            ("forge", "1.20.1", 17, true),
            ("forge", "1.16.5", 8, true),
            ("fabric", "1.21.4", 21, false),
            ("neoforge", "1.21.1", 21, true),
            ("quilt", "1.20.4", 17, false),
        ];
        for (loader, mc, java, heavy) in matrix {
            let rec = recommend_jvm_args(loader, mc, java, heavy);
            for arg in &rec.args {
                for banned in ["-Xmx", "-Xms", "-Xmn"] {
                    assert!(!arg.starts_with(banned), "{arg} in {loader}/{mc}");
                }
            }
            // Unlock must precede the experimental G1 flags it enables.
            if let Some(unlock) = rec.args.iter().position(|a| a.contains("UnlockExperimental")) {
                for (i, arg) in rec.args.iter().enumerate() {
                    if arg.contains("G1NewSizePercent")
                        || arg.contains("G1HeapRegionSize")
                        || arg.contains("G1MixedGC")
                    {
                        assert!(unlock < i, "unlock after {arg} in {loader}/{mc}");
                    }
                }
            }
        }
    }

    #[test]
    fn heap_weights_and_caps() {
        assert_eq!(category_ram_weight("worldgen"), 160);
        assert_eq!(category_ram_weight("Technology"), 140);
        assert_eq!(category_ram_weight("optimization"), 10);
        assert_eq!(category_ram_weight("nope"), 60);

        // Multi-category mod counts once, under its heaviest tag.
        let mods = vec![cats(&["technology", "utility"]), cats(&["library"])];
        let rec = recommend_heap_mb(16384, "forge", &mods);
        assert_eq!(rec.mod_count, 2);
        assert_eq!(rec.base_mb, 3072);
        assert_eq!(rec.category_mb, 140 + 15);
        assert_eq!(rec.slices[0].category, "technology");

        // Vanilla with no mods = bare base.
        let rec = recommend_heap_mb(16384, "vanilla", &[]);
        assert_eq!(rec.memory_mb, 2048);

        // Heavy pack hits the absolute cap, rounded to 512.
        let many = vec![cats(&["worldgen"]); 200];
        let rec = recommend_heap_mb(65536, "forge", &many);
        assert_eq!(rec.memory_mb, 12288);

        // 60% RAM ceiling + 512 rounding.
        let rec = recommend_heap_mb(8192, "fabric", &vec![cats(&["magic"]); 30]);
        assert!(rec.memory_mb <= 8192 * 60 / 100 + 511);
        assert_eq!(rec.memory_mb % 512, 0);

        // Unknown machine → safe default.
        let rec = recommend_heap_mb(0, "forge", &many);
        assert_eq!(rec.memory_mb, 4096);
    }

    #[test]
    fn mc_minor_parses() {
        assert_eq!(mc_minor("1.21.4"), 21);
        assert_eq!(mc_minor("1.7.10"), 7);
        assert_eq!(mc_minor("garbage"), 0);
    }
}
