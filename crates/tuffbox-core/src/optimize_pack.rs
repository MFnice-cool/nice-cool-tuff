//! Pack optimization helpers: curated Fabric maps, ModernFix safety, config templates.
//!
//! Network resolve (Modrinth / CurseForge) lives in the desktop crate; this module
//! stays deterministic and AI-free.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::action_plan::{ActionPlan, LauncherAction, ACTION_PLAN_SCHEMA_VERSION};
use crate::manifest::ProjectManifest;

/// Bundled map: loader → MC version → Modrinth project.
const CURATED_PACKS_JSON: &str = include_str!("../data/optimize-packs.json");
const OPTIMIZE_MODS_JSON: &str = include_str!("../data/optimize-mods.json");

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CuratedPackRef {
    pub project_id: String,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CuratedPacksFile {
    #[serde(default)]
    pub fabric: HashMap<String, CuratedPackRef>,
    #[serde(default)]
    pub quilt: HashMap<String, CuratedPackRef>,
    #[serde(default)]
    pub neoforge: HashMap<String, CuratedPackRef>,
    #[serde(default)]
    pub forge: HashMap<String, CuratedPackRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OptimizeModEntry {
    pub name: String,
    #[serde(default)]
    pub modrinth_slug: Option<String>,
    #[serde(default)]
    pub curseforge_slug: Option<String>,
    pub reason: String,
    #[serde(default = "default_risk_low")]
    pub risk: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub loaders: Vec<String>,
}

fn default_risk_low() -> String {
    "low".into()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OptimizeModsFile {
    /// Schema marker of the embedded `optimize-mods.json`; parsed to keep the
    /// data shape self-describing but not consumed by any migration yet.
    #[serde(default)]
    #[allow(dead_code)]
    schema_version: u32,
    mods: HashMap<String, OptimizeModEntry>,
    profiles: HashMap<String, HashMap<String, Vec<String>>>,
}

/// Resolved mod candidate for custom optimize (slug + metadata).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OptimizeModCandidate {
    pub slug: String,
    pub name: String,
    pub reason: String,
    pub risk: String,
    pub category: String,
    pub modrinth_slug: Option<String>,
    pub curseforge_slug: Option<String>,
}

pub fn load_curated_packs() -> CuratedPacksFile {
    serde_json::from_str(CURATED_PACKS_JSON).unwrap_or(CuratedPacksFile {
        fabric: HashMap::new(),
        quilt: HashMap::new(),
        neoforge: HashMap::new(),
        forge: HashMap::new(),
    })
}

/// Optional env override: path to JSON with the same shape as `optimize-packs.json`.
pub fn load_curated_packs_with_override() -> CuratedPacksFile {
    if let Ok(path) = std::env::var("TUFFBOX_OPT_PACKS") {
        if let Ok(raw) = std::fs::read_to_string(&path) {
            if let Ok(parsed) = serde_json::from_str(&raw) {
                return parsed;
            }
        }
    }
    load_curated_packs()
}

/// Placeholder rows in `optimize-packs.json` (not published on Modrinth yet).
pub fn is_unpublished_curated_stub(pack: &CuratedPackRef) -> bool {
    let id = pack.project_id.trim();
    let slug = pack.slug.as_deref().unwrap_or("").trim();
    id.starts_with("tuffbox-opt-") || slug.starts_with("tuffbox-opt-")
}

fn loader_curated_map<'a>(
    file: &'a CuratedPacksFile,
    loader: &str,
) -> Option<&'a HashMap<String, CuratedPackRef>> {
    match loader {
        "fabric" => Some(&file.fabric),
        "quilt" => Some(&file.fabric),
        "neoforge" => Some(&file.neoforge),
        "forge" => Some(&file.forge),
        _ => None,
    }
}

/// Parse `1.20.1` style versions for ordering within the same minor series.
fn parse_mc_version_parts(v: &str) -> Option<(u32, u32, u32)> {
    let mut it = v.split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next()?.parse().ok()?;
    let patch = it.next().map(|p| p.parse().ok()).flatten().unwrap_or(0);
    Some((major, minor, patch))
}

fn cmp_mc_version(a: &str, b: &str) -> std::cmp::Ordering {
    match (parse_mc_version_parts(a), parse_mc_version_parts(b)) {
        (Some(va), Some(vb)) => va.cmp(&vb),
        _ => a.cmp(b),
    }
}

