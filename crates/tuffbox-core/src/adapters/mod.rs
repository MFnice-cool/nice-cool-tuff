pub mod fabric;
pub mod forge;
pub mod neoforge;

use crate::environment::{LoaderKind, McVersion};
use crate::unified::{tag::TagEntry, UnifiedRecipe, UnifiedTag};
use std::io::Read;

pub use fabric::FabricAdapter;
pub use forge::ForgeAdapter;
pub use neoforge::NeoForgeAdapter;

#[derive(Debug, Clone)]
pub struct ModMetadata {
    pub mod_id: String,
    pub display_name: String,
    pub version: String,
    pub namespace: String,
    pub dependencies: Vec<ModDependency>,
}

#[derive(Debug, Clone)]
pub struct ModDependency {
    pub mod_id: String,
    pub required: bool,
}

pub trait LoaderAdapter: Send + Sync {
    fn supported_loaders(&self) -> &[LoaderKind];

    fn extract_metadata(
        &self,
        archive: &mut zip::ZipArchive<std::fs::File>,
    ) -> Result<ModMetadata, AdapterError>;

    fn recipe_paths(&self, archive: &zip::ZipArchive<std::fs::File>) -> Vec<String>;

    fn item_tag_paths(&self, archive: &zip::ZipArchive<std::fs::File>) -> Vec<String>;

    fn config_file_patterns(&self) -> Vec<ConfigPattern>;

    fn parse_recipe(
        &self,
        json: &serde_json::Value,
        file_path: &str,
        mc_version: &McVersion,
    ) -> Result<UnifiedRecipe, AdapterError>;

    fn parse_tag(
        &self,
        json: &serde_json::Value,
        file_path: &str,
    ) -> Result<UnifiedTag, AdapterError>;
}

#[derive(Debug, Clone)]
pub struct ConfigPattern {
    pub path_pattern: String,
    pub format: ConfigFileFormat,
}

#[derive(Debug, Clone, Copy)]
pub enum ConfigFileFormat {
    Toml,
    Json,
    Json5,
    Cfg,
    Properties,
    Snbt,
    Yaml,
}

#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    #[error("IO: {0}")]
    Io(#[from] std::io::Error),
    #[error("No mod metadata found")]
    NoMetadata,
    #[error("Parse error: {0}")]
    Parse(String),
    #[error("Invalid path: {0}")]
    InvalidPath(String),
    #[error("Unsupported recipe type: {0}")]
    UnsupportedRecipeType(String),
}
