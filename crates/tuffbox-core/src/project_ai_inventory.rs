//! Project inventory snapshot for AI crash / pack analysis prompts.

use crate::content_packs::{list_content_packs, ContentPackEntry};
use crate::manifest::ProjectManifest;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProjectAiInventory {
    pub mods: Vec<InventoryMod>,
    pub resourcepacks: Vec<InventoryPack>,
    pub shaderpacks: Vec<InventoryPack>,
    pub datapacks: Vec<InventoryPack>,
    pub config_files: Vec<InventoryConfigFile>,
    pub kubejs_scripts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryMod {
    pub id: String,
    pub name: String,
    pub version: String,
    pub content_type: String,
    pub enabled: bool,
    pub side: String,
    pub file_name: Option<String>,
    #[serde(default)]
    pub authors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryPack {
    pub name: String,
    pub file_name: String,
    pub enabled: bool,
    pub kind: String,
    pub location: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryConfigFile {
    pub relative_path: String,
    pub size: u64,
}

/// Scan mods / packs / datapacks / config / kubejs for AI context.
pub fn collect_project_ai_inventory(
    project_dir: &Path,
    manifest: &ProjectManifest,
) -> ProjectAiInventory {
    let mut inv = ProjectAiInventory::default();

    for m in &manifest.mods {
        let enabled = !m.status.iter().any(|s| s.eq_ignore_ascii_case("disabled"));
        inv.mods.push(InventoryMod {
            id: m.id.clone(),
            name: m.name.clone(),
            version: m.version.clone(),
            content_type: format!("{:?}", m.content_type).to_lowercase(),
            enabled,
            side: format!("{:?}", m.side).to_lowercase(),
            file_name: m.file_name.clone().or_else(|| m.source.path.clone()),
            authors: m.authors.clone(),
        });
    }
    // Also list loose jars in mods/ not in manifest.
    let mods_dir = project_dir.join("mods");
    if mods_dir.is_dir() {
        if let Ok(rd) = fs::read_dir(&mods_dir) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                let lower = name.to_lowercase();
                if !(lower.ends_with(".jar") || lower.ends_with(".jar.disabled")) {
                    continue;
                }
                let already = inv.mods.iter().any(|m| {
                    m.file_name
                        .as_deref()
                        .map(|f| f.eq_ignore_ascii_case(&name))
                        .unwrap_or(false)
                        || name.to_lowercase().contains(&m.id.to_lowercase())
                });
                if already {
                    continue;
                }
                let enabled = !lower.ends_with(".disabled");
                let id = name
                    .trim_end_matches(".disabled")
                    .trim_end_matches(".jar")
                    .trim_end_matches(".JAR")
                    .to_string();
                let authors = crate::mod_scan::scan_mod_jar(&e.path())
                    .map(|r| r.authors)
                    .unwrap_or_default();
                inv.mods.push(InventoryMod {
                    id: id.clone(),
                    name: id,
                    version: String::new(),
                    content_type: "mod".into(),
                    enabled,
                    side: "unknown".into(),
                    file_name: Some(name),
                    authors,
                });
            }
        }
    }

    inv.resourcepacks = map_packs(
        list_content_packs(project_dir, "resourcepacks").unwrap_or_default(),
        "resourcepacks",
    );
    inv.shaderpacks = map_packs(
        list_content_packs(project_dir, "shaderpacks").unwrap_or_default(),
        "shaderpacks",
    );
    inv.datapacks = collect_datapacks(project_dir);
    inv.config_files = collect_config_files(project_dir);
    inv.kubejs_scripts = collect_kubejs_scripts(project_dir);
    inv
}

fn map_packs(entries: Vec<ContentPackEntry>, location: &str) -> Vec<InventoryPack> {
    entries
        .into_iter()
        .map(|e| InventoryPack {
            name: e.name,
            file_name: e.file_name,
            enabled: e.enabled,
            kind: e.kind,
            location: location.into(),
        })
        .collect()
}

fn collect_datapacks(project_dir: &Path) -> Vec<InventoryPack> {
    let mut out = Vec::new();
    let roots = [
        project_dir.join("datapacks"),
        project_dir.join("kubejs").join("data"),
    ];
    for root in roots {
        push_datapack_dir(&root, &mut out, &root);
    }
    let saves = project_dir.join("saves");
    if saves.is_dir() {
        if let Ok(rd) = fs::read_dir(&saves) {
            for world in rd.flatten() {
                let dp = world.path().join("datapacks");
                if dp.is_dir() {
                    let loc = format!("saves/{}/datapacks", world.file_name().to_string_lossy());
                    if let Ok(entries) = fs::read_dir(&dp) {
                        for e in entries.flatten() {
                            let path = e.path();
                            let file_name = e.file_name().to_string_lossy().to_string();
                            if file_name.starts_with('.') {
                                continue;
                            }
                            let enabled = !file_name.to_lowercase().ends_with(".disabled");
                            let kind = if path.is_dir() {
                                "folder"
                            } else if file_name.to_lowercase().ends_with(".zip") {
                                "zip"
                            } else {
                                continue;
                            };
                            out.push(InventoryPack {
                                name: file_name
                                    .trim_end_matches(".disabled")
                                    .trim_end_matches(".zip")
                                    .to_string(),
                                file_name,
                                enabled,
                                kind: kind.into(),
                                location: loc.clone(),
                            });
                        }
                    }
                }
            }
        }
    }
    let _ = roots;
    out
}

fn push_datapack_dir(dir: &Path, out: &mut Vec<InventoryPack>, root: &Path) {
    if !dir.is_dir() {
        return;
    }
    // Top-level packs only for instance datapacks/; for kubejs/data list namespaces.
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            let path = e.path();
            let file_name = e.file_name().to_string_lossy().to_string();
            if file_name.starts_with('.') {
                continue;
            }
            let rel = path
                .strip_prefix(root)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|_| file_name.clone());
            let enabled = !file_name.to_lowercase().ends_with(".disabled");
            let kind = if path.is_dir() {
                "folder"
            } else if file_name.to_lowercase().ends_with(".zip") {
                "zip"
            } else {
                continue;
            };
            let location = root
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "datapacks".into());
            out.push(InventoryPack {
                name: rel,
                file_name,
                enabled,
                kind: kind.into(),
                location,
            });
        }
    }
}