/// Resolve a version key from a profile map: exact → same minor (closest patch) → `default`.
pub fn resolve_version_profile_key(
    profiles: &HashMap<String, Vec<String>>,
    mc_version: &str,
) -> Option<String> {
    if profiles.contains_key(mc_version) {
        return Some(mc_version.to_string());
    }
    let target = parse_mc_version_parts(mc_version)?;
    let mut same_minor: Vec<String> = profiles
        .keys()
        .filter(|k| *k != "default")
        .filter(|k| {
            parse_mc_version_parts(k)
                .map(|p| p.0 == target.0 && p.1 == target.1)
                .unwrap_or(false)
        })
        .cloned()
        .collect();
    same_minor.sort_by(|a, b| cmp_mc_version(a, b));
    if let Some(best) = same_minor
        .iter()
        .filter(|k| cmp_mc_version(k, mc_version) != std::cmp::Ordering::Greater)
        .max_by(|a, b| cmp_mc_version(a, b))
        .or_else(|| same_minor.last())
    {
        return Some((*best).clone());
    }
    profiles
        .contains_key("default")
        .then(|| "default".to_string())
}

fn resolve_curated_pack(
    map: &HashMap<String, CuratedPackRef>,
    mc_version: &str,
) -> Option<CuratedPackRef> {
    if let Some(p) = map
        .get(mc_version)
        .filter(|p| !is_unpublished_curated_stub(p))
    {
        return Some(p.clone());
    }
    let keys_map: HashMap<String, Vec<String>> =
        map.keys().map(|k| (k.clone(), Vec::new())).collect();
    if let Some(key) = resolve_version_profile_key(&keys_map, mc_version) {
        if let Some(p) = map.get(&key).filter(|p| !is_unpublished_curated_stub(p)) {
            return Some(p.clone());
        }
    }
    map.get("default")
        .cloned()
        .filter(|p| !is_unpublished_curated_stub(p))
}

fn load_optimize_mods() -> OptimizeModsFile {
    serde_json::from_str(OPTIMIZE_MODS_JSON).unwrap_or(OptimizeModsFile {
        schema_version: 1,
        mods: HashMap::new(),
        profiles: HashMap::new(),
    })
}

fn load_optimize_mods_with_override() -> OptimizeModsFile {
    if let Ok(path) = std::env::var("TUFFBOX_OPT_MODS") {
        if let Ok(raw) = std::fs::read_to_string(&path) {
            if let Ok(parsed) = serde_json::from_str(&raw) {
                return parsed;
            }
        }
    }
    load_optimize_mods()
}

/// Aliases used to detect already-installed optimization mods in the manifest.
pub fn recommendation_aliases(slug: &str) -> Vec<&'static str> {
    match slug {
        "ferrite-core" | "ferritecore" => vec!["ferrite-core", "ferritecore"],
        "entityculling" | "entity-culling" => vec!["entityculling", "entity-culling"],
        "embeddium" => vec!["embeddium", "rubidium", "sodium", "magnesium"],
        "rubidium" => vec!["rubidium", "embeddium", "sodium", "magnesium"],
        "sodium" => vec!["sodium", "embeddium", "rubidium", "magnesium"],
        "sodium-extra" => vec!["sodium-extra"],
        "iris" => vec!["iris", "oculus"],
        "oculus" => vec!["oculus", "iris"],
        "modernfix" => vec!["modernfix", "modernfix-mvus"],
        "lithium" => vec!["lithium", "radium", "canary"],
        "radium" => vec!["radium", "lithium", "canary"],
        "canary" => vec!["canary", "radium", "lithium"],
        "c2me-fabric" | "c2me" => vec!["c2me-fabric", "c2me", "c2me-opts"],
        "bobby" => vec!["bobby"],
        "starlight" => vec!["starlight"],
        "reeses-sodium-options" => vec!["reeses-sodium-options", "reeses-sodium-options-forge"],
        "moreculling" => vec!["moreculling", "more-culling"],
        "dynamic-fps" => vec!["dynamic-fps"],
        "immediatelyfast" => vec!["immediatelyfast"],
        "nvidium" => vec!["nvidium"],
        "krypton" => vec!["krypton"],
        "lazydfu" => vec!["lazydfu"],
        "indium" => vec!["indium"],
        _ => Vec::new(),
    }
}

