//! Policy engine that turns a set of conflict pairs + suspects into ranked,
//! category-aware resolution candidates. The highest-scoring candidates are
//! labelled `preferred` so the UI can offer a sensible default first and let the
//! user choose a radio alternative (e.g. disable the optimization mod instead of
//! the content mod, when that is safe).
//!
//! Resolution principles, in priority order:
//!   1. Safety: never hard-delete; suggest `.disabled` (reversible) first, and
//!      offer *both* sides as candidates — the default is the most replaceable.
//!   2. Replaceability: Library/API → update; Content → keep; Optimization /
//!      bridge / legacy / duplicate → disable.
//!   3. Data: number of dependents lost (from the graph) penalises a candidate.
//!   4. User preference: persisted per crash fingerprint (handled upstream).

use crate::{
    change_plan::ChangeAction,
    crash::SuspectedMod,
    graph::{DependencyGraph, EdgeKind, NodeId, NodeKind},
    manifest::ProjectManifest,
    mod_category::{is_legacy, replaceability, ModCategory},
    mod_conflict::{Conflict, ConflictKind},
};
use serde::{Deserialize, Serialize};

/// Penalty per dependent mod lost when disabling a target (weight).
const DEPENDENT_PENALTY: f32 = 14.0;
/// Bonus when the target is flagged legacy / abandoned.
const LEGACY_BONUS: f32 = 25.0;
/// Bonus for a duplicate target.
const DUPLICATE_BONUS: f32 = 20.0;

#[derive(Debug, Clone)]
pub struct ResolveCtx<'a> {
    pub graph: &'a DependencyGraph,
    /// Present when a manifest is available (graph + manifest). `None` when the
    /// resolver is driven from crash-log signals alone.
    pub manifest: Option<&'a ProjectManifest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankedFix {
    /// Concrete, actionable change (DisableMod / UpdateMod / InstallMod /…).
    pub action: ChangeAction,
    /// The mod that is *kept* by applying this candidate (may be empty).
    pub keep_mod: String,
    /// Human reason.
    pub reason: String,
    /// Higher = should be tried first.
    pub score: f32,
    /// True for the category-aware default(s) to suggest first.
    pub preferred: bool,
    /// Whether the change is reversible (disable vs remove).
    pub revertible: bool,
    /// Number of installed dependents lost by disabling the target.
    pub dependents_lost: usize,
}

/// How many installed mods transitively-or-directly depend on `target`.
pub fn dependents_count(graph: &DependencyGraph, target: &NodeId) -> usize {
    graph
        .edges
        .iter()
        .filter(|e| e.kind == EdgeKind::Requires && e.to == *target)
        .filter(|e| {
            graph
                .node(&e.from)
                .is_some_and(|n| n.kind == NodeKind::Mod && &n.id != target)
        })
        .count()
}

fn classify_for(_graph: &DependencyGraph, slug: &str) -> ModCategory {
    crate::mod_category::classify(slug, "")
}

/// Produce ranked resolution candidates for a conflict set. Deterministic:
/// sorted by `score` descending, then by slug for stability.
pub fn ranked_candidates(
    conflicts: &[Conflict],
    suspects: &[SuspectedMod],
    ctx: &ResolveCtx,
) -> Vec<RankedFix> {
    let mut candidates: Vec<RankedFix> = Vec::new();
    let graph = ctx.graph;

    // Missing-dependency style conflicts should always prefer to *install* the
    // missing side over disabling anything.
    for c in conflicts {
        if c.kind == ConflictKind::DependsOn {
            for side in [&c.a, &c.b] {
                let node = NodeId::module(side);
                if !graph.has_node(&node) && !manifest_has_mod(ctx.manifest, side) {
                    candidates.push(RankedFix {
                        action: ChangeAction::InstallMod {
                            project_id: side.clone(),
                            version: None,
                        },
                        keep_mod: c.a.clone(),
                        reason: format!(
                            "{} depends on missing {} — install it instead of disabling anything",
                            c.a, side
                        ),
                        score: 100.0,
                        preferred: true,
                        revertible: true,
                        dependents_lost: 0,
                    });
                }
            }
        }
    }

    // For every conflict, offer *both* sides as a disable candidate (the
    // resolver picks the higher-replaceability side as the preferred default).
    let mut seen_reasons: Vec<String> = Vec::new();
    for c in conflicts {
        for (victim, keeper) in [(c.a.clone(), c.b.clone()), (c.b.clone(), c.a.clone())] {
            // Only suppress a candidate when we are confident the target is
            // *not* installed: none in manifest, none in graph, and the log did
            // not name it as a suspect (log suspects are authoritative too).
            let victim_disabled = !manifest_has_mod(ctx.manifest, &victim)
                && !graph.has_node(&NodeId::module(&victim))
                && !suspects.iter().any(|s| s.id == victim);
            if victim_disabled {
                continue;
            }
            let reason = format!("Disable {} to resolve conflict with {}", victim, keeper);
            if seen_reasons.iter().any(|r| r == &reason) {
                continue;
            }
            seen_reasons.push(reason.clone());
            let cat = classify_for(graph, &victim);
            let node = NodeId::module(&victim);
            let lost = dependents_count(graph, &node);
            let legacy = is_legacy(&victim) || cat == ModCategory::Legacy;
            let duplicate = cat == ModCategory::Duplicate;
            let mut score = replaceability(cat) as f32 - lost as f32 * DEPENDENT_PENALTY;
            if legacy {
                score += LEGACY_BONUS;
            }
            if duplicate {
                score += DUPLICATE_BONUS;
            }
            let preferred = score >= 60.0 && lost == 0 && !victim_disabled;
            candidates.push(RankedFix {
                action: ChangeAction::DisableMod {
                    node_id: node.clone(),
                },
                keep_mod: keeper.clone(),
                reason,
                score,
                preferred,
                revertible: true,
                dependents_lost: lost,
            });
        }
    }

    candidates.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a_reason_key(a).cmp(&a_reason_key(b)))
    });
    candidates
}

