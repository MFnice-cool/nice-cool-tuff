use crate::graph::NodeId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub severity: DiagnosticSeverity,
    pub code: String,
    pub message: String,
    pub related_nodes: Vec<NodeId>,
}

impl Diagnostic {
    pub fn info(
        code: impl Into<String>,
        message: impl Into<String>,
        related_nodes: Vec<NodeId>,
    ) -> Self {
        Self {
            severity: DiagnosticSeverity::Info,
            code: code.into(),
            message: message.into(),
            related_nodes,
        }
    }

    pub fn error(
        code: impl Into<String>,
        message: impl Into<String>,
        related_nodes: Vec<NodeId>,
    ) -> Self {
        Self {
            severity: DiagnosticSeverity::Error,
            code: code.into(),
            message: message.into(),
            related_nodes,
        }
    }

    pub fn warning(
        code: impl Into<String>,
        message: impl Into<String>,
        related_nodes: Vec<NodeId>,
    ) -> Self {
        Self {
            severity: DiagnosticSeverity::Warning,
            code: code.into(),
            message: message.into(),
            related_nodes,
        }
    }
}