pub fn aliases_for_candidate(slug: &str) -> Vec<String> {
    let mut aliases: Vec<String> = recommendation_aliases(slug)
        .into_iter()
        .map(str::to_string)
        .collect();
    if !aliases.iter().any(|a| a == slug) {
        aliases.insert(0, slug.to_string());
    }
    aliases
}

fn mod_entry_supports_loader(entry: &OptimizeModEntry, loader: &str) -> bool {
    entry.loaders.is_empty() || entry.loaders.iter().any(|l| l == loader)
}

/// Version- and loader-aware optimization mod list (Fabulously Optimized–inspired).
pub fn optimization_candidates(loader: &str, mc_version: &str) -> Vec<OptimizeModCandidate> {
    let file = load_optimize_mods_with_override();
    let loader_profiles = file.profiles.get(loader).or_else(|| {
        if loader == "quilt" {
            file.profiles.get("fabric")
        } else {
            None
        }
    });
    let Some(loader_profiles) = loader_profiles else {
        return Vec::new();
    };
    let profile_key =
        resolve_version_profile_key(loader_profiles, mc_version).unwrap_or_else(|| {
            if loader_profiles.contains_key("default") {
                "default".to_string()
            } else {
                mc_version.to_string()
            }
        });
    let mod_ids = loader_profiles
        .get(&profile_key)
        .or_else(|| loader_profiles.get("default"))
        .cloned()
        .unwrap_or_default();

    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for id in mod_ids {
        let Some(entry) = file.mods.get(&id) else {
            continue;
        };
        if !mod_entry_supports_loader(entry, loader) {
            continue;
        }
        let slug = entry
            .modrinth_slug
            .as_deref()
            .unwrap_or(id.as_str())
            .to_string();
        if !seen.insert(slug.clone()) {
            continue;
        }
        out.push(OptimizeModCandidate {
            slug,
            name: entry.name.clone(),
            reason: entry.reason.clone(),
            risk: entry.risk.clone(),
            category: entry.category.clone(),
            modrinth_slug: entry.modrinth_slug.clone(),
            curseforge_slug: entry.curseforge_slug.clone(),
        });
    }
    out
}

pub fn curated_pack_for(loader: &str, mc_version: &str) -> Option<CuratedPackRef> {
    let file = load_curated_packs_with_override();
    if loader == "quilt" {
        if let Some(map) = loader_curated_map(&file, "quilt") {
            if let Some(p) = resolve_curated_pack(map, mc_version) {
                return Some(p);
            }
        }
    }
    let map = loader_curated_map(&file, loader)?;
    resolve_curated_pack(map, mc_version)
}

pub fn list_curated_pack_entries(loader: &str) -> Vec<(String, CuratedPackRef)> {
    let file = load_curated_packs_with_override();
    let map = match loader {
        "fabric" => file.fabric,
        "quilt" => {
            let mut m = file.fabric;
            for (k, v) in file.quilt {
                m.insert(k, v);
            }
            m
        }
        "neoforge" => file.neoforge,
        "forge" => file.forge,
        _ => HashMap::new(),
    };
    let mut out: Vec<_> = map
        .into_iter()
        .filter(|(_, p)| !is_unpublished_curated_stub(p))
        .collect();
    out.sort_by(|a, b| cmp_mc_version(&a.0, &b.0));
    out
}

/// Mods that make aggressive ModernFix options unsafe.
pub fn modernfix_denylist_hit(mod_ids_and_names: &[String]) -> Vec<String> {
    const DENY: &[&str] = &[
        "optifine",
        "optifabric",
        "connector",
        "sinytra-connector",
        "forgified-fabric-api",
        "rubidium", // prefer embeddium path; mixed sodium forks + modernfix can be fragile
    ];
    let mut hits = Vec::new();
    for raw in mod_ids_and_names {
        let c: String = raw
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric())
            .flat_map(|ch| ch.to_lowercase())
            .collect();
        for d in DENY {
            let dc: String = d
                .chars()
                .filter(|ch| ch.is_ascii_alphanumeric())
                .flat_map(|ch| ch.to_lowercase())
                .collect();
            if c == dc || c.contains(&dc) || dc.contains(&c) && c.len() >= 6 {
                hits.push(raw.clone());
                break;
            }
        }
    }
    hits.sort();
    hits.dedup();
    hits
}

