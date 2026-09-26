// Conceptual frontend types for TuffBox UI.

export type LoaderKind = 'vanilla' | 'fabric' | 'forge' | 'neoforge' | 'quilt';
export type Side = 'client' | 'server' | 'both' | 'optional' | 'unknown';
export type NodeKind =
  | 'MinecraftVersion'
  | 'Loader'
  | 'JavaRuntime'
  | 'Mod'
  | 'Library'
  | 'ConfigFile'
  | 'ScriptFile'
  | 'ResourcePack'
  | 'ShaderPack'
  | 'Profile';

export type EdgeKind =
  | 'requires'
  | 'optional'
  | 'conflicts'
  | 'breaks_with'
  | 'replaces'
  | 'requires_loader'
  | 'requires_minecraft'
  | 'requires_java'
  | 'client_only'
  | 'server_only'
  | 'both_sides'
  | 'loads_before'
  | 'loads_after'
  | 'configured_by'
  | 'modified_by_script';

export interface GraphNode {
  id: string;
  kind: NodeKind;
  label: string;
  version?: string;
  side: Side;
  status: NodeStatus[];
}

export type NodeStatus =
  | 'ok'
  | 'missing_dependency'
  | 'version_mismatch'
  | 'conflict'
  | 'duplicate'
  | 'deprecated'
  | 'unknown_side'
  | 'update_available'
  | 'risky_update'
  | 'local_only'
  | 'unresolved';

export interface GraphEdge {
  from: string;
  to: string;
  kind: EdgeKind;
  constraint?: string;
  reason?: string;
}

export interface DependencyGraphViewModel {
  nodes: GraphNode[];
  edges: GraphEdge[];
  diagnostics: Diagnostic[];
}

export interface Diagnostic {
  severity: 'info' | 'warning' | 'error';
  code: string;
  message: string;
  relatedNodes: string[];
}

export interface ChangePlan {
  summary: string;
  risk: 'low' | 'medium' | 'high';
  actions: ChangeAction[];
  requiresSnapshot: boolean;
}

export type ChangeAction =
  | { type: 'install_mod'; projectId: string; version: string }
  | { type: 'remove_mod'; nodeId: string }
  | { type: 'disable_mod'; nodeId: string }
  | { type: 'update_mod'; nodeId: string; targetVersion: string }
  | { type: 'edit_config'; path: string; patch: string };

export interface AiCrashExplanation {
  humanExplanation: string;
  confidence: number;
  suspectedNodes: string[];
  recommendedPlan: ChangeAction[];
  needsUserReview: boolean;
}
