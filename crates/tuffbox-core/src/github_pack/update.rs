use crate::manifest::{ProjectManifest, SourceKind};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TrustOrigin {
    Provider,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PackChange {
    ModAdded {
        id: String,
        version: String,
        origin: TrustOrigin,
    },
    ModRemoved {
        id: String,
        version: String,
    },
    ModBumped {
        id: String,
        from: String,
        to: String,
        origin: TrustOrigin,
    },
    MinecraftChanged {
        from: String,
        to: String,
    },
    LoaderChanged {
        from: String,
        to: String,
    },
    OverridesChanged {
        from: Vec<String>,
        to: Vec<String>,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackDiff {
    pub changes: Vec<PackChange>,
}

impl PackDiff {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    pub fn requires_full_reinstall(&self) -> bool {
        self.changes.iter().any(|c| {
            matches!(
                c,
                PackChange::MinecraftChanged { .. } | PackChange::LoaderChanged { .. }
            )
        })
    }
}

fn origin_of(kind: &SourceKind) -> TrustOrigin {
    match kind {
        SourceKind::Modrinth | SourceKind::Curseforge => TrustOrigin::Provider,
        _ => TrustOrigin::Custom,
    }
}

/// True when the remote commit is different from the locally installed one.
pub fn update_available(installed_commit_sha: Option<&str>, remote_commit_sha: &str) -> bool {
    match installed_commit_sha {
        Some(local) => local != remote_commit_sha,
        None => !remote_commit_sha.is_empty(),
    }
}

pub fn diff_manifests(old: &ProjectManifest, new: &ProjectManifest) -> PackDiff {
    let mut changes = Vec::new();
    if old.minecraft.version != new.minecraft.version {
        changes.push(PackChange::MinecraftChanged {
            from: old.minecraft.version.clone(),
            to: new.minecraft.version.clone(),
        });
    }
    let old_loader = format!("{}@{}", old.loader.kind.as_str(), old.loader.version);
    let new_loader = format!("{}@{}", new.loader.kind.as_str(), new.loader.version);
    if old_loader != new_loader {
        changes.push(PackChange::LoaderChanged {
            from: old_loader,
            to: new_loader,
        });
    }

    let old_mods: std::collections::BTreeMap<_, _> =
        old.mods.iter().map(|m| (m.id.as_str(), m)).collect();
    let new_mods: std::collections::BTreeMap<_, _> =
        new.mods.iter().map(|m| (m.id.as_str(), m)).collect();

    for (id, module) in &new_mods {
        match old_mods.get(id) {
            None => changes.push(PackChange::ModAdded {
                id: (*id).to_string(),
                version: module.version.clone(),
                origin: origin_of(&module.source.kind),
            }),
            Some(prev) if prev.version != module.version => {
                changes.push(PackChange::ModBumped {
                    id: (*id).to_string(),
                    from: prev.version.clone(),
                    to: module.version.clone(),
                    origin: origin_of(&module.source.kind),
                });
            }
            Some(_) => {}
        }
    }
    for (id, module) in &old_mods {
        if !new_mods.contains_key(id) {
            changes.push(PackChange::ModRemoved {
                id: (*id).to_string(),
                version: module.version.clone(),
            });
        }
    }

    let old_ov = override_paths(&old.overrides);
    let new_ov = override_paths(&new.overrides);
    if old_ov != new_ov {
        changes.push(PackChange::OverridesChanged {
            from: old_ov,
            to: new_ov,
        });
    }

    PackDiff { changes }
}

fn override_paths(spec: &Option<crate::manifest::OverridesSpec>) -> Vec<String> {
    let Some(spec) = spec else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Some(v) = &spec.config {
        out.push(format!("config:{v}"));
    }
    if let Some(v) = &spec.kubejs {
        out.push(format!("kubejs:{v}"));
    }
    if let Some(v) = &spec.resourcepacks {
        out.push(format!("resourcepacks:{v}"));
    }
    if let Some(v) = &spec.shaderpacks {
        out.push(format!("shaderpacks:{v}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{
        ContentType, FileHashes, LoaderKind, LoaderSpec, MinecraftSpec, ModSource, ModSpec,
        ProjectManifest, ProjectMetadata, Side, SourceKind,
    };

    fn manifest(mc: &str, loader: &str, mods: Vec<ModSpec>) -> ProjectManifest {
        ProjectManifest {
            schema_version: crate::manifest::CURRENT_PROJECT_SCHEMA_VERSION.into(),
            project: ProjectMetadata {
                id: "demo".into(),
                name: "Demo".into(),
                version: "1.0.0".into(),
                description: None,
                authors: vec![],
            },
            minecraft: MinecraftSpec { version: mc.into() },
            loader: LoaderSpec {
                kind: LoaderKind::Fabric,
                version: loader.into(),
            },
            brief: None,
            listing: None,
            java: None,
            profiles: vec![],
            mods,
            overrides: None,
        }
    }

    fn spec(id: &str, version: &str, kind: SourceKind) -> ModSpec {
        ModSpec {
            id: id.into(),
            name: id.into(),
            source: ModSource {
                kind,
                project_id: Some(id.into()),
                file_id: Some(version.into()),
                url: Some(format!("https://example.com/{id}.jar")),
                path: None,
                icon_url: None,
                categories: vec![],
            },
            version: version.into(),
            file_name: Some(format!("{id}.jar")),
            hashes: Some(FileHashes {
                sha1: Some("aa".into()),
                sha512: None,
            }),
            side: Side::Both,
            dependencies: vec![],
            status: vec![],
            content_type: ContentType::Mod,
            authors: vec![],
            option: None,
        }
    }

    #[test]
    fn diffs_add_remove_bump_and_loader() {
        let old = manifest(
            "1.21.1",
            "0.16.0",
            vec![
                spec("sodium", "0.6.0", SourceKind::Modrinth),
                spec("gone", "1.0.0", SourceKind::Github),
            ],
        );
        let new = manifest(
            "1.21.1",
            "0.16.1",
            vec![
                spec("sodium", "0.6.1", SourceKind::Modrinth),
                spec("iris", "1.0.0", SourceKind::Modrinth),
            ],
        );
        let diff = diff_manifests(&old, &new);
        assert!(diff.requires_full_reinstall());
        assert!(diff.changes.iter().any(|c| matches!(
            c,
            PackChange::ModBumped { id, from, to, origin: TrustOrigin::Provider }
            if id == "sodium" && from == "0.6.0" && to == "0.6.1"
        )));
        assert!(diff.changes.iter().any(|c| matches!(
            c,
            PackChange::ModAdded { id, origin: TrustOrigin::Provider, .. } if id == "iris"
        )));
        assert!(diff
            .changes
            .iter()
            .any(|c| matches!(c, PackChange::ModRemoved { id, .. } if id == "gone")));
    }

    #[test]
    fn diffs_override_paths() {
        let mut old = manifest("1.21.1", "0.16.0", vec![]);
        let mut new = manifest("1.21.1", "0.16.0", vec![]);
        new.overrides = Some(crate::manifest::OverridesSpec {
            config: Some("config".into()),
            kubejs: None,
            resourcepacks: None,
            shaderpacks: None,
        });
        old.overrides = None;
        let diff = diff_manifests(&old, &new);
        assert!(diff.changes.iter().any(|c| matches!(
            c,
            PackChange::OverridesChanged { to, .. } if to.iter().any(|p| p.starts_with("config:"))
        )));
    }
}