fn a_reason_key(c: &RankedFix) -> String {
    match &c.action {
        ChangeAction::DisableMod { node_id } | ChangeAction::UpdateMod { node_id, .. } => {
            node_id.0.clone()
        }
        ChangeAction::InstallMod { project_id, .. } => project_id.clone(),
        _ => String::new(),
    }
}

fn manifest_has_mod(manifest: Option<&ProjectManifest>, id: &str) -> bool {
    manifest
        .map(|m| m.mods.iter().any(|mod_spec| mod_spec.id == id))
        .unwrap_or(false)
}

/// Choose the single best default fix (first preferred candidate, else best).
pub fn best_fix(
    conflicts: &[Conflict],
    suspects: &[SuspectedMod],
    ctx: &ResolveCtx,
) -> Option<RankedFix> {
    let ranked = ranked_candidates(conflicts, suspects, ctx);
    ranked
        .iter()
        .find(|c| c.preferred)
        .or_else(|| ranked.first())
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::DependencyGraph;
    use crate::manifest::{
        LoaderKind, LoaderSpec, MinecraftSpec, ProjectManifest, ProjectMetadata, Side,
    };

    fn minimal_manifest() -> ProjectManifest {
        ProjectManifest {
            schema_version: "1".into(),
            project: ProjectMetadata {
                id: "p".into(),
                name: "P".into(),
                version: "1".into(),
                description: None,
                authors: vec![],
            },
            minecraft: MinecraftSpec {
                version: "1.20.1".into(),
            },
            loader: LoaderSpec {
                kind: LoaderKind::Fabric,
                version: "0.15.10".into(),
            },
            brief: None,
            listing: None,
            java: None,
            profiles: vec![],
            mods: vec![],
            overrides: None,
        }
    }

    fn ctx(graph: DependencyGraph, manifest: &ProjectManifest) -> ResolveCtx<'static> {
        let boxed: Box<DependencyGraph> = Box::new(graph);
        let m: &'static ProjectManifest = Box::leak(Box::new(manifest.clone()));
        let g: &'static DependencyGraph = Box::leak(boxed);
        ResolveCtx {
            graph: g,
            manifest: Some(m),
        }
    }

    fn suspect(id: &str) -> SuspectedMod {
        SuspectedMod {
            id: id.to_string(),
            name: id.to_string(),
            version: None,
            file_name: None,
            known_in_manifest: true,
            confidence: 90,
            evidence: vec![],
            authors: vec![],
            blame_role: Default::default(),
            match_sources: vec![],
        }
    }

    fn make_conflicts() -> Vec<Conflict> {
        vec![
            Conflict {
                a: "spb-revamped".into(),
                b: "sodium".into(),
                kind: ConflictKind::Breaking,
                reason: "spb-revamped breaks sodium".into(),
            },
            Conflict {
                a: "spb-revamped".into(),
                b: "indium".into(),
                kind: ConflictKind::Breaking,
                reason: "spb-revamped breaks indium".into(),
            },
        ]
    }

    #[test]
    fn spb_prefers_disable_optimization_side() {
        let graph = DependencyGraph::default();
        let manifest = minimal_manifest();
        let suspects = vec![
            suspect("spb-revamped"),
            suspect("sodium"),
            suspect("indium"),
        ];
        let ranked = ranked_candidates(&make_conflicts(), &suspects, &ctx(graph, &manifest));
        let preferred: Vec<&RankedFix> = ranked.iter().filter(|c| c.preferred).collect();
        // Preferred first moves = the optimization/bridge sides, not the content.
        assert!(preferred.iter().any(|c| {
            matches!(&c.action, ChangeAction::DisableMod { node_id } if node_id.0 == "mod:sodium")
        }));
        assert!(preferred.iter().any(|c| {
            matches!(&c.action, ChangeAction::DisableMod { node_id } if node_id.0 == "mod:indium")
        }));
        // The content side (spb) must NOT be a preferred default.
        assert!(
            !preferred.iter().any(|c| {
                matches!(&c.action, ChangeAction::DisableMod { node_id } if node_id.0 == "mod:spb-revamped")
            })
        );
        // Both sides offered as alternatives.
        assert!(
            ranked.iter().any(|c| {
                matches!(&c.action, ChangeAction::DisableMod { node_id } if node_id.0 == "mod:spb-revamped")
            })
        );
    }

    #[test]
    fn legacy_content_shifts_weight_toward_legacy() {
        let graph = DependencyGraph::default();
        let manifest = minimal_manifest();
        // A legacy content pack (old build) — disable the legacy side first.
        let conflicts = vec![Conflict {
            a: "spb-revamped" // not classified legacy here
                .into(),
            b: "sodium".into(),
            kind: ConflictKind::Breaking,
            reason: "x".into(),
        }];
        let suspects = vec![suspect("spb-revamped"), suspect("sodium")];
        let ranked = ranked_candidates(&conflicts, &suspects, &ctx(graph, &manifest));
        let best = ranked.first().unwrap();
        // sodium still preferred (higher replaceability) for non-legacy content.
        assert!(
            matches!(&best.action, ChangeAction::DisableMod { node_id } if node_id.0 != "mod:spb-revamped")
        );
    }

    #[test]
    fn depends_on_missing_prefers_install() {
        let graph = DependencyGraph::default();
        let manifest = minimal_manifest();
        let conflicts = vec![Conflict {
            a: "create".into(),
            b: "shimmer".into(),
            kind: ConflictKind::DependsOn,
            reason: "create requires shimmer".into(),
        }];
        let suspects = vec![suspect("create")];
        let ranked = ranked_candidates(&conflicts, &suspects, &ctx(graph, &manifest));
        assert!(
            ranked.iter().any(|c| {
                matches!(&c.action, ChangeAction::InstallMod { project_id, .. } if project_id == "shimmer")
            })
        );
    }

    #[test]
    fn dependents_penalize_disable() {
        // Build a tiny graph where "fabric-api" is depended on by two mods.
        let mut graph = DependencyGraph::default();
        graph.nodes.push(crate::graph::GraphNode {
            id: NodeId::module("sodium"),
            kind: NodeKind::Mod,
            label: "Sodium".into(),
            version: None,
            side: Side::Client,
            metadata: Default::default(),
        });
        graph.nodes.push(crate::graph::GraphNode {
            id: NodeId::module("extras"),
            kind: NodeKind::Mod,
            label: "Extras".into(),
            version: None,
            side: Side::Client,
            metadata: Default::default(),
        });
        graph.nodes.push(crate::graph::GraphNode {
            id: NodeId::module("fabric-api"),
            kind: NodeKind::Mod,
            label: "Fabric API".into(),
            version: None,
            side: Side::Client,
            metadata: Default::default(),
        });
        graph.edges.push(crate::graph::GraphEdge {
            from: NodeId::module("sodium"),
            to: NodeId::module("fabric-api"),
            kind: EdgeKind::Requires,
            constraint: None,
            reason: Some("dep".into()),
        });
        graph.edges.push(crate::graph::GraphEdge {
            from: NodeId::module("extras"),
            to: NodeId::module("fabric-api"),
            kind: EdgeKind::Requires,
            constraint: None,
            reason: Some("dep".into()),
        });
        graph.rebuild_index();
        let manifest = minimal_manifest();
        let conflicts = vec![Conflict {
            a: "create".into(),
            b: "fabric-api".into(),
            kind: ConflictKind::Breaking,
            reason: "x".into(),
        }];
        let suspects = vec![suspect("create")];
        let ranked = ranked_candidates(&conflicts, &suspects, &ctx(graph, &manifest));
        let fab = ranked
            .iter()
            .find(|c| matches!(&c.action, ChangeAction::DisableMod { node_id } if node_id.0 == "mod:fabric-api"));
        assert!(fab.is_some());
        assert_eq!(fab.unwrap().dependents_lost, 2);
        // fabric-api consumers means it is not preferred as a disable target.
        assert!(!fab.unwrap().preferred);
    }
}
