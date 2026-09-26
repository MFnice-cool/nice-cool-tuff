use super::*;

pub struct MekanismOverride;

impl ModOverride for MekanismOverride {
    fn mod_id(&self) -> &str {
        "mekanism"
    }

    fn config_locations(&self) -> Vec<ModConfigLocation> {
        vec![
            ModConfigLocation {
                path: "config/mekanism/general.toml".to_string(),
                format: ConfigFileFormat::Toml,
                description: "Mekanism general settings".to_string(),
            },
            ModConfigLocation {
                path: "config/mekanism/world.toml".to_string(),
                format: ConfigFileFormat::Toml,
                description: "Ore generation".to_string(),
            },
            ModConfigLocation {
                path: "config/mekanism/gear.toml".to_string(),
                format: ConfigFileFormat::Toml,
                description: "Equipment settings".to_string(),
            },
            ModConfigLocation {
                path: "config/mekanism/storage.toml".to_string(),
                format: ConfigFileFormat::Toml,
                description: "Energy storage".to_string(),
            },
            ModConfigLocation {
                path: "config/mekanism/usage.toml".to_string(),
                format: ConfigFileFormat::Toml,
                description: "Machine energy usage".to_string(),
            },
        ]
    }

    fn ore_gen_config_keys(&self) -> Vec<OreConfigMapping> {
        vec![
            OreConfigMapping {
                resource_name: "tin".to_string(),
                config_file: "config/mekanism/world.toml".to_string(),
                enabled_key: "tin.shouldGenerate".to_string(),
                vein_size_key: Some("tin.perChunk".to_string()),
                min_height_key: Some("tin.bottomOffset".to_string()),
                max_height_key: Some("tin.topOffset".to_string()),
                enabled_inverted: false,
            },
            OreConfigMapping {
                resource_name: "osmium".to_string(),
                config_file: "config/mekanism/world.toml".to_string(),
                enabled_key: "osmium.shouldGenerate".to_string(),
                vein_size_key: Some("osmium.perChunk".to_string()),
                min_height_key: Some("osmium.bottomOffset".to_string()),
                max_height_key: Some("osmium.topOffset".to_string()),
                enabled_inverted: false,
            },
            OreConfigMapping {
                resource_name: "lead".to_string(),
                config_file: "config/mekanism/world.toml".to_string(),
                enabled_key: "lead.shouldGenerate".to_string(),
                vein_size_key: Some("lead.perChunk".to_string()),
                min_height_key: None,
                max_height_key: None,
                enabled_inverted: false,
            },
            OreConfigMapping {
                resource_name: "uranium".to_string(),
                config_file: "config/mekanism/world.toml".to_string(),
                enabled_key: "uranium.shouldGenerate".to_string(),
                vein_size_key: Some("uranium.perChunk".to_string()),
                min_height_key: None,
                max_height_key: None,
                enabled_inverted: false,
            },
            OreConfigMapping {
                resource_name: "fluorite".to_string(),
                config_file: "config/mekanism/world.toml".to_string(),
                enabled_key: "fluorite.shouldGenerate".to_string(),
                vein_size_key: None,
                min_height_key: None,
                max_height_key: None,
                enabled_inverted: false,
            },
        ]
    }

    fn programmatic_items(&self) -> Vec<String> {
        vec![]
    }
    fn programmatic_recipes(&self) -> Vec<UnifiedRecipe> {
        vec![]
    }
}
