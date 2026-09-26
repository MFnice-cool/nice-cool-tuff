use super::*;

pub struct CreateOverride;

impl ModOverride for CreateOverride {
    fn mod_id(&self) -> &str {
        "create"
    }

    fn config_locations(&self) -> Vec<ModConfigLocation> {
        vec![
            ModConfigLocation {
                path: "config/create-common.toml".to_string(),
                format: ConfigFileFormat::Toml,
                description: "Create common settings".to_string(),
            },
            ModConfigLocation {
                path: "config/create-client.toml".to_string(),
                format: ConfigFileFormat::Toml,
                description: "Create client settings".to_string(),
            },
        ]
    }

    fn ore_gen_config_keys(&self) -> Vec<OreConfigMapping> {
        vec![OreConfigMapping {
            resource_name: "zinc".to_string(),
            config_file: "config/create-common.toml".to_string(),
            enabled_key: "disableZincOre".to_string(),
            vein_size_key: None,
            min_height_key: None,
            max_height_key: None,
            enabled_inverted: true,
        }]
    }

    fn programmatic_items(&self) -> Vec<String> {
        vec![
            "create:crushed_raw_iron".to_string(),
            "create:crushed_raw_gold".to_string(),
            "create:crushed_raw_copper".to_string(),
            "create:crushed_raw_zinc".to_string(),
            "create:crushed_raw_tin".to_string(),
            "create:crushed_raw_silver".to_string(),
            "create:crushed_raw_lead".to_string(),
            "create:crushed_raw_aluminum".to_string(),
            "create:crushed_raw_nickel".to_string(),
            "create:crushed_raw_uranium".to_string(),
            "create:crushed_raw_osmium".to_string(),
        ]
    }

    fn programmatic_recipes(&self) -> Vec<UnifiedRecipe> {
        vec![]
    }
}
