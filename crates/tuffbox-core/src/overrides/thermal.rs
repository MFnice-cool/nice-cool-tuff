use super::*;

pub struct ThermalOverride;

impl ModOverride for ThermalOverride {
    fn mod_id(&self) -> &str {
        "thermal"
    }

    fn config_locations(&self) -> Vec<ModConfigLocation> {
        vec![ModConfigLocation {
            path: "config/thermal/thermal-core.toml".to_string(),
            format: ConfigFileFormat::Toml,
            description: "Thermal main settings".to_string(),
        }]
    }

    fn ore_gen_config_keys(&self) -> Vec<OreConfigMapping> {
        vec![]
    }

    fn programmatic_items(&self) -> Vec<String> {
        vec![]
    }
    fn programmatic_recipes(&self) -> Vec<UnifiedRecipe> {
        vec![]
    }
}