pub fn inventory_tokens(manifest: &ProjectManifest) -> Vec<String> {
    let mut out = Vec::new();
    for m in &manifest.mods {
        out.push(m.id.clone());
        out.push(m.name.clone());
        if let Some(pid) = &m.source.project_id {
            out.push(pid.clone());
        }
        if let Some(f) = &m.file_name {
            out.push(f.clone());
        }
    }
    out
}

fn edit_action(
    path: &str,
    patch_type: &str,
    patch: Value,
    reason: &str,
    risk: &str,
) -> LauncherAction {
    LauncherAction {
        op: "edit_config".into(),
        mod_id: None,
        provider: None,
        project_id: None,
        version: None,
        path: Some(path.into()),
        patch_type: Some(patch_type.into()),
        patch: Some(patch),
        reason: Some(reason.into()),
        risk: risk.into(),
    }
}

/// Deterministic safe config patches. Prefer patching existing files.
pub fn build_optimize_config_actions(
    project_dir: &Path,
    manifest: &ProjectManifest,
    allow_modernfix_aggressive: bool,
) -> (Vec<LauncherAction>, Vec<String>) {
    let mut actions = Vec::new();
    let mut warnings = Vec::new();
    let tokens = inventory_tokens(manifest);
    let deny_hits = modernfix_denylist_hit(&tokens);
    if !deny_hits.is_empty() {
        warnings.push(format!(
            "ModernFix aggressive options skipped — possible conflicts: {}",
            deny_hits.join(", ")
        ));
    }
    let modernfix_safe_only = !allow_modernfix_aggressive || !deny_hits.is_empty();

    // options.txt
    let options_path = project_dir.join("options.txt");
    if options_path.is_file() {
        if let Ok(raw) = std::fs::read_to_string(&options_path) {
            let mut patch = serde_json::Map::new();
            if let Some(rd) = prop_get(&raw, "renderDistance") {
                if let Ok(n) = rd.parse::<i32>() {
                    if n > 16 {
                        patch.insert("renderDistance".into(), json!("12"));
                    }
                }
            }
            if let Some(sd) = prop_get(&raw, "simulationDistance") {
                if let Ok(n) = sd.parse::<i32>() {
                    if n > 12 {
                        patch.insert("simulationDistance".into(), json!("8"));
                    }
                }
            }
            if prop_get(&raw, "maxFps").is_none() {
                patch.insert("maxFps".into(), json!("120"));
            }
            if !patch.is_empty() {
                actions.push(edit_action(
                    "options.txt",
                    "properties_set",
                    Value::Object(patch),
                    "Cap render/simulation distance and set a sane maxFps for modded clients",
                    "low",
                ));
            }
        }
    }

    // Sodium
    for name in ["sodium-options.json", "sodium-extra-options.json"] {
        let rel = format!("config/{name}");
        let fp = project_dir.join("config").join(name);
        if !fp.is_file() {
            continue;
        }
        if name.starts_with("sodium-options") {
            actions.push(edit_action(
                &rel,
                "json_merge",
                json!({
                    "performance": {
                        "enable_memory_tracing": false,
                        "always_defer_chunk_updates_v0_shouldbedeleted": false
                    }
                }),
                "Disable Sodium memory tracing / keep chunk updates efficient",
                "low",
            ));
        } else {
            actions.push(edit_action(
                &rel,
                "json_merge",
                json!({
                    "extra_settings": {
                        "reduce_fps_when_afk": true
                    }
                }),
                "Enable mild Sodium Extra AFK FPS reduction when available",
                "low",
            ));
        }
    }

    // C2ME — only touch if config exists
    for name in ["c2me.toml", "c2me.json", "c2me-opts.toml"] {
        let fp = project_dir.join("config").join(name);
        if !fp.is_file() {
            continue;
        }
        let rel = format!("config/{name}");
        if name.ends_with(".toml") {
            actions.push(edit_action(
                &rel,
                "toml_set",
                json!({
                    "threadedWorldGen.enabled": true
                }),
                "Enable moderate C2ME threaded worldgen when present",
                "medium",
            ));
        } else {
            actions.push(edit_action(
                &rel,
                "json_merge",
                json!({ "threadedWorldGen": { "enabled": true } }),
                "Enable moderate C2ME threaded worldgen when present",
                "medium",
            ));
        }
        break;
    }

    // Bobby
    for name in ["bobby.toml", "bobby.json"] {
        let fp = project_dir.join("config").join(name);
        if !fp.is_file() {
            continue;
        }
        let rel = format!("config/{name}");
        if name.ends_with(".toml") {
            actions.push(edit_action(
                &rel,
                "toml_set",
                json!({
                    "maxRenderDistance": 12
                }),
                "Keep Bobby cache render distance moderate",
                "low",
            ));
        } else {
            actions.push(edit_action(
                &rel,
                "json_merge",
                json!({ "maxRenderDistance": 12 }),
                "Keep Bobby cache render distance moderate",
                "low",
            ));
        }
        break;
    }

    // ModernFix — safe subset only when denylist hits
    let mf_names = [
        "modernfix-common.toml",
        "modernfix.toml",
        "modernfix-fabric.toml",
        "modernfix-neoforge.toml",
    ];
    for name in mf_names {
        let fp = project_dir.join("config").join(name);
        if !fp.is_file() {
            continue;
        }
        let rel = format!("config/{name}");
        if modernfix_safe_only {
            actions.push(edit_action(
                &rel,
                "toml_set",
                json!({
                    "mixin.perf.dynamic_resources": false,
                    "mixin.perf.faster_item_rendering": true
                }),
                "ModernFix safe subset only (aggressive options disabled due to pack conflicts)",
                "low",
            ));
        } else {
            actions.push(edit_action(
                &rel,
                "toml_set",
                json!({
                    "mixin.perf.dynamic_resources": true,
                    "mixin.perf.faster_item_rendering": true,
                    "mixin.perf.cache_strongholds": true
                }),
                "Enable common ModernFix performance mixins",
                "medium",
            ));
        }
        break;
    }

    (actions, warnings)
}

