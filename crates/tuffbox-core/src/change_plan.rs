use crate::graph::NodeId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChangeAction {
    InstallMod {
        project_id: String,
        version: Option<String>,
    },
    RemoveMod {
        node_id: NodeId,
    },
    DisableMod {
        node_id: NodeId,
    },
    UpdateMod {
        node_id: NodeId,
        target_version: String,
    },
    EditConfig {
        path: String,
        patch: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChangeRisk {
    Low,
    Medium,
    High,
}

/// One selectable resolution alternative within a ChangePlan (radio choice in
/// the diagnostics UI). Not all plans carry options — the UI falls back to the
/// flat `actions` list when `options` is empty.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeOption {
    /// Human label, e.g. "Disable Sodium".
    pub label: String,
    /// The mod kept by choosing this option (may be empty for system actions).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_mod: Option<String>,
    /// Why this is offered.
    pub reason: String,
    /// Recommended default (category-aware) for this plan.
    #[serde(default)]
    pub preferred: bool,
    /// The concrete actions this option executes when chosen.
    #[serde(default)]
    pub actions: Vec<ChangeAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangePlan {
    pub summary: String,
    pub risk: ChangeRisk,
    pub actions: Vec<ChangeAction>,
    pub requires_snapshot: bool,
    /// Selectable resolution alternatives, when the plan is a conflict. Empty
    /// for non-conflict plans (backward compatible).
    #[serde(default)]
    pub options: Vec<ChangeOption>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn change_plan_without_options_deserializes() {
        // Old serialized plans (no `options` key) must still deserialize.
        let raw = r#"{"summary":"x","risk":"Low","actions":[],"requiresSnapshot":false}"#;
        let plan: ChangePlan = serde_json::from_str(raw).expect("backward-compatible");
        assert!(plan.options.is_empty());
        assert_eq!(plan.risk, ChangeRisk::Low);
        assert!(!plan.requires_snapshot);
    }

    #[test]
    fn change_plan_serializes_options_camel_case() {
        let plan = ChangePlan {
            summary: "s".into(),
            risk: ChangeRisk::Medium,
            actions: vec![],
            requires_snapshot: true,
            options: vec![ChangeOption {
                label: "Disable Sodium".into(),
                keep_mod: Some("spb-revamped".into()),
                reason: "keep content".into(),
                preferred: true,
                actions: vec![ChangeAction::DisableMod {
                    node_id: crate::graph::NodeId::module("sodium"),
                }],
            }],
        };
        let json = serde_json::to_value(&plan).unwrap();
        let opts = &json["options"];
        assert_eq!(opts.as_array().unwrap().len(), 1);
        assert_eq!(opts[0]["label"], "Disable Sodium");
        assert_eq!(opts[0]["keepMod"], "spb-revamped");
        assert_eq!(opts[0]["preferred"], true);
    }

    #[test]
    fn change_plan_round_trips() {
        let plan = ChangePlan {
            summary: "s".into(),
            risk: ChangeRisk::High,
            actions: vec![],
            requires_snapshot: true,
            options: vec![ChangeOption {
                label: "l".into(),
                keep_mod: None,
                reason: "r".into(),
                preferred: false,
                actions: vec![],
            }],
        };
        let raw = serde_json::to_string(&plan).unwrap();
        let back: ChangePlan = serde_json::from_str(&raw).unwrap();
        assert_eq!(back.options.len(), 1);
    }
}
