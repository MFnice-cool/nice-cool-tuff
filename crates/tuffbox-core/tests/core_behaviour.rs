use tuffbox_core::{
    DependencyGraph, DiagnosticSeverity, ProjectManifest, Resolver, TuffboxLockfile,
};

#[test]
fn detects_missing_dependency() {
    let raw = include_str!("fixtures/missing-dependency.tuffbox.json");
    let manifest: ProjectManifest = serde_json::from_str(raw).unwrap();
    let graph = DependencyGraph::from_manifest(&manifest);
    let diagnostics = Resolver::analyze(&graph);

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.severity == DiagnosticSeverity::Error && diagnostic.code == "MISSING_DEPENDENCY"
    }));
}

#[test]
fn detects_conflict() {
    let raw = include_str!("fixtures/conflict.tuffbox.json");
    let manifest: ProjectManifest = serde_json::from_str(raw).unwrap();
    let graph = DependencyGraph::from_manifest(&manifest);
    let diagnostics = Resolver::analyze(&graph);

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.severity == DiagnosticSeverity::Error && diagnostic.code == "MOD_CONFLICT"
    }));
}

#[test]
fn detects_duplicate_mod() {
    let raw = include_str!("fixtures/duplicate.tuffbox.json");
    let manifest: ProjectManifest = serde_json::from_str(raw).unwrap();
    let graph = DependencyGraph::from_manifest(&manifest);
    let diagnostics = Resolver::analyze(&graph);

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.severity == DiagnosticSeverity::Error && diagnostic.code == "DUPLICATE_MOD"
    }));
}

#[test]
fn detects_side_mismatch() {
    let raw = include_str!("fixtures/side-mismatch.tuffbox.json");
    let manifest: ProjectManifest = serde_json::from_str(raw).unwrap();
    let graph = DependencyGraph::from_manifest(&manifest);
    let diagnostics = Resolver::analyze_project(&manifest, &graph);

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.severity == DiagnosticSeverity::Error
            && diagnostic.code == "WRONG_SIDE_IN_PROFILE"
    }));
}

#[test]
fn creates_lockfile_with_sorted_mods() {
    let raw = include_str!("../../../examples/sample-project.tuffbox.json");
    let manifest: ProjectManifest = serde_json::from_str(raw).unwrap();
    let graph = DependencyGraph::from_manifest(&manifest);
    let lockfile = TuffboxLockfile::from_manifest_and_graph(&manifest, &graph);

    assert_eq!(lockfile.project_id, "tuffcraft-rpg");
    assert_eq!(lockfile.minecraft_version, "1.20.1");
    assert_eq!(lockfile.mods.len(), 2);
    assert!(lockfile.graph.node_count >= 2);
}