fn collect_config_files(project_dir: &Path) -> Vec<InventoryConfigFile> {
    let config = project_dir.join("config");
    let mut out = Vec::new();
    if !config.is_dir() {
        return out;
    }
    let mut stack = vec![config.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let path = e.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if !matches!(
                ext.as_str(),
                "toml" | "json" | "json5" | "cfg" | "properties" | "yml" | "yaml" | "txt" | "snbt"
            ) {
                continue;
            }
            let rel = path
                .strip_prefix(project_dir)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            let size = path.metadata().map(|m| m.len()).unwrap_or(0);
            out.push(InventoryConfigFile {
                relative_path: rel,
                size,
            });
            if out.len() >= 400 {
                return out;
            }
        }
    }
    out.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    out
}

fn collect_kubejs_scripts(project_dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for sub in ["server_scripts", "client_scripts", "startup_scripts"] {
        let dir = project_dir.join("kubejs").join(sub);
        if !dir.is_dir() {
            continue;
        }
        let mut stack = vec![dir];
        while let Some(d) = stack.pop() {
            let Ok(rd) = fs::read_dir(&d) else {
                continue;
            };
            for e in rd.flatten() {
                let path = e.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().and_then(|e| e.to_str()) == Some("js")
                    || path.extension().and_then(|e| e.to_str()) == Some("ts")
                {
                    if let Ok(rel) = path.strip_prefix(project_dir) {
                        out.push(rel.to_string_lossy().replace('\\', "/"));
                    }
                }
                if out.len() >= 200 {
                    return out;
                }
            }
        }
    }
    out.sort();
    out
}

