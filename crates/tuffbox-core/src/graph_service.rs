use crate::adapters::{FabricAdapter, ForgeAdapter, LoaderAdapter, NeoForgeAdapter};
use crate::diagnostics::{Diagnostic, DiagnosticSeverity};
use crate::{
    DependencyGraph, DependencyKind, LoaderKind, ModDependencySpec, ProjectManifest, Resolver,
};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Schema version for cache invalidation. Increment when the cache format or
/// enrichment logic changes so old caches are rebuilt automatically.
const CACHE_VERSION: u32 = 5;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphCache {
    pub manifest_fingerprint: String,
    /// Fingerprint of installed mod jars used during local metadata enrichment.
    /// Keeping it separate avoids rescanning unchanged jars while still
    /// invalidating the graph when a local jar is replaced in-place.
    #[serde(default)]
    pub installed_jars_fingerprint: String,
    pub generated_at: String,
    pub enriched_manifest: ProjectManifest,
    pub graph: DependencyGraph,
    #[serde(default)]
    pub cache_version: u32,
    /// True only after Modrinth/CurseForge enrich (`refresh_graph`). Jar-only
    /// warm caches stay `false` so the Graph view still background-refreshes.
    #[serde(default)]
    pub network_enriched: bool,
}

impl GraphCache {
    pub fn new(base_manifest: &ProjectManifest, enriched_manifest: ProjectManifest) -> Self {
        let graph = DependencyGraph::from_manifest(&enriched_manifest);
        Self {
            manifest_fingerprint: manifest_fingerprint(base_manifest),
            installed_jars_fingerprint: String::new(),
            generated_at: crate::time_util::rfc3339_now(),
            enriched_manifest,
            graph,
            cache_version: CACHE_VERSION,
            network_enriched: false,
        }
    }

    pub fn with_network_enriched(mut self) -> Self {
        self.network_enriched = true;
        self
    }

    pub fn load_if_current(
        manifest_path: &Path,
        manifest: &ProjectManifest,
    ) -> Result<Option<Self>, String> {
        let path = graph_cache_path(manifest_path)?;
        if !path.is_file() {
            return Ok(None);
        }
        // A cache is an optimization, never a reason for Diagnose to fail.
        // Interrupted writes, upgrades and manual edits are all treated as a
        // miss; the next warm pass will rebuild it.
        let Ok(raw) = std::fs::read_to_string(&path) else {
            return Ok(None);
        };
        let Ok(cache) = serde_json::from_str::<Self>(&raw) else {
            return Ok(None);
        };
        let expected_jars = installed_jars_fingerprint(manifest_path);
        // Empty is retained for programmatic GraphCache::new callers and
        // older test fixtures; warm_graph_cache always writes the real key.
        let jars_match = cache.installed_jars_fingerprint.is_empty()
            || cache.installed_jars_fingerprint == expected_jars;
        Ok((cache.cache_version == CACHE_VERSION
            && cache.manifest_fingerprint == manifest_fingerprint(manifest)
            && jars_match)
        .then_some(cache))
    }

    pub fn save(&self, manifest_path: &Path) -> Result<PathBuf, String> {
        let path = graph_cache_path(manifest_path)?;
        let parent = path
            .parent()
            .ok_or_else(|| format!("graph cache path has no parent: {}", path.display()))?;
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let bytes = serde_json::to_vec_pretty(self).map_err(|error| error.to_string())?;
        // Use the shared replacement primitive: unlike a direct persist, it
        // replaces an existing stale cache on every supported platform and
        // still never exposes a half-written JSON file to Diagnose.
        crate::fs_util::atomic_write(&path, bytes)?;
        Ok(path)
    }
}

/// Enriched manifest for UI click-path: GraphCache when current, otherwise the
/// raw manifest. Never opens installed jars.
pub fn enriched_manifest_for_click_path(
    manifest_path: &Path,
    manifest: &ProjectManifest,
) -> (ProjectManifest, bool) {
    match GraphCache::load_if_current(manifest_path, manifest) {
        Ok(Some(cache)) => (cache.enriched_manifest, true),
        _ => (manifest.clone(), false),
    }
}

#[derive(Debug, Clone)]
pub struct ClickPathDiagnostics {
    pub diagnostics: Vec<Diagnostic>,
    pub cached: bool,
}

