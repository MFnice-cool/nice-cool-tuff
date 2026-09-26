use super::*;
use crate::environment::LoaderKind;
use crate::unified::recipe::*;
use crate::unified::tag::tag_id_from_path;

pub struct FabricAdapter;

impl LoaderAdapter for FabricAdapter {
    fn supported_loaders(&self) -> &[LoaderKind] {
        &[LoaderKind::Fabric, LoaderKind::Quilt]
    }

    fn extract_metadata(
        &self,
        archive: &mut zip::ZipArchive<std::fs::File>,
    ) -> Result<ModMetadata, AdapterError> {
        if let Ok(mut entry) = archive.by_name("fabric.mod.json") {
            let mut content = String::new();
            entry.read_to_string(&mut content)?;
            return Self::parse_fabric_mod_json(&content);
        }

        if let Ok(mut entry) = archive.by_name("quilt.mod.json") {
            let mut content = String::new();
            entry.read_to_string(&mut content)?;
            return Self::parse_quilt_mod_json(&content);
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
                path_pattern: "config/*.json".to_string(),
                format: ConfigFileFormat::Json,
            },
            ConfigPattern {
                path_pattern: "config/*.json5".to_string(),
                format: ConfigFileFormat::Json5,
            },
            ConfigPattern {
                path_pattern: "config/*/*.json".to_string(),
                format: ConfigFileFormat::Json,
            },
            ConfigPattern {
                path_pattern: "config/*.toml".to_string(),
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
                        if let Some(obj) = v.as_object() {
                            let id = obj.get("id")?.as_str()?.to_string();
                            let required = obj
                                .get("required")
                                .and_then(|r| r.as_bool())
                                .unwrap_or(true);
                            Some(TagEntry { id, required })
                        } else if let Some(s) = v.as_str() {
                            Some(TagEntry {
                                id: s.to_string(),
                                required: true,
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

impl FabricAdapter {
    pub fn parse_fabric_mod_json(content: &str) -> Result<ModMetadata, AdapterError> {
        let json: serde_json::Value =
            serde_json::from_str(content).map_err(|e| AdapterError::Parse(e.to_string()))?;

        let mod_id = json
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let display_name = json
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(&mod_id)
            .to_string();
        let version = json
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("0.0.0")
            .to_string();

        let mut dependencies: Vec<ModDependency> = json
            .get("depends")
            .and_then(|v| v.as_object())
            .into_iter()
            .flat_map(|deps| deps.keys())
            .filter(|id| !matches!(id.as_str(), "minecraft" | "fabricloader" | "java"))
            .map(|id| ModDependency {
                mod_id: id.clone(),
                required: true,
            })
            .collect();
        for section in ["recommends", "suggests"] {
            dependencies.extend(
                json.get(section)
                    .and_then(|value| value.as_object())
                    .into_iter()
                    .flat_map(|deps| deps.keys())
                    .map(|id| ModDependency {
                        mod_id: id.clone(),
                        required: false,
                    }),
            );
        }

        Ok(ModMetadata {
            namespace: mod_id.clone(),
            mod_id,
            display_name,
            version,
            dependencies,
        })
    }

    pub fn parse_quilt_mod_json(content: &str) -> Result<ModMetadata, AdapterError> {
        let json: serde_json::Value =
            serde_json::from_str(content).map_err(|e| AdapterError::Parse(e.to_string()))?;

        let loader = json.get("quilt_loader").ok_or(AdapterError::NoMetadata)?;

        let mod_id = loader
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let metadata = loader.get("metadata");
        let display_name = metadata
            .and_then(|m| m.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or(&mod_id)
            .to_string();

        Ok(ModMetadata {
            namespace: mod_id.clone(),
            mod_id,
            display_name,
            version: loader
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("0.0.0")
                .to_string(),
            dependencies: Vec::new(),
        })
    }
}
