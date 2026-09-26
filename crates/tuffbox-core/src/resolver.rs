use crate::{
    change_plan::{ChangeAction, ChangePlan, ChangeRisk},
    diagnostics::{Diagnostic, DiagnosticSeverity},
    graph::{DependencyGraph, EdgeKind, NodeId, NodeKind},
    manifest::{ProjectManifest, Side},
    mod_category,
    resolve::dependents_count,
};
use std::collections::{HashMap, HashSet};

/// Pick which mod node of a conflict pair is safest to disable: prefer a
/// higher replaceability category (optimization / bridge / legacy / duplicate)
/// and fewer dependents, falling back to the last node (legacy behaviour).
fn pick_conflict_removable(graph: &DependencyGraph, related_nodes: &[NodeId]) -> Option<NodeId> {
    let score = |nid: &NodeId| -> f32 {
        let slug = nid.0.strip_prefix("mod:").unwrap_or(&nid.0);
        let cat = mod_category::classify(slug, "");
        let repl = mod_category::replaceability(cat) as f32;
        let lost = dependents_count(graph, nid) as f32 * 14.0;
        repl - lost
    };
    related_nodes
        .iter()
        .filter(|nid| nid.0.starts_with("mod:"))
        .max_by(|a, b| {
            score(a)
                .partial_cmp(&score(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .cloned()
}

pub struct Resolver;

/// Keep the diagnostics contract deterministic for both the UI and fix-plan
/// generation. Graphs can contain repeated metadata edges (and HashSet-based
/// enrichment intentionally has no iteration order); exposing that directly
/// produced duplicate cards and flaky action ordering.
fn normalize_diagnostics(mut diagnostics: Vec<Diagnostic>) -> Vec<Diagnostic> {
    let mut seen = HashSet::new();
    diagnostics.retain(|diagnostic| {
        let related = diagnostic
            .related_nodes
            .iter()
            .map(|node| node.0.as_str())
            .collect::<Vec<_>>()
            .join("\u{1f}");
        seen.insert(format!(
            "{:?}\u{1f}{}\u{1f}{}\u{1f}{}",
            diagnostic.severity, diagnostic.code, diagnostic.message, related
        ))
    });
    diagnostics.sort_by(|a, b| {
        severity_rank(&a.severity)
            .cmp(&severity_rank(&b.severity))
            .then_with(|| a.code.cmp(&b.code))
            .then_with(|| a.message.cmp(&b.message))
            .then_with(|| format_node_ids(&a.related_nodes).cmp(&format_node_ids(&b.related_nodes)))
    });
    diagnostics
}

fn severity_rank(severity: &DiagnosticSeverity) -> u8 {
    match severity {
        DiagnosticSeverity::Error => 0,
        DiagnosticSeverity::Warning => 1,
        DiagnosticSeverity::Info => 2,
    }
}

fn format_node_ids(nodes: &[NodeId]) -> String {
    nodes.iter().map(|node| node.0.as_str()).collect::<Vec<_>>().join("\u{1f}")
}

impl Resolver {
    pub fn analyze(graph: &DependencyGraph) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        diagnostics.extend(Self::find_missing_dependencies(graph));
        diagnostics.extend(Self::find_conflicts(graph));
        diagnostics.extend(Self::find_duplicate_mod_ids(graph));
        diagnostics.extend(Self::find_profile_includes_unknown_mod(graph));
        normalize_diagnostics(diagnostics)
    }

    pub fn analyze_project(manifest: &ProjectManifest, graph: &DependencyGraph) -> Vec<Diagnostic> {
        let mut diagnostics = Self::analyze(graph);
        diagnostics.extend(Self::find_wrong_side_in_profile(manifest));
        diagnostics.extend(Self::find_unknown_sides(graph));
        normalize_diagnostics(diagnostics)
    }

    pub fn create_fix_plan(
        graph: &DependencyGraph,
        diagnostics: &[Diagnostic],
    ) -> Option<ChangePlan> {
        // One InstallMod per unique slug — several mods may require the same dep.
        let mut seen = HashSet::new();
        let missing_deps: Vec<String> = diagnostics
            .iter()
            .filter(|d| d.severity == DiagnosticSeverity::Error && d.code == "MISSING_DEPENDENCY")
            .filter_map(|d| d.related_nodes.last())
            .filter_map(|id| id.0.strip_prefix("mod:").map(|s| s.to_string()))
            .filter(|slug| seen.insert(slug.clone()))
            .collect();

        if !missing_deps.is_empty() {
            let actions: Vec<ChangeAction> = missing_deps
                .iter()
                .map(|slug| ChangeAction::InstallMod {
                    project_id: slug.clone(),
                    version: None,
                })
                .collect();
            let summary = if missing_deps.len() == 1 {
                format!("Install missing dependency: {}", missing_deps[0])
            } else {
                format!("Install {} missing dependencies", missing_deps.len())
            };
            return Some(ChangePlan {
                summary,
                risk: ChangeRisk::Low,
                actions,
                requires_snapshot: true,
                options: Vec::new(),
            });
        }

        if let Some(conflict) = diagnostics
            .iter()
            .find(|d| d.severity == DiagnosticSeverity::Error && d.code == "MOD_CONFLICT")
        {
            // Choose the more *replaceable* / less-coupled side as the disable
            // target (category-aware), instead of blindly disabling the last
            // related node. Disabling optimization / bridge / legacy mods is
            // safe & reversible; disabling content or libraries is not.
            let Some(removable) = pick_conflict_removable(graph, &conflict.related_nodes) else {
                return None;
            };
            let label = graph
                .node(&removable)
                .map(|n| n.label.clone())
                .unwrap_or(removable.0.clone());
            return Some(ChangePlan {
                summary: format!("Disable conflicting mod: {label}"),
                risk: ChangeRisk::Medium,
                actions: vec![ChangeAction::DisableMod { node_id: removable }],
                requires_snapshot: true,
                options: Vec::new(),
            });
        }

        // Local fallback (kept from the older local version): for any other error
        // diagnostic that has no specific handler above, still surface a review
        // plan so callers always get *something* actionable instead of `None`.
        if let Some(other) = diagnostics
            .iter()
            .find(|d| d.severity == DiagnosticSeverity::Error)
        {
            return Some(ChangePlan {
                summary: format!("Review diagnostic: {}", other.code),
                risk: ChangeRisk::Medium,
                actions: vec![],
                requires_snapshot: true,
                options: Vec::new(),
            });
        }

        None
    }

    fn find_missing_dependencies(graph: &DependencyGraph) -> Vec<Diagnostic> {
        graph
            .edges
            .iter()
            .filter(|edge| edge.kind == EdgeKind::Requires)
            .filter(|edge| {
                graph
                    .node(&edge.to)
                    .map(|node| node.kind == NodeKind::Missing)
                    .unwrap_or(true)
            })
            .map(|edge| {
                let from = graph
                    .node(&edge.from)
                    .map(|n| n.label.clone())
                    .unwrap_or(edge.from.0.clone());
                Diagnostic::error(
                    "MISSING_DEPENDENCY",
                    format!("{from} requires missing dependency {}", edge.to.0),
                    vec![edge.from.clone(), edge.to.clone()],
                )
            })
            .collect()
    }

    fn find_conflicts(graph: &DependencyGraph) -> Vec<Diagnostic> {
        graph
            .edges
            .iter()
            .filter(|edge| matches!(edge.kind, EdgeKind::Conflicts | EdgeKind::BreaksWith))
            .filter(|edge| {
                graph
                    .node(&edge.from)
                    .is_some_and(|node| node.kind != NodeKind::Missing)
                    && graph
                        .node(&edge.to)
                        .is_some_and(|node| node.kind != NodeKind::Missing)
            })
            .map(|edge| {
                let from = graph
                    .node(&edge.from)
                    .map(|n| n.label.clone())
                    .unwrap_or(edge.from.0.clone());
                let to = graph
                    .node(&edge.to)
                    .map(|n| n.label.clone())
                    .unwrap_or(edge.to.0.clone());
                Diagnostic::error(
                    "MOD_CONFLICT",
                    edge.reason
                        .clone()
                        .unwrap_or_else(|| format!("{from} conflicts with {to}")),
                    vec![edge.from.clone(), edge.to.clone()],
                )
            })
            .collect()
    }

    fn find_duplicate_mod_ids(graph: &DependencyGraph) -> Vec<Diagnostic> {
        let mut seen: HashMap<&str, Vec<NodeId>> = HashMap::new();
        for node in &graph.nodes {
            if node.kind == NodeKind::Mod {
                seen.entry(node.id.0.as_str())
                    .or_default()
                    .push(node.id.clone());
            }
        }

        seen.into_iter()
            .filter(|(_, ids)| ids.len() > 1)
            .map(|(id, ids)| {
                Diagnostic::error("DUPLICATE_MOD", format!("Duplicate mod node: {id}"), ids)
            })
            .collect()
    }

    fn find_profile_includes_unknown_mod(graph: &DependencyGraph) -> Vec<Diagnostic> {
        let mod_nodes: HashSet<_> = graph
            .nodes
            .iter()
            .filter(|node| node.kind == NodeKind::Mod)
            .map(|node| node.id.clone())
            .collect();

        graph
            .edges
            .iter()
            .filter(|edge| edge.kind == EdgeKind::IncludedInProfile)
            .filter(|edge| !mod_nodes.contains(&edge.to))
            .map(|edge| {
                Diagnostic::warning(
                    "PROFILE_INCLUDES_UNKNOWN_MOD",
                    format!("Profile edge points to unknown mod {}", edge.to.0),
                    vec![edge.from.clone(), edge.to.clone()],
                )
            })
            .collect()
    }

    fn find_wrong_side_in_profile(manifest: &ProjectManifest) -> Vec<Diagnostic> {
        let mod_map: HashMap<String, Side> = manifest
            .mods
            .iter()
            .map(|module| (module.id.clone(), module.side))
            .collect();

        let mut diagnostics = Vec::new();
        for profile in &manifest.profiles {
            for mod_id in &profile.include_mods {
                let Some(&module_side) = mod_map.get(mod_id) else {
                    continue;
                };
                if module_side.is_compatible_with_profile(profile.side) {
                    continue;
                }
                diagnostics.push(Diagnostic::error(
                    "WRONG_SIDE_IN_PROFILE",
                    format!(
                        "Profile {} includes mod {} with incompatible side {:?}",
                        profile.name, mod_id, module_side
                    ),
                    vec![NodeId::profile(&profile.id), NodeId::module(mod_id)],
                ));
            }
        }
        diagnostics
    }

    fn find_unknown_sides(graph: &DependencyGraph) -> Vec<Diagnostic> {
        graph
            .nodes
            .iter()
            .filter(|node| node.kind == NodeKind::Mod && node.side == Side::Unknown)
            .map(|node| {
                Diagnostic::warning(
                    "UNKNOWN_SIDE",
                    format!(
                        "Mod {} has unknown side; verify profile compatibility",
                        node.label
                    ),
                    vec![node.id.clone()],
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{graph::DependencyGraph, manifest::ProjectManifest};

    #[test]
    fn sample_manifest_builds_graph_without_errors() {
        let raw = include_str!("../../../examples/sample-project.tuffbox.json");
        let manifest: ProjectManifest = serde_json::from_str(raw).unwrap();
        let graph = DependencyGraph::from_manifest(&manifest);
        let diagnostics = Resolver::analyze(&graph);
        assert!(
            diagnostics.is_empty(),
            "expected no diagnostics, got {diagnostics:#?}"
        );
    }

    #[test]
    fn change_plan_dedupes_repeated_missing_slugs() {
        use crate::diagnostics::{Diagnostic, DiagnosticSeverity};
        use crate::graph::NodeId;

        let diagnostics = vec![
            Diagnostic::error(
                "MISSING_DEPENDENCY",
                "a requires missing dependency mod:meteor-client",
                vec![NodeId::module("a"), NodeId::module("meteor-client")],
            ),
            Diagnostic::error(
                "MISSING_DEPENDENCY",
                "b requires missing dependency mod:meteor-client",
                vec![NodeId::module("b"), NodeId::module("meteor-client")],
            ),
            Diagnostic::error(
                "MISSING_DEPENDENCY",
                "c requires missing dependency mod:nuit",
                vec![NodeId::module("c"), NodeId::module("nuit")],
            ),
        ];
        assert!(diagnostics
            .iter()
            .all(|d| d.severity == DiagnosticSeverity::Error));

        let graph = DependencyGraph::default();
        let plan = Resolver::create_fix_plan(&graph, &diagnostics).expect("plan");
        assert_eq!(
            plan.actions.len(),
            2,
            "duplicate meteor-client must collapse"
        );
        assert_eq!(plan.summary, "Install 2 missing dependencies");
    }
}
