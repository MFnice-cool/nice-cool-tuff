//! Conceptual domain types for TuffBox core.
//! This is not final production code; it documents the intended model.

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProjectId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NodeId(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoaderKind {
    Vanilla,
    Fabric,
    Forge,
    NeoForge,
    Quilt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Side {
    Client,
    Server,
    Both,
    Optional,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeKind {
    MinecraftVersion,
    Loader,
    JavaRuntime,
    Mod,
    Library,
    ConfigFile,
    ScriptFile,
    ResourcePack,
    ShaderPack,
    Profile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EdgeKind {
    Requires,
    Optional,
    Conflicts,
    BreaksWith,
    Replaces,
    RequiresLoader,
    RequiresMinecraft,
    RequiresJava,
    ClientOnly,
    ServerOnly,
    BothSides,
    LoadsBefore,
    LoadsAfter,
    ConfiguredBy,
    ModifiedByScript,
}

#[derive(Debug, Clone)]
pub struct VersionConstraint {
    pub raw: String,
}

#[derive(Debug, Clone)]
pub struct GraphNode {
    pub id: NodeId,
    pub kind: NodeKind,
    pub label: String,
    pub version: Option<String>,
    pub side: Side,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct GraphEdge {
    pub from: NodeId,
    pub to: NodeId,
    pub kind: EdgeKind,
    pub constraint: Option<VersionConstraint>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DependencyGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: DiagnosticSeverity,
    pub code: String,
    pub message: String,
    pub related_nodes: Vec<NodeId>,
}

#[derive(Debug, Clone)]
pub enum ChangeAction {
    InstallMod { project_id: String, version: String },
    RemoveMod { node_id: NodeId },
    DisableMod { node_id: NodeId },
    UpdateMod { node_id: NodeId, target_version: String },
    EditConfig { path: String, patch: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeRisk {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone)]
pub struct ChangePlan {
    pub summary: String,
    pub risk: ChangeRisk,
    pub actions: Vec<ChangeAction>,
    pub requires_snapshot: bool,
}
