//! Built-in knowledge base for ~20 popular Minecraft mods.
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModKnowledgeEntry {
    pub slug: String,
    pub name: String,
    pub config_paths: Vec<String>,
    pub ore_keys: Vec<String>,
    pub programmatic_items: Vec<String>,
    pub known_conflicts: Vec<String>,
    pub loaders: Vec<String>,
    pub category: String,
}
use ModKnowledgeEntry as E;
lazy_static::lazy_static! {
    static ref BUILTIN: Vec<ModKnowledgeEntry> = vec![
        E { slug:"sodium".into(), name:"Sodium".into(), config_paths:vec!["sodium-options.json".into()], ore_keys:vec![], programmatic_items:vec![], known_conflicts:vec!["optifine".into(),"vulkanmod".into(),"canvas".into()], loaders:vec!["fabric".into(),"neoforge".into()], category:"optimization".into() },
        E { slug:"iris".into(), name:"Iris".into(), config_paths:vec!["iris.properties".into()], ore_keys:vec![], programmatic_items:vec![], known_conflicts:vec!["optifine".into(),"canvas".into()], loaders:vec!["fabric".into(),"neoforge".into()], category:"shader".into() },
        E { slug:"create".into(), name:"Create".into(), config_paths:vec!["create-common.toml".into(),"create-server.toml".into()], ore_keys:vec!["generateZincOre".into()], programmatic_items:vec![], known_conflicts:vec![], loaders:vec!["forge".into(),"fabric".into()], category:"technology".into() },
        E { slug:"mekanism".into(), name:"Mekanism".into(), config_paths:vec!["mekanism/general.toml".into(),"mekanism/world.toml".into()], ore_keys:vec!["enableCopperOre".into(),"enableTinOre".into(),"enableOsmiumOre".into()], programmatic_items:vec![], known_conflicts:vec![], loaders:vec!["forge".into(),"neoforge".into()], category:"technology".into() },
        E { slug:"thermal-expansion".into(), name:"Thermal Expansion".into(), config_paths:vec!["cofh/thermal-common.toml".into(),"cofh/thermal-worldgen.toml".into()], ore_keys:vec!["EnableCopperGeneration".into(),"EnableTinGeneration".into(),"EnableLeadGeneration".into()], programmatic_items:vec![], known_conflicts:vec![], loaders:vec!["forge".into()], category:"technology".into() },
        E { slug:"immersive-engineering".into(), name:"Immersive Engineering".into(), config_paths:vec!["immersiveengineering-common.toml".into()], ore_keys:vec!["generateBauxite".into(),"generateLead".into(),"generateSilver".into()], programmatic_items:vec![], known_conflicts:vec![], loaders:vec!["forge".into(),"neoforge".into()], category:"technology".into() },
        E { slug:"jei".into(), name:"JEI".into(), config_paths:vec!["jei/jei-client.toml".into(),"jei/jei-common.toml".into()], ore_keys:vec![], programmatic_items:vec![], known_conflicts:vec![], loaders:vec!["forge".into(),"fabric".into(),"neoforge".into()], category:"utility".into() },
        E { slug:"kubejs".into(), name:"KubeJS".into(), config_paths:vec!["kubejs/config.json".into(),"kubejs/common.properties".into()], ore_keys:vec![], programmatic_items:vec![], known_conflicts:vec![], loaders:vec!["forge".into(),"fabric".into(),"neoforge".into()], category:"library".into() },
        E { slug:"terrablender".into(), name:"TerraBlender".into(), config_paths:vec!["terrablender-common.toml".into()], ore_keys:vec![], programmatic_items:vec![], known_conflicts:vec![], loaders:vec!["forge".into(),"fabric".into(),"neoforge".into()], category:"worldgen".into() },
        E { slug:"biomes-o-plenty".into(), name:"Biomes O' Plenty".into(), config_paths:vec!["biomesoplenty/generation.toml".into()], ore_keys:vec![], programmatic_items:vec![], known_conflicts:vec![], loaders:vec!["forge".into(),"fabric".into(),"neoforge".into()], category:"worldgen".into() },
        E { slug:"apotheosis".into(), name:"Apotheosis".into(), config_paths:vec!["apotheosis/adventure.cfg".into(),"apotheosis/enchanting.cfg".into()], ore_keys:vec![], programmatic_items:vec![], known_conflicts:vec![], loaders:vec!["forge".into(),"neoforge".into()], category:"game-mechanics".into() },
        E { slug:"botania".into(), name:"Botania".into(), config_paths:vec!["botania-common.toml".into()], ore_keys:vec!["generateMysticalFlowers".into(),"spreadMysticalFlowers".into()], programmatic_items:vec![], known_conflicts:vec![], loaders:vec!["forge".into(),"fabric".into(),"neoforge".into()], category:"magic".into() },
        E { slug:"quark".into(), name:"Quark".into(), config_paths:vec!["quark-common.toml".into()], ore_keys:vec![], programmatic_items:vec![], known_conflicts:vec![], loaders:vec!["forge".into()], category:"decoration".into() },
        E { slug:"xaeros-minimap".into(), name:"Xaero's Minimap".into(), config_paths:vec!["xaerominimap.txt".into()], ore_keys:vec![], programmatic_items:vec![], known_conflicts:vec!["journeymap".into()], loaders:vec!["forge".into(),"fabric".into(),"neoforge".into()], category:"utility".into() },
        E { slug:"journeymap".into(), name:"JourneyMap".into(), config_paths:vec!["journeymap/journeymap.toml".into(),"journeymap/minimap.toml".into()], ore_keys:vec![], programmatic_items:vec![], known_conflicts:vec!["xaeros-minimap".into()], loaders:vec!["forge".into(),"fabric".into(),"neoforge".into()], category:"utility".into() },
        E { slug:"farmers-delight".into(), name:"Farmer's Delight".into(), config_paths:vec!["farmersdelight-common.toml".into()], ore_keys:vec!["generateWildCabbages".into(),"generateWildTomatoes".into(),"generateWildOnions".into(),"generateRichSoil".into()], programmatic_items:vec![], known_conflicts:vec![], loaders:vec!["forge".into(),"fabric".into(),"neoforge".into()], category:"food".into() },
        E { slug:"twilightforest".into(), name:"Twilight Forest".into(), config_paths:vec!["twilightforest-common.toml".into()], ore_keys:vec![], programmatic_items:vec![], known_conflicts:vec![], loaders:vec!["forge".into(),"neoforge".into()], category:"adventure".into() },
        E { slug:"tconstruct".into(), name:"Tinkers' Construct".into(), config_paths:vec!["tconstruct-common.toml".into(),"tconstruct-worldgen.toml".into()], ore_keys:vec!["generateCobalt".into(),"generateCopper".into()], programmatic_items:vec![], known_conflicts:vec![], loaders:vec!["forge".into()], category:"technology".into() },
        E { slug:"applied-energistics-2".into(), name:"Applied Energistics 2".into(), config_paths:vec!["appliedenergistics2/common.toml".into()], ore_keys:vec!["generateCertusQuartz".into(),"generateChargedCertusQuartz".into()], programmatic_items:vec![], known_conflicts:vec!["refined-storage".into()], loaders:vec!["forge".into(),"neoforge".into()], category:"technology".into() },
        E { slug:"refined-storage".into(), name:"Refined Storage".into(), config_paths:vec!["refinedstorage-server.toml".into()], ore_keys:vec!["generateQuartz".into()], programmatic_items:vec![], known_conflicts:vec!["applied-energistics-2".into()], loaders:vec!["forge".into(),"neoforge".into()], category:"technology".into() },
    ];
}
impl ModKnowledgeEntry {
    pub fn builtin() -> &'static [ModKnowledgeEntry] {
        &*BUILTIN
    }
    pub fn lookup(slug: &str) -> Option<&'static ModKnowledgeEntry> {
        let l = slug.to_lowercase();
        Self::builtin().iter().find(|e| e.slug.to_lowercase() == l)
    }
}
pub fn check_known_conflict(slug_a: &str, slug_b: &str) -> Option<String> {
    let a = ModKnowledgeEntry::lookup(slug_a)?;
    if a.known_conflicts
        .iter()
        .any(|c| c.eq_ignore_ascii_case(slug_b))
    {
        return Some(format!("{} conflicts with {} (known)", a.name, slug_b));
    }
    let b = ModKnowledgeEntry::lookup(slug_b)?;
    if b.known_conflicts
        .iter()
        .any(|c| c.eq_ignore_ascii_case(slug_a))
    {
        return Some(format!("{} conflicts with {} (known)", slug_a, b.name));
    }
    None
}
