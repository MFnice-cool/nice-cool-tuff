use super::*;
use crate::adapters::forge::ForgeAdapter;
use crate::environment::LoaderKind;
use crate::unified::recipe::*;
use crate::unified::tag::tag_id_from_path;

pub struct NeoForgeAdapter;

impl LoaderAdapter for NeoForgeAdapter {
    fn supported_loaders(&self) -> &[LoaderKind] {
        &[LoaderKind::Neoforge]
    }

    fn extract_metadata(
        &self,
        archive: &mut zip::ZipArchive<std::fs::File>,
    ) -> Result<ModMetadata, AdapterError> {
        if let Ok(mut entry) = archive.by_name("META-INF/neoforge.mods.toml") {
            let mut content = String::new();
            entry.read_to_string(&mut content)?;
            return ForgeAdapter::parse_mods_toml(&content);
        }

        if let Ok(mut entry) = archive.by_name("META-INF/mods.toml") {
            let mut content = String::new();
            entry.read_to_string(&mut content)?;
            return ForgeAdapter::parse_mods_toml(&content);
        }

        Err(AdapterError::NoMetadata)
    }

    fn recipe_paths(&self, archive: &zip::ZipArchive<std::fs::File>) -> Vec<String> {
        archive
            .file_names()
            .filter(|name| {
                let parts: Vec<&str> = name.split('/').collect();
                parts.len() >= 4
                    && parts[0] == "data"
                    && (parts[2] == "recipes" || parts[2] == "recipe")
                    && name.ends_with(".json")
            })
            .map(|s| s.to_string())
            .collect()
    }

    fn item_tag_paths(&self, archive: &zip::ZipArchive<std::fs::File>) -> Vec<String> {
        archive
            .file_names()
            .filter(|name| {
                let parts: Vec<&str> = name.split('/').collect();
                parts.len() >= 5
                    && parts[0] == "data"
                    && parts[2] == "tags"
                    && (parts[3] == "items" || parts[3] == "item")
                    && name.ends_with(".json")
            })
            .map(|s| s.to_string())
            .collect()
    }

    fn config_file_patterns(&self) -> Vec<ConfigPattern> {
        vec![
            ConfigPattern {
                path_pattern: "config/*.toml".to_string(),
                format: ConfigFileFormat::Toml,
            },
            ConfigPattern {
                path_pattern: "config/*/*.toml".to_string(),
                format: ConfigFileFormat::Toml,
            },
        ]
    }

    fn parse_recipe(
        &self,
        json: &serde_json::Value,
        file_path: &str,
        mc_version: &McVersion,
    ) -> Result<UnifiedRecipe, AdapterError> {
        let recipe_type = json
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        parser_for_type(recipe_type, mc_version)
            .parse(json, file_path, mc_version)
            .map_err(|e| AdapterError::Parse(e.to_string()))
    }

    fn parse_tag(
        &self,
        json: &serde_json::Value,
        file_path: &str,
    ) -> Result<UnifiedTag, AdapterError> {
        let replace = json
            .get("replace")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let values: Vec<TagEntry> = json
            .get("values")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| {
                        if let Some(s) = v.as_str() {
                            Some(TagEntry {
                                id: s.to_string(),
                                required: true,
                            })
                        } else if let Some(obj) = v.as_object() {
                            Some(TagEntry {
                                id: obj.get("id")?.as_str()?.to_string(),
                                required: obj
                                    .get("required")
                                    .and_then(|r| r.as_bool())
                                    .unwrap_or(true),
                            })
                        } else {
                            None
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        let tag_id = tag_id_from_path(file_path)
            .ok_or_else(|| AdapterError::InvalidPath(file_path.to_string()))?;

        Ok(UnifiedTag {
            id: tag_id,
            entries: values,
            replace,
        })
    }
}
