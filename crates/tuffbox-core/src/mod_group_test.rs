//! Adaptive combinatorial group testing for crash-culprit mods.
//!
//! A launch is a group test on the **enabled** set: crash ⇔ the enabled set
//! contains at least one defective. Healthy ⇔ every defective is disabled
//! (the disabled suspects form a covering).
//!
//! Binary search is the d=1 special case and is wrong when two conflicting
//! mods sit in different halves: a crash must not re-enable the other half.
//! This module peels a covering (Hwang-style split of the positive group).

use crate::action_plan::{ActionPlan, LauncherAction, ACTION_PLAN_SCHEMA_VERSION};
use crate::manifest::{ContentType, DependencyKind, ModSpec};
use serde::{Deserialize, Serialize};

/// Mods that must not be disabled while dependents stay enabled.
const PROTECTED_IDS: &[&str] = &[
    "minecraft",
    "java",
    "forge",
    "neoforge",
    "fabricloader",
    "fabric-loader",
    "fabric",
    "fabric-api",
    "fabric-api-base",
    "quilt_loader",
    "quilted-fabric-api",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TestOutcome {
    Healthy,
    Crash,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GroupTestPhase {
    /// Disable the whole pool; wait for the first launch.
    NeedCovering,
    /// Enable `test_group` from covering; rest of covering stays disabled.
    Testing,
    /// Disable only isolated defectives; rest of pool enabled.
    VerifyAll,
    /// Enable `defectives[index]`; other defectives stay disabled.
    VerifyOne {
        index: usize,
    },
    Done,
    Failed {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GroupTestSession {
    pub pool: Vec<String>,
    pub covering: Vec<String>,
    pub known_clean: Vec<String>,
    pub defectives: Vec<String>,
    /// Currently enabled subset of `covering` under test.
    pub test_group: Vec<String>,
    pub phase: GroupTestPhase,
    pub snapshot_id: Option<String>,
    pub verified: bool,
    pub step: u32,
}

impl GroupTestSession {
    pub fn start(pool: Vec<String>) -> Self {
        let covering = pool.clone();
        Self {
            pool,
            covering,
            known_clean: Vec::new(),
            defectives: Vec::new(),
            test_group: Vec::new(),
            phase: GroupTestPhase::NeedCovering,
            snapshot_id: None,
            verified: false,
            step: 0,
        }
    }

    /// Mods that must be disabled for the next launch.
    pub fn desired_disabled(&self) -> Vec<String> {
        match &self.phase {
            GroupTestPhase::NeedCovering => self.covering.clone(),
            GroupTestPhase::Testing => {
                let mut out: Vec<String> = self
                    .covering
                    .iter()
                    .filter(|id| !self.test_group.iter().any(|g| g == *id))
                    .cloned()
                    .collect();
                out.extend(self.defectives.iter().cloned());
                out.sort();
                out.dedup();
                out
            }
            GroupTestPhase::VerifyAll | GroupTestPhase::Done => self.defectives.clone(),
            GroupTestPhase::VerifyOne { index } => self
                .defectives
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != *index)
                .map(|(_, id)| id.clone())
                .collect(),
            GroupTestPhase::Failed { .. } => self
                .covering
                .iter()
                .chain(self.defectives.iter())
                .cloned()
                .collect(),
        }
    }

    pub fn status_line(&self) -> String {
        match &self.phase {
            GroupTestPhase::NeedCovering => format!(
                "Disable all {} suspects, then test launch.",
                self.covering.len()
            ),
            GroupTestPhase::Testing => format!(
                "Enable [{}]; keep {} other suspects disabled.",
                self.test_group.join(", "),
                self.covering.len().saturating_sub(self.test_group.len()) + self.defectives.len()
            ),
            GroupTestPhase::VerifyAll => {
                format!("Verify: disable only [{}].", self.defectives.join(", "))
            }
            GroupTestPhase::VerifyOne { index } => {
                let id = self.defectives.get(*index).cloned().unwrap_or_default();
                format!("Verify: re-enable {id} (should crash).")
            }
            GroupTestPhase::Done => format!(
                "Isolated: [{}].",
                if self.defectives.is_empty() {
                    "none".into()
                } else {
                    self.defectives.join(", ")
                }
            ),
            GroupTestPhase::Failed { reason } => reason.clone(),
        }
    }

    pub fn apply_outcome(&mut self, outcome: TestOutcome) {
        if matches!(
            self.phase,
            GroupTestPhase::Done | GroupTestPhase::Failed { .. }
        ) {
            return;
        }
        self.step = self.step.saturating_add(1);
        match self.phase.clone() {
            GroupTestPhase::NeedCovering => match outcome {
                TestOutcome::Crash => {
                    self.phase = GroupTestPhase::Failed {
                        reason: "Still crashed with the whole suspect pool disabled — culprit is not in the pool.".into(),
                    };
                }
                TestOutcome::Healthy => self.begin_peel(),
            },
            GroupTestPhase::Testing => match outcome {
                TestOutcome::Healthy => {
                    // G contains no defective.
                    let g = std::mem::take(&mut self.test_group);
                    self.known_clean.extend(g.iter().cloned());
                    self.covering.retain(|id| !g.iter().any(|x| x == id));
                    self.begin_peel();
                }
                TestOutcome::Crash => {
                    // G contains ≥1 defective. Do not mark covering\G clean.
                    if self.test_group.len() <= 1 {
                        if let Some(d) = self.test_group.first().cloned() {
                            if !self.defectives.contains(&d) {
                                self.defectives.push(d.clone());
                            }
                            self.covering.retain(|id| id != &d);
                        }
                        self.test_group.clear();
                        self.begin_peel();
                    } else {
                        let n = peel_group_size(self.test_group.len());
                        self.test_group.truncate(n.max(1));
                    }
                }
            },
            GroupTestPhase::VerifyAll => match outcome {
                TestOutcome::Healthy => {
                    if self.defectives.is_empty() {
                        self.phase = GroupTestPhase::Failed {
                            reason: "Covering peeled to empty with no isolated defectives.".into(),
                        };
                    } else {
                        self.phase = GroupTestPhase::VerifyOne { index: 0 };
                    }
                }
                TestOutcome::Crash => {
                    self.verified = false;
                    self.phase = GroupTestPhase::Failed {
                        reason:
                            "Verify failed: pack still crashes with only isolated mods disabled."
                                .into(),
                    };
                }
            },
            GroupTestPhase::VerifyOne { index } => match outcome {
                TestOutcome::Crash => {
                    let next = index + 1;
                    if next >= self.defectives.len() {
                        self.verified = true;
                        self.phase = GroupTestPhase::Done;
                    } else {
                        self.phase = GroupTestPhase::VerifyOne { index: next };
                    }
                }
                TestOutcome::Healthy => {
                    // This claimed defective was not necessary.
                    if index < self.defectives.len() {
                        self.defectives.remove(index);
                    }
                    if self.defectives.is_empty() {
                        self.verified = false;
                        self.phase = GroupTestPhase::Failed {
                            reason: "Verify failed: no claimed defective reproduced the crash."
                                .into(),
                        };
                    } else if index >= self.defectives.len() {
                        self.verified = true;
                        self.phase = GroupTestPhase::Done;
                    } else {
                        self.phase = GroupTestPhase::VerifyOne { index };
                    }
                }
            },
            GroupTestPhase::Done | GroupTestPhase::Failed { .. } => {}
        }
    }

    fn begin_peel(&mut self) {
        if self.covering.is_empty() {
            if self.defectives.is_empty() {
                self.phase = GroupTestPhase::Failed {
                    reason: "All suspects were clean — crash is not explained by this pool.".into(),
                };
            } else {
                self.phase = GroupTestPhase::VerifyAll;
                self.test_group.clear();
            }
            return;
        }
        let n = peel_group_size(self.covering.len());
        self.test_group = self.covering.iter().take(n.max(1)).cloned().collect();
        self.phase = GroupTestPhase::Testing;
    }

    pub fn share_plan(&self) -> Option<ActionPlan> {
        if !self.verified || self.defectives.is_empty() {
            return None;
        }
        let actions: Vec<LauncherAction> = self
            .defectives
            .iter()
            .map(|id| LauncherAction {
                op: "disable_mod".into(),
                mod_id: Some(id.clone()),
                provider: None,
                project_id: None,
                version: None,
                path: None,
                patch_type: None,
                patch: None,
                reason: Some("Isolated by group testing".into()),
                risk: "medium".into(),
            })
            .collect();
        let human_explanation = if self.defectives.len() == 1 {
            format!("Isolated by group testing: disable {}.", self.defectives[0])
        } else {
            format!(
                "Isolated by group testing: disable {}.",
                self.defectives.join(", ")
            )
        };
        Some(ActionPlan {
            schema_version: ACTION_PLAN_SCHEMA_VERSION,
            human_explanation,
            confidence: 0.85,
            suspected_mods: self.defectives.clone(),
            needs_user_review: true,
            source: Some("group_test".into()),
            matched_case_ids: Vec::new(),
            actions,
            additional_context: None,
        })
    }
}

/// Group size for the next peel/split. Half, capped by the largest power of two ≤ n.
pub fn peel_group_size(n: usize) -> usize {
    if n <= 1 {
        return n;
    }
    let half = n.div_ceil(2);
    let pow = 1usize << n.ilog2();
    half.min(pow).max(1)
}

pub fn is_protected_mod_id(id: &str) -> bool {
    let l = id.to_ascii_lowercase();
    PROTECTED_IDS
        .iter()
        .any(|p| l == *p || l.starts_with(&format!("{p}-")))
        || l.contains("fabric-api")
        || l == "neoforged"
}

/// Candidate pool: recently changed ∪ suspected, else all content mods, minus protected / required-by-kept.
pub fn candidate_pool(
    mods: &[ModSpec],
    recently_changed: &[String],
    suspected: &[String],
) -> Vec<String> {
    let content: Vec<&ModSpec> = mods
        .iter()
        .filter(|m| m.content_type == ContentType::Mod)
        .filter(|m| !is_protected_mod_id(&m.id))
        .collect();
    let mut ids: Vec<String> = Vec::new();
    let push_if = |ids: &mut Vec<String>, id: &str| {
        if content.iter().any(|m| m.id == id) && !ids.iter().any(|x| x == id) {
            ids.push(id.to_string());
        }
    };
    for id in recently_changed.iter().chain(suspected.iter()) {
        push_if(&mut ids, id);
    }
    if ids.is_empty() {
        ids = content.iter().map(|m| m.id.clone()).collect();
    }
    let pool_set: std::collections::HashSet<&str> = ids.iter().map(|s| s.as_str()).collect();
    let required_by_kept: std::collections::HashSet<String> = mods
        .iter()
        .filter(|m| !pool_set.contains(m.id.as_str()))
        .flat_map(|m| {
            m.dependencies.iter().filter_map(|d| {
                if d.kind == DependencyKind::Requires {
                    Some(d.target.clone())
                } else {
                    None
                }
            })
        })
        .collect();
    ids.retain(|id| !required_by_kept.contains(id) && !is_protected_mod_id(id));
    ids
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrailEventKind {
    Disable(String),
    Enable(String),
    Crash,
    Healthy,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrailEvent {
    pub kind: TrailEventKind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DecodedTrail {
    pub clean: Vec<String>,
    pub covering: Vec<String>,
    pub extra_actions: Vec<LauncherAction>,
    pub explanation: String,
    pub confidence: f64,
}

impl DecodedTrail {
    pub fn disable_actions(&self) -> Vec<LauncherAction> {
        self.covering
            .iter()
            .map(|id| LauncherAction {
                op: "disable_mod".into(),
                mod_id: Some(id.clone()),
                provider: None,
                project_id: None,
                version: None,
                path: None,
                patch_type: None,
                patch: None,
                reason: Some("Covering from group-test trail decode".into()),
                risk: "medium".into(),
            })
            .collect()
    }
}

/// COMP-style decode: healthy launch ⇒ every enabled mod is clean.
/// Covering = disabled at first healthy, minus later-proven clean.
pub fn decode_player_trail(universe: &[String], events: &[TrailEvent]) -> DecodedTrail {
    let mut disabled: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut clean: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut covering: Vec<String> = Vec::new();
    let mut saw_healthy = false;

    for ev in events {
        match &ev.kind {
            TrailEventKind::Disable(id) => {
                disabled.insert(id.clone());
            }
            TrailEventKind::Enable(id) => {
                disabled.remove(id);
            }
            TrailEventKind::Crash => {}
            TrailEventKind::Healthy => {
                let enabled: Vec<String> = universe
                    .iter()
                    .filter(|id| !disabled.contains(*id))
                    .cloned()
                    .collect();
                for id in &enabled {
                    clean.insert(id.clone());
                }
                if !saw_healthy {
                    covering = universe
                        .iter()
                        .filter(|id| disabled.contains(*id))
                        .cloned()
                        .collect();
                    saw_healthy = true;
                }
            }
            TrailEventKind::Other => {}
        }
    }
    covering.retain(|id| !clean.contains(id));
    covering.sort();
    covering.dedup();
    let mut clean_vec: Vec<String> = clean.into_iter().collect();
    clean_vec.sort();

    let (explanation, confidence) = if !saw_healthy {
        (
            "No healthy launch in the trail — cannot decode a covering.".into(),
            0.2,
        )
    } else if covering.len() == 1 {
        (
            format!("Pack launched after disabling {}.", covering[0]),
            0.7,
        )
    } else if covering.is_empty() {
        (
            "Healthy launch with the full candidate set enabled — no disable covering.".into(),
            0.4,
        )
    } else {
        (
            format!(
                "Launch succeeded with these mods disabled: {}.",
                covering.join(", ")
            ),
            0.55,
        )
    };

    DecodedTrail {
        clean: clean_vec,
        covering,
        extra_actions: Vec::new(),
        explanation,
        confidence,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(ids: &[&str]) -> GroupTestSession {
        GroupTestSession::start(ids.iter().map(|s| (*s).to_string()).collect())
    }

    #[test]
    fn peel_sizes() {
        assert_eq!(peel_group_size(1), 1);
        assert_eq!(peel_group_size(2), 1);
        assert_eq!(peel_group_size(3), 2);
        assert_eq!(peel_group_size(4), 2);
        assert_eq!(peel_group_size(8), 4);
    }

    #[test]
    fn d1_peel_isolates_first_mod() {
        // Pool A B C D. Covering healthy. Peel enables first half [A,B].
        // Crash on [A,B] → split to [A]. Crash on A → A defective.
        // Peel remaining [B,C,D] all healthy → clean. Verify A.
        let mut s = session(&["A", "B", "C", "D"]);
        assert_eq!(s.phase, GroupTestPhase::NeedCovering);
        s.apply_outcome(TestOutcome::Healthy);
        assert_eq!(s.phase, GroupTestPhase::Testing);
        assert_eq!(s.test_group, vec!["A", "B"]);
        s.apply_outcome(TestOutcome::Crash);
        assert_eq!(s.test_group, vec!["A"]);
        s.apply_outcome(TestOutcome::Crash);
        assert_eq!(s.defectives, vec!["A"]);
        // Remaining covering B,C,D — enable first half
        while matches!(s.phase, GroupTestPhase::Testing) {
            s.apply_outcome(TestOutcome::Healthy);
        }
        assert_eq!(s.phase, GroupTestPhase::VerifyAll);
        s.apply_outcome(TestOutcome::Healthy);
        assert!(matches!(s.phase, GroupTestPhase::VerifyOne { index: 0 }));
        s.apply_outcome(TestOutcome::Crash);
        assert_eq!(s.phase, GroupTestPhase::Done);
        assert!(s.verified);
        assert_eq!(s.defectives, vec!["A"]);
    }

    #[test]
    fn d2_does_not_reenable_other_half() {
        // Defectives A and C in different halves of [A,B,C,D].
        // NeedCovering healthy. Test [A,B]: crash (A). Split to [A]: crash → A isolated.
        // Covering left [B,C,D]. Test [B,C]: crash (C). Split to [B]: healthy → B clean.
        // Covering [C,D]. Test [C]: crash → C isolated.
        // Covering [D]. Test [D]: healthy → D clean.
        // Verify {A,C}.
        let mut s = session(&["A", "B", "C", "D"]);
        s.apply_outcome(TestOutcome::Healthy);
        assert_eq!(s.test_group, vec!["A", "B"]);
        s.apply_outcome(TestOutcome::Crash);
        assert_eq!(s.test_group, vec!["A"]);
        // Binary search would re-enable {A,B} after concluding culprit in {C,D}.
        // After isolating A, C must still be in covering.
        s.apply_outcome(TestOutcome::Crash);
        assert_eq!(s.defectives, vec!["A"]);
        assert!(s.covering.contains(&"C".to_string()), "{:?}", s.covering);
        assert!(!s.known_clean.contains(&"C".to_string()));

        // Next peel: covering B,C,D → test [B,C]
        assert_eq!(s.test_group, vec!["B", "C"]);
        s.apply_outcome(TestOutcome::Crash);
        assert_eq!(s.test_group, vec!["B"]);
        s.apply_outcome(TestOutcome::Healthy); // B clean
        assert!(s.known_clean.contains(&"B".to_string()));
        // covering C,D
        assert_eq!(s.test_group, vec!["C"]);
        s.apply_outcome(TestOutcome::Crash);
        assert_eq!(s.defectives, vec!["A", "C"]);
        assert_eq!(s.test_group, vec!["D"]);
        s.apply_outcome(TestOutcome::Healthy);
        assert_eq!(s.phase, GroupTestPhase::VerifyAll);
        s.apply_outcome(TestOutcome::Healthy);
        s.apply_outcome(TestOutcome::Crash); // A
        s.apply_outcome(TestOutcome::Crash); // C
        assert_eq!(s.phase, GroupTestPhase::Done);
        assert_eq!(s.defectives, vec!["A", "C"]);
        assert!(s.verified);
    }

    #[test]
    fn covering_crash_fails_pool() {
        let mut s = session(&["A", "B"]);
        s.apply_outcome(TestOutcome::Crash);
        assert!(matches!(s.phase, GroupTestPhase::Failed { .. }));
        assert!(!s.verified);
        assert!(s.share_plan().is_none());
    }

    #[test]
    fn verify_fail_does_not_share() {
        let mut s = session(&["A", "B"]);
        s.apply_outcome(TestOutcome::Healthy);
        // test [A], crash → A defective; test [B] healthy
        s.apply_outcome(TestOutcome::Crash);
        s.apply_outcome(TestOutcome::Healthy);
        assert_eq!(s.phase, GroupTestPhase::VerifyAll);
        s.apply_outcome(TestOutcome::Crash);
        assert!(matches!(s.phase, GroupTestPhase::Failed { .. }));
        assert!(!s.verified);
        assert!(s.share_plan().is_none());
    }

    #[test]
    fn decode_healthy_enabled_are_clean() {
        let universe = vec!["A".into(), "B".into(), "C".into()];
        let events = vec![
            TrailEvent {
                kind: TrailEventKind::Disable("C".into()),
            },
            TrailEvent {
                kind: TrailEventKind::Healthy,
            },
        ];
        let d = decode_player_trail(&universe, &events);
        assert!(d.clean.contains(&"A".into()));
        assert!(d.clean.contains(&"B".into()));
        assert_eq!(d.covering, vec!["C"]);
        assert!(d.explanation.contains("C"));
    }

    #[test]
    fn decode_does_not_mark_untested_half_clean() {
        // Disable 8, crash, enable first 4 / keep last 4 disabled, healthy.
        // First half was enabled at healthy → clean. Second half = covering.
        // First half is clean only because it was enabled when healthy — not because of a crash.
        let universe: Vec<String> = (1..=8).map(|i| format!("m{i}")).collect();
        let mut events = Vec::new();
        for i in 1..=8 {
            events.push(TrailEvent {
                kind: TrailEventKind::Disable(format!("m{i}")),
            });
        }
        events.push(TrailEvent {
            kind: TrailEventKind::Crash,
        });
        for i in 1..=4 {
            events.push(TrailEvent {
                kind: TrailEventKind::Enable(format!("m{i}")),
            });
        }
        events.push(TrailEvent {
            kind: TrailEventKind::Healthy,
        });
        let d = decode_player_trail(&universe, &events);
        for i in 1..=4 {
            assert!(d.clean.contains(&format!("m{i}")), "m{i} should be clean");
            assert!(!d.covering.contains(&format!("m{i}")));
        }
        for i in 5..=8 {
            assert!(
                d.covering.contains(&format!("m{i}")),
                "m{i} should stay in covering"
            );
            assert!(
                !d.clean.contains(&format!("m{i}")),
                "m{i} was never enabled on a healthy launch"
            );
        }
    }

    #[test]
    fn decode_crash_does_not_mark_disabled_half_clean() {
        // Binary-search heuristic: disable first half, crash → wrongly treat that
        // half as clean. Group-test decode: crash only proves the enabled set is
        // contaminated; disabled mods stay unproven until a healthy launch.
        let universe: Vec<String> = (1..=8).map(|i| format!("m{i}")).collect();
        let mut events = Vec::new();
        for i in 1..=4 {
            events.push(TrailEvent {
                kind: TrailEventKind::Disable(format!("m{i}")),
            });
        }
        events.push(TrailEvent {
            kind: TrailEventKind::Crash,
        });
        let d = decode_player_trail(&universe, &events);
        assert!(d.covering.is_empty());
        for i in 1..=4 {
            assert!(
                !d.clean.contains(&format!("m{i}")),
                "m{i} must not be clean after crash-only"
            );
        }
    }

    #[test]
    fn decode_cancelled_toggle_drops_from_covering() {
        let universe = vec!["A".into(), "B".into(), "C".into()];
        let events = vec![
            TrailEvent {
                kind: TrailEventKind::Disable("A".into()),
            },
            TrailEvent {
                kind: TrailEventKind::Enable("A".into()),
            },
            TrailEvent {
                kind: TrailEventKind::Disable("C".into()),
            },
            TrailEvent {
                kind: TrailEventKind::Healthy,
            },
        ];
        let d = decode_player_trail(&universe, &events);
        assert!(d.clean.contains(&"A".into()));
        assert!(!d.covering.contains(&"A".into()));
        assert_eq!(d.covering, vec!["C"]);
    }

    #[test]
    fn protected_ids() {
        assert!(is_protected_mod_id("fabric-api"));
        assert!(is_protected_mod_id("neoforge"));
        assert!(!is_protected_mod_id("sodium"));
    }
}