/// Resolver diagnostics without zip-scanning `mods/*.jar`.
pub fn diagnostics_for_click_path(
    manifest_path: &Path,
    manifest: &ProjectManifest,
) -> ClickPathDiagnostics {
    let (enriched, cached) = enriched_manifest_for_click_path(manifest_path, manifest);
    let graph = DependencyGraph::from_manifest(&enriched);
    ClickPathDiagnostics {
        diagnostics: Resolver::analyze_project(&enriched, &graph),
        cached,
    }
}

/// Graph payload for UI click-path: cached graph as stored, else local
/// `from_manifest`. Does not attach disk content packs or scan jars.
pub fn graph_for_click_path(
    manifest_path: &Path,
    manifest: &ProjectManifest,
) -> (DependencyGraph, &'static str, Option<String>) {
    match GraphCache::load_if_current(manifest_path, manifest) {
        Ok(Some(cache)) => {
            let source = if cache.network_enriched {
                "cache"
            } else {
                "local"
            };
            (cache.graph, source, Some(cache.generated_at))
        }
        _ => (DependencyGraph::from_manifest(manifest), "local", None),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticCounts {
    pub error_count: usize,
    pub warning_count: usize,
    pub cached: bool,
}

pub fn diagnostic_counts(diagnostics: &[Diagnostic], cached: bool) -> DiagnosticCounts {
    DiagnosticCounts {
        error_count: diagnostics
            .iter()
            .filter(|d| d.severity == DiagnosticSeverity::Error)
            .count(),
        warning_count: diagnostics
            .iter()
            .filter(|d| d.severity == DiagnosticSeverity::Warning)
            .count(),
        cached,
    }
}

/// Write GraphCache when missing/stale. Jar enrich happens here, not on click.
/// Returns `true` when a new cache file was written.
pub fn warm_graph_cache(manifest_path: &Path, manifest: &ProjectManifest) -> Result<bool, String> {
    if GraphCache::load_if_current(manifest_path, manifest)?.is_some() {
        return Ok(false);
    }
    let mut enriched = manifest.clone();
    enrich_manifest_from_installed_jars(manifest_path, &mut enriched);
    let mut cache = GraphCache::new(manifest, enriched);
    cache.installed_jars_fingerprint = installed_jars_fingerprint(manifest_path);
    cache.save(manifest_path)?;
    Ok(true)
}

pub fn manifest_fingerprint(manifest: &ProjectManifest) -> String {
    let bytes = serde_json::to_vec(manifest).unwrap_or_default();
    format!("{:x}", Sha1::digest(bytes))
}

/// Cheap invalidation key for local jar enrichment. It intentionally hashes
/// names, lengths and mtimes rather than jar contents: download/materialize
/// code already verifies content hashes, while this keeps startup O(number of
/// directory entries) instead of reading hundreds of megabytes.
pub fn installed_jars_fingerprint(manifest_path: &Path) -> String {
    let Some(instance_dir) = crate::instance_dir_for_manifest(manifest_path) else {
        return String::new();
    };
    let mods_dir = instance_dir.join("mods");
    let mut entries = Vec::new();
    let Ok(read_dir) = std::fs::read_dir(mods_dir) else {
        return format!("{:x}", Sha1::digest(b""));
    };
    for entry in read_dir.flatten() {
        let path = entry.path();
        let is_jar = path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("jar"));
        if !is_jar {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        entries.push(format!(
            "{}:{}:{}",
            entry.file_name().to_string_lossy(),
            metadata.len(),
            modified
        ));
    }
    entries.sort_unstable();
    format!("{:x}", Sha1::digest(entries.join("\\n").as_bytes()))
}

pub fn graph_cache_path(manifest_path: &Path) -> Result<PathBuf, String> {
    let project_dir = manifest_path
        .parent()
        .ok_or_else(|| format!("manifest path has no parent: {}", manifest_path.display()))?;
    Ok(project_dir
        .join(".tuffbox")
        .join("cache")
        .join("dependency-graph.json"))
}

/// Adds dependency metadata directly from installed mod jars. Network data can
/// replace this later, but the graph remains useful offline and for local mods.
pub fn enrich_manifest_from_installed_jars(manifest_path: &Path, manifest: &mut ProjectManifest) {
    let Some(instance_dir) = crate::instance_dir_for_manifest(manifest_path) else {
        return;
    };
    let adapter: Box<dyn LoaderAdapter> = match manifest.loader.kind {
        LoaderKind::Fabric | LoaderKind::Quilt => Box::new(FabricAdapter),
        LoaderKind::Forge => Box::new(ForgeAdapter),
        LoaderKind::Neoforge => Box::new(NeoForgeAdapter),
        LoaderKind::Vanilla => return,
    };
    // Snapshot ids so we can rename Local mods to their jar mod id without collisions.
    let mut occupied: HashSet<String> = manifest.mods.iter().map(|m| m.id.clone()).collect();

    for module in &mut manifest.mods {
        let Some(file_name) = module.file_name.as_ref() else {
            continue;
        };
        let path = instance_dir.join("mods").join(file_name);
        let Ok(file) = std::fs::File::open(&path) else {
            continue;
        };
        let Ok(mut archive) = zip::ZipArchive::new(file) else {
            continue;
        };
        let Ok(metadata) = adapter.extract_metadata(&mut archive) else {
            continue;
        };

        // Local drop-ins often used the jar filename as `id`. Rewrite to the
        // descriptor id so Requires("meteor-client") resolves to the local jar.
        if matches!(module.source.kind, crate::manifest::SourceKind::Local)
            && !metadata.mod_id.is_empty()
            && metadata.mod_id != "unknown"
            && module.id != metadata.mod_id
            && !occupied.contains(&metadata.mod_id)
        {
            occupied.remove(&module.id);
            module.id = metadata.mod_id.clone();
            occupied.insert(module.id.clone());
            if module.name.ends_with(".jar") || module.name == file_name.as_str() {
                module.name = metadata.display_name.clone();
            }
            if module.version == "unknown" && metadata.version != "0.0.0" {
                module.version = metadata.version.clone();
            }
        }

        if !module.dependencies.is_empty() {
            continue;
        }
        module.dependencies = metadata
            .dependencies
            .into_iter()
            .filter(|dependency| {
                dependency.mod_id != module.id && !is_platform_dependency(&dependency.mod_id)
            })
            .map(|dependency| ModDependencySpec {
                target: dependency.mod_id,
                kind: if dependency.required {
                    DependencyKind::Requires
                } else {
                    DependencyKind::Optional
                },
                version_constraint: None,
                reason: Some("Read from installed mod metadata".to_string()),
            })
            .collect();
    }
}

/// Loader/runtime ids that appear in fabric.mod.json `depends` but are not pack mods.
fn is_platform_dependency(id: &str) -> bool {
    matches!(
        id.to_ascii_lowercase().as_str(),
        "minecraft"
            | "java"
            | "fabricloader"
            | "fabric-loader"
            | "quilt_loader"
            | "quilt-loader"
            | "forge"
            | "neoforge"
            | "neoforge-loader"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_is_invalidated_when_manifest_changes() {
        let manifest: ProjectManifest = serde_json::from_str(include_str!(
            "../../../examples/sample-project.tuffbox.json"
        ))
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("project.tuffbox.json");
        let cache = GraphCache::new(&manifest, manifest.clone());
        cache.save(&manifest_path).unwrap();
        assert!(GraphCache::load_if_current(&manifest_path, &manifest)
            .unwrap()
            .is_some());

        let mut changed = manifest;
        changed.project.version.push_str("-changed");
        assert!(GraphCache::load_if_current(&manifest_path, &changed)
            .unwrap()
            .is_none());
    }

    #[test]
    fn cache_is_invalidated_when_version_bumped() {
        let manifest: ProjectManifest = serde_json::from_str(include_str!(
            "../../../examples/sample-project.tuffbox.json"
        ))
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("project.tuffbox.json");
        // Save with a deliberately old version to simulate a stale cache.
        let mut cache = GraphCache::new(&manifest, manifest.clone());
        cache.cache_version = 0;
        cache.save(&manifest_path).unwrap();
        // Should be rejected: cache.cache_version (0) != CACHE_VERSION
        assert!(
            GraphCache::load_if_current(&manifest_path, &manifest)
                .unwrap()
                .is_none(),
            "old-version cache should be rejected"
        );
    }

    #[test]
    fn corrupted_cache_is_a_miss_not_a_diagnostics_error() {
        let manifest: ProjectManifest = serde_json::from_str(include_str!(
            "../../../examples/sample-project.tuffbox.json"
        ))
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("project.tuffbox.json");
        let cache_path = graph_cache_path(&manifest_path).unwrap();
        std::fs::create_dir_all(cache_path.parent().unwrap()).unwrap();
        std::fs::write(&cache_path, b"{ definitely not json").unwrap();
        assert!(GraphCache::load_if_current(&manifest_path, &manifest)
            .unwrap()
            .is_none());
        let result = diagnostics_for_click_path(&manifest_path, &manifest);
        assert!(!result.cached);
    }

    #[test]
    fn local_jar_change_invalidates_warmed_cache() {
        let manifest: ProjectManifest = serde_json::from_str(include_str!(
            "../../../examples/sample-project.tuffbox.json"
        ))
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("project.tuffbox.json");
        std::fs::create_dir_all(dir.path().join("mods")).unwrap();
        assert!(warm_graph_cache(&manifest_path, &manifest).unwrap());
        assert!(GraphCache::load_if_current(&manifest_path, &manifest)
            .unwrap()
            .is_some());
        std::fs::write(dir.path().join("mods").join("changed.jar"), b"jar").unwrap();
        assert!(GraphCache::load_if_current(&manifest_path, &manifest)
            .unwrap()
            .is_none());
    }

    #[test]
    fn click_path_diagnostics_without_cache_use_manifest_only() {
        let manifest: ProjectManifest = serde_json::from_str(include_str!(
            "../../../examples/sample-project.tuffbox.json"
        ))
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("project.tuffbox.json");
        let result = diagnostics_for_click_path(&manifest_path, &manifest);
        assert!(
            !result.cached,
            "missing cache must not pretend the graph was enriched"
        );
        let _ = result.diagnostics;
    }

    #[test]
    fn click_path_diagnostics_prefer_current_cache() {
        let manifest: ProjectManifest = serde_json::from_str(include_str!(
            "../../../examples/sample-project.tuffbox.json"
        ))
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("project.tuffbox.json");
        GraphCache::new(&manifest, manifest.clone())
            .save(&manifest_path)
            .unwrap();
        let result = diagnostics_for_click_path(&manifest_path, &manifest);
        assert!(result.cached);
    }

    #[test]
    fn diagnostic_counts_split_errors_and_warnings() {
        use crate::{Diagnostic, DiagnosticSeverity, NodeId};
        let diags = vec![
            Diagnostic::error(
                "MISSING_DEPENDENCY",
                "missing",
                vec![NodeId("mod:a".into())],
            ),
            Diagnostic::warning("UNKNOWN_SIDE", "side", vec![]),
            Diagnostic {
                severity: DiagnosticSeverity::Info,
                code: "NOTE".into(),
                message: "info".into(),
                related_nodes: vec![],
            },
        ];
        let counts = diagnostic_counts(&diags, true);
        assert_eq!(counts.error_count, 1);
        assert_eq!(counts.warning_count, 1);
        assert!(counts.cached);
    }

    #[test]
    fn click_path_graph_uses_cached_graph_without_disk_packs() {
        let manifest: ProjectManifest = serde_json::from_str(include_str!(
            "../../../examples/sample-project.tuffbox.json"
        ))
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("project.tuffbox.json");
        GraphCache::new(&manifest, manifest.clone())
            .with_network_enriched()
            .save(&manifest_path)
            .unwrap();
        let (graph, source, generated_at) = graph_for_click_path(&manifest_path, &manifest);
        assert_eq!(source, "cache");
        assert!(generated_at.is_some());
        assert!(!graph.nodes.is_empty());
    }

    #[test]
    fn click_path_graph_jar_cache_stays_local_source() {
        let manifest: ProjectManifest = serde_json::from_str(include_str!(
            "../../../examples/sample-project.tuffbox.json"
        ))
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("project.tuffbox.json");
        GraphCache::new(&manifest, manifest.clone())
            .save(&manifest_path)
            .unwrap();
        let (_graph, source, _) = graph_for_click_path(&manifest_path, &manifest);
        assert_eq!(source, "local");
    }

    #[test]
    fn warm_graph_cache_writes_when_missing() {
        let manifest: ProjectManifest = serde_json::from_str(include_str!(
            "../../../examples/sample-project.tuffbox.json"
        ))
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("project.tuffbox.json");
        assert!(warm_graph_cache(&manifest_path, &manifest).unwrap());
        assert!(!warm_graph_cache(&manifest_path, &manifest).unwrap());
        assert!(GraphCache::load_if_current(&manifest_path, &manifest)
            .unwrap()
            .is_some());
    }
}