/// Render inventory as markdown sections for the LLM prompt (bounded size).
pub fn format_inventory_for_prompt(inv: &ProjectAiInventory, max_chars: usize) -> String {
    let mut p = String::new();
    p.push_str(&format!(
        "## Project inventory\n- Mods: {} (enabled {})\n- Resource packs: {}\n- Shader packs: {}\n- Datapacks: {}\n- Config files: {}\n- KubeJS scripts: {}\n\n",
        inv.mods.len(),
        inv.mods.iter().filter(|m| m.enabled).count(),
        inv.resourcepacks.len(),
        inv.shaderpacks.len(),
        inv.datapacks.len(),
        inv.config_files.len(),
        inv.kubejs_scripts.len()
    ));

    p.push_str("### Mods\n");
    for m in &inv.mods {
        let flag = if m.enabled { "" } else { " [disabled]" };
        let ver = if m.version.is_empty() {
            String::new()
        } else {
            format!(" @{}", m.version)
        };
        let line = format!(
            "- {}{}{} ({}, side={}){}{}\n",
            m.id,
            ver,
            flag,
            m.content_type,
            m.side,
            m.file_name
                .as_ref()
                .map(|f| format!(" file={f}"))
                .unwrap_or_default(),
            if m.authors.is_empty() {
                String::new()
            } else {
                format!(" by={}", m.authors.join("/"))
            }
        );
        if p.len() + line.len() > max_chars {
            p.push_str("- … (truncated)\n");
            return p;
        }
        p.push_str(&line);
    }

    fn pack_section(
        p: &mut String,
        title: &str,
        packs: &[InventoryPack],
        max_chars: usize,
    ) -> bool {
        p.push_str(&format!("\n### {title}\n"));
        if packs.is_empty() {
            p.push_str("(none)\n");
            return true;
        }
        for pack in packs {
            let flag = if pack.enabled { "" } else { " [disabled]" };
            let line = format!(
                "- {}{} ({}, {})\n",
                pack.name, flag, pack.location, pack.kind
            );
            if p.len() + line.len() > max_chars {
                p.push_str("- … (truncated)\n");
                return false;
            }
            p.push_str(&line);
        }
        true
    }

    if !pack_section(&mut p, "Resource packs", &inv.resourcepacks, max_chars) {
        return p;
    }
    if !pack_section(&mut p, "Shader packs", &inv.shaderpacks, max_chars) {
        return p;
    }
    if !pack_section(&mut p, "Datapacks", &inv.datapacks, max_chars) {
        return p;
    }

    p.push_str("\n### Mod configs (paths)\n");
    for c in &inv.config_files {
        let line = format!("- {} ({} B)\n", c.relative_path, c.size);
        if p.len() + line.len() > max_chars {
            p.push_str("- … (truncated)\n");
            return p;
        }
        p.push_str(&line);
    }

    if !inv.kubejs_scripts.is_empty() {
        p.push_str("\n### KubeJS scripts\n");
        for s in &inv.kubejs_scripts {
            let line = format!("- {s}\n");
            if p.len() + line.len() > max_chars {
                p.push_str("- … (truncated)\n");
                return p;
            }
            p.push_str(&line);
        }
    }

    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_empty_inventory() {
        let inv = ProjectAiInventory::default();
        let text = format_inventory_for_prompt(&inv, 2000);
        assert!(text.contains("Mods: 0"));
    }

    #[test]
    fn collects_config_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("config")).unwrap();
        fs::write(dir.path().join("config").join("example.toml"), "a=1\n").unwrap();
        let inv = ProjectAiInventory {
            config_files: collect_config_files(dir.path()),
            ..Default::default()
        };
        assert_eq!(inv.config_files.len(), 1);
        assert!(inv.config_files[0].relative_path.contains("example.toml"));
    }
}
