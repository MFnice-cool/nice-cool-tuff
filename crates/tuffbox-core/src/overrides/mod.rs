pub mod create;
pub mod mekanism;
pub mod thermal;

use crate::adapters::ConfigFileFormat;
use crate::unified::UnifiedRecipe;

pub use create::CreateOverride;
pub use mekanism::MekanismOverride;
pub use thermal::ThermalOverride;

pub trait ModOverride: Send + Sync {
    fn mod_id(&self) -> &str;
    fn config_locations(&self) -> Vec<ModConfigLocation>;
    fn ore_gen_config_keys(&self) -> Vec<OreConfigMapping>;
    fn programmatic_items(&self) -> Vec<String>;
    fn programmatic_recipes(&self) -> Vec<UnifiedRecipe>;
}

#[derive(Debug, Clone)]
pub struct ModConfigLocation {
    pub path: String,
    pub format: ConfigFileFormat,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct OreConfigMapping {
    pub resource_name: String,
    pub config_file: String,
    pub enabled_key: String,
    pub vein_size_key: Option<String>,
    pub min_height_key: Option<String>,
    pub max_height_key: Option<String>,
    /// When `true`, the enabled_key is a *disable* flag (`true` = ore is OFF).
    /// The scanner inverts the value before passing it to the frontend.
    #[allow(dead_code)]
    pub enabled_inverted: bool,
}
