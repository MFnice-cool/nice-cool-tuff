//! Conceptual dependency resolver for TuffBox.
//! The resolver is deterministic and must not depend on AI.

use crate::domain::*;

pub struct Resolver;

impl Resolver {
    pub fn analyze(graph: &DependencyGraph) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        diagnostics.extend(Self::find_missing_dependencies(graph));
        diagnostics.extend(Self::find_conflicts(graph));
        diagnostics.extend(Self::find_side_mismatches(graph));
        diagnostics.extend(Self::find_version_mismatches(graph));

        diagnostics
    }

    pub fn create_fix_plan(graph: &DependencyGraph, diagnostics: &[Diagnostic]) -> Option<ChangePlan> {
        // In real implementation this should:
        // 1. inspect diagnostics;
        // 2. query provider metadata;
        // 3. select compatible candidates;
        // 4. return a deterministic plan;
        // 5. never apply changes directly.

        let first_error = diagnostics.iter().find(|d| d.severity == DiagnosticSeverity::Error)?;

        Some(ChangePlan {
            summary: format!("Resolve diagnostic: {}", first_error.code),
            risk: ChangeRisk::Medium,
            actions: vec![],
            requires_snapshot: true,
        })
    }

    fn find_missing_dependencies(_graph: &DependencyGraph) -> Vec<Diagnostic> {
        vec![]
    }

    fn find_conflicts(graph: &DependencyGraph) -> Vec<Diagnostic> {
        graph
            .edges
            .iter()
            .filter(|edge| edge.kind == EdgeKind::Conflicts)
            .map(|edge| Diagnostic {
                severity: DiagnosticSeverity::Error,
                code: "MOD_CONFLICT".to_string(),
                message: edge
                    .reason
                    .clone()
                    .unwrap_or_else(|| "Two mods are marked as conflicting".to_string()),
                related_nodes: vec![edge.from.clone(), edge.to.clone()],
            })
            .collect()
    }

    fn find_side_mismatches(_graph: &DependencyGraph) -> Vec<Diagnostic> {
        vec![]
    }

    fn find_version_mismatches(_graph: &DependencyGraph) -> Vec<Diagnostic> {
        vec![]
    }
}