fn prop_get(raw: &str, key: &str) -> Option<String> {
    for line in raw.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            if k.trim() == key {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

pub fn config_actions_to_plan(actions: Vec<LauncherAction>, explanation: &str) -> ActionPlan {
    ActionPlan {
        schema_version: ACTION_PLAN_SCHEMA_VERSION,
        human_explanation: explanation.into(),
        confidence: 0.85,
        suspected_mods: Vec::new(),
        needs_user_review: true,
        source: Some("optimize_pack".into()),
        matched_case_ids: Vec::new(),
        actions,
        additional_context: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn denylist_detects_optifine() {
        let hits = modernfix_denylist_hit(&["OptiFine 1.20.1".into()]);
        assert!(!hits.is_empty());
    }

    #[test]
    fn curated_json_parses() {
        let _ = load_curated_packs();
    }

    #[test]
    fn stub_curated_packs_are_unavailable() {
        let stub = CuratedPackRef {
            project_id: "tuffbox-opt-1-20-1".into(),
            slug: Some("tuffbox-opt-1-20-1".into()),
            name: Some("stub".into()),
        };
        assert!(is_unpublished_curated_stub(&stub));
    }

    #[test]
    fn fabulously_optimized_curated_for_fabric_1211() {
        let pack = curated_pack_for("fabric", "1.21.1").expect("FO curated");
        assert_eq!(pack.slug.as_deref(), Some("fabulously-optimized"));
    }

    #[test]
    fn optimization_candidates_fabric_1211_includes_sodium() {
        let mods = optimization_candidates("fabric", "1.21.1");
        assert!(mods.iter().any(|m| m.slug == "sodium"));
        assert!(mods.iter().any(|m| m.slug == "lithium"));
        assert!(!mods.iter().any(|m| m.slug == "embeddium"));
    }

    #[test]
    fn optimization_candidates_neoforge_uses_embeddium() {
        let mods = optimization_candidates("neoforge", "1.21.1");
        assert!(mods.iter().any(|m| m.slug == "embeddium"));
        assert!(!mods.iter().any(|m| m.slug == "sodium"));
    }

    #[test]
    fn version_profile_falls_back_to_minor_series() {
        let file = load_optimize_mods();
        let fabric = file.profiles.get("fabric").unwrap();
        let key = resolve_version_profile_key(fabric, "1.21.3").unwrap();
        assert!(key == "1.21.1" || key == "1.21.4" || key == "default");
    }

    #[test]
    fn optimize_mods_json_parses() {
        let file = load_optimize_mods();
        assert!(!file.mods.is_empty());
        assert!(file.profiles.contains_key("fabric"));
    }
}
