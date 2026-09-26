//! AI crash explanation infrastructure.
//!
//! Builds structured context from crash data for LLM consumption.
//! This module does NOT call any LLM API directly — it prepares the
//! context, prompt templates, and parsing logic that the Tauri
//! backend can use with any LLM provider (OpenAI, Anthropic, local).

use crate::crash_assistant::CrashAnalysisFinding;
use crate::crash_kb::{smart_excerpt, SimilarCaseHit};
use crate::project_ai_inventory::ProjectAiInventory;
use serde::{Deserialize, Serialize};

/// Context passed to an AI model for crash explanation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashAiContext {
    pub mc_version: String,
    pub loader: String,
    pub loader_version: String,
    pub java_version: String,
    pub os: String,
    /// Truncated list (suspects / sample) — prefer inventory for full list.
    pub installed_mods: Vec<String>,
    #[serde(default)]
    pub installed_mod_count: u32,
    pub crash_report_excerpt: String,
    pub latest_log_excerpt: String,
    pub suspected_mods: Vec<String>,
    /// Ranked culprits from Diagnose (id/name/authors/confidence/role).
    #[serde(default)]
    pub culprit_details: Vec<CrashAiCulprit>,
    pub crash_assistant_findings: Vec<CrashAiFinding>,
    pub recent_changes: Vec<String>,
    pub graph_diagnostics: Vec<String>,
    #[serde(default)]
    pub similar_cases: Vec<SimilarCaseHit>,
    #[serde(default)]
    pub fingerprint_key: String,
    #[serde(default)]
    pub report_id: Option<String>,
    /// Full project inventory (mods, packs, datapacks, configs).
    #[serde(default)]
    pub inventory: Option<ProjectAiInventory>,
    /// Live Diagnose group-test session (launcher facts, not hypotheses).
    #[serde(default)]
    pub group_test: Option<CrashAiGroupTest>,
    /// COMP-style decode of the crash→launch trail (healthy ⇒ enabled are clean).
    #[serde(default)]
    pub trail_covering: Option<CrashAiTrailCovering>,
}

/// Compact group-test snapshot for the Crash Planner prompt.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CrashAiGroupTest {
    pub phase: String,
    #[serde(default)]
    pub covering: Vec<String>,
    #[serde(default)]
    pub known_clean: Vec<String>,
    #[serde(default)]
    pub defectives: Vec<String>,
    #[serde(default)]
    pub verified: bool,
}

impl CrashAiGroupTest {
    pub fn from_session(session: &crate::mod_group_test::GroupTestSession) -> Self {
        use crate::mod_group_test::GroupTestPhase;
        let phase = match &session.phase {
            GroupTestPhase::NeedCovering => "needCovering".into(),
            GroupTestPhase::Testing => "testing".into(),
            GroupTestPhase::VerifyAll => "verifyAll".into(),
            GroupTestPhase::VerifyOne { index } => format!("verifyOne:{index}"),
            GroupTestPhase::Done => "done".into(),
            GroupTestPhase::Failed { reason } => format!("failed:{reason}"),
        };
        Self {
            phase,
            covering: session.covering.clone(),
            known_clean: session.known_clean.clone(),
            defectives: session.defectives.clone(),
            verified: session.verified,
        }
    }
}

/// Decoded disable covering from the player trail (not a guessed single root cause).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CrashAiTrailCovering {
    #[serde(default)]
    pub clean: Vec<String>,
    #[serde(default)]
    pub covering: Vec<String>,
    #[serde(default)]
    pub explanation: String,
}

impl CrashAiTrailCovering {
    pub fn from_decoded(decoded: &crate::mod_group_test::DecodedTrail) -> Self {
        Self {
            clean: decoded.clean.clone(),
            covering: decoded.covering.clone(),
            explanation: decoded.explanation.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashAiCulprit {
    pub id: String,
    pub name: String,
    pub confidence: u8,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub blame_role: String,
    #[serde(default)]
    pub match_sources: Vec<String>,
    #[serde(default)]
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashAiFinding {
    pub code: String,
    pub title: String,
    pub description: String,
    pub auto_fix: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashAiResponse {
    pub human_explanation: String,
    pub confidence: f64,
    pub suspected_mods: Vec<String>,
    pub recommended_actions: Vec<AiAction>,
    pub needs_user_review: bool,
    pub additional_context: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiAction {
    pub action_type: String, // "update", "remove", "install", "disable", "config_change"
    pub mod_id: Option<String>,
    pub description: String,
    pub risk: String, // "low", "medium", "high"
}

/// Prefer [`crate::action_plan::ACTION_PLAN_JSON_SCHEMA_HINT`] for executable plans.
/// Kept as an alias so existing call sites keep compiling.
pub const CRASH_JSON_SCHEMA_HINT: &str = crate::action_plan::ACTION_PLAN_JSON_SCHEMA_HINT;

/// Builds a structured prompt for the AI to explain a crash.
pub fn build_crash_prompt(ctx: &CrashAiContext) -> String {
    let mut p = String::new();
    p.push_str(crate::action_plan::ACTION_PLAN_SYSTEM_PROMPT);
    p.push_str("\n\n");
    p.push_str(&crash_prompt_body(ctx, CrashPromptBudget::full()));
    p
}

/// Compact prompt for small local models: no system-prompt duplication (caller
/// puts rules in the chat `system` message), no full inventory dump.
pub fn build_compact_crash_prompt(ctx: &CrashAiContext) -> String {
    crash_prompt_body(ctx, CrashPromptBudget::compact())
}

/// True when the configured provider/model should use the compact Explain prompt.
pub fn prefers_compact_crash_prompt(provider: &str, model: &str) -> bool {
    if !provider.eq_ignore_ascii_case("ollama") {
        return false;
    }
    is_small_local_model(model)
}

fn is_small_local_model(model: &str) -> bool {
    let m = model.trim().to_ascii_lowercase();
    if m.is_empty() {
        return true; // unknown Ollama model → be conservative
    }
    // Explicit small tags from Settings suggestions / common pulls.
    const SMALL: &[&str] = &[
        "llama3.2:1b",
        "llama3.2:3b",
        "llama3.2",
        "qwen2.5:0.5b",
        "qwen2.5:1.5b",
        "qwen2.5:3b",
        "gemma2:2b",
        "gemma3:1b",
        "gemma3:4b",
        "phi3:mini",
        "phi3:3.8b",
        "tinydolphin",
        "tinyllama",
    ];
    if SMALL
        .iter()
        .any(|s| m == *s || m.starts_with(&format!("{s}-")))
    {
        return true;
    }
    // Heuristic: parameter tags under 7b.
    for part in m.split([':', '-', '_', '/']) {
        if let Some(num) = part.strip_suffix('b').and_then(|n| n.parse::<f32>().ok()) {
            if num > 0.0 && num < 7.0 {
                return true;
            }
        }
    }
    false
}

struct CrashPromptBudget {
    include_full_inventory: bool,
    crash_excerpt: usize,
    log_excerpt: usize,
    similar_cases: usize,
    graph_lines: usize,
}

impl CrashPromptBudget {
    fn full() -> Self {
        Self {
            include_full_inventory: true,
            crash_excerpt: 4500,
            log_excerpt: 3200,
            similar_cases: usize::MAX,
            graph_lines: usize::MAX,
        }
    }
    fn compact() -> Self {
        Self {
            include_full_inventory: false,
            crash_excerpt: 2200,
            log_excerpt: 1800,
            similar_cases: 3,
            graph_lines: 8,
        }
    }
}

fn crash_prompt_body(ctx: &CrashAiContext, budget: CrashPromptBudget) -> String {
    let mut p = String::new();

    p.push_str("## System Context\n");
    p.push_str(&format!("- Minecraft: {}\n", ctx.mc_version));
    p.push_str(&format!(
        "- Loader: {} {}\n",
        ctx.loader, ctx.loader_version
    ));
    p.push_str(&format!("- Java: {}\n", ctx.java_version));
    p.push_str(&format!("- OS: {}\n", ctx.os));
    p.push_str(&format!(
        "- Installed mods: {} (ids listed only when relevant)\n\n",
        if ctx.installed_mod_count > 0 {
            ctx.installed_mod_count
        } else {
            ctx.installed_mods.len() as u32
        }
    ));
    if let Some(ref id) = ctx.report_id {
        p.push_str(&format!("- Crash report id: {id}\n"));
    }
    if !ctx.fingerprint_key.is_empty() {
        p.push_str(&format!("- Fingerprint: {}\n\n", ctx.fingerprint_key));
    }

    if !ctx.culprit_details.is_empty() {
        p.push_str("## Culprits (launcher diagnosis — prefer these)\n");
        for c in &ctx.culprit_details {
            let authors = if c.authors.is_empty() {
                String::new()
            } else {
                format!(" by {}", c.authors.join(", "))
            };
            p.push_str(&format!(
                "- [{}] {}{} — confidence {}%, role={}, sources=[{}]\n",
                c.id,
                c.name,
                authors,
                c.confidence,
                c.blame_role,
                c.match_sources.join(", ")
            ));
            for ev in c.evidence.iter().take(2) {
                p.push_str(&format!("  evidence: {ev}\n"));
            }
        }
        p.push('\n');
    } else if !ctx.suspected_mods.is_empty() {
        p.push_str("## Suspected Mods\n");
        for m in &ctx.suspected_mods {
            p.push_str(&format!("- {m}\n"));
        }
        p.push('\n');
    }

    if let Some(ref gt) = ctx.group_test {
        p.push_str("## Group test (launcher facts — not hypotheses)\n");
        if gt.verified {
            p.push_str(
                "Verified covering. Prefer disable_mod on each isolated defective. Do not invent a different single root cause.\n",
            );
        } else {
            p.push_str(
                "In progress. Do not blame known_clean. Do not treat the whole remaining covering as one root cause.\n",
            );
        }
        p.push_str(&format!("- phase: {}\n", gt.phase));
        p.push_str(&format!("- verified: {}\n", gt.verified));
        if !gt.defectives.is_empty() {
            p.push_str(&format!("- defectives: [{}]\n", gt.defectives.join(", ")));
        }
        if !gt.covering.is_empty() {
            p.push_str(&format!("- covering: [{}]\n", gt.covering.join(", ")));
        }
        if !gt.known_clean.is_empty() {
            p.push_str(&format!("- known_clean: [{}]\n", gt.known_clean.join(", ")));
        }
        p.push('\n');
    }

    if let Some(ref trail) = ctx.trail_covering {
        p.push_str("## Player trail covering (decoded group tests)\n");
        p.push_str(
            "Healthy launch ⇒ every enabled mod is clean. Remaining disables are a covering, not proof of a single culprit. Do not collapse covering size > 1 into one mod.\n",
        );
        if !trail.explanation.is_empty() {
            p.push_str(&format!("- {}\n", trail.explanation));
        }
        if !trail.covering.is_empty() {
            p.push_str(&format!("- covering: [{}]\n", trail.covering.join(", ")));
        }
        if !trail.clean.is_empty() {
            let shown: Vec<&String> = trail.clean.iter().take(24).collect();
            p.push_str(&format!(
                "- clean (enabled on healthy): [{}{}]\n",
                shown
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                if trail.clean.len() > 24 { ", …" } else { "" }
            ));
        }
        p.push('\n');
    }

    if !ctx.crash_assistant_findings.is_empty() {
        p.push_str("## Automated Analysis Results\n");
        for f in &ctx.crash_assistant_findings {
            p.push_str(&format!("- [{}] {}: {}\n", f.code, f.title, f.description));
            if let Some(fix) = &f.auto_fix {
                p.push_str(&format!("  Auto-fix: {fix}\n"));
            }
        }
        p.push('\n');
    }

    if !ctx.similar_cases.is_empty() {
        p.push_str("## Similar known cases (from local knowledge base)\n");
        p.push_str("Prefer these solutions when they match. Do not invent mods outside the project inventory.\n");
        for (i, c) in ctx
            .similar_cases
            .iter()
            .take(budget.similar_cases)
            .enumerate()
        {
            p.push_str(&format!(
                "{}. score={:.2} source={} key={}\n   Solution: {}\n",
                i + 1,
                c.score,
                c.source,
                c.fingerprint_key,
                c.solution
            ));
            if !c.actions.is_empty() {
                for a in &c.actions {
                    p.push_str(&format!(
                        "   Action: {} {} — {} (risk {})\n",
                        a.action_type,
                        a.mod_id.as_deref().unwrap_or("-"),
                        a.description,
                        a.risk
                    ));
                }
            }
        }
        p.push('\n');
    }

    if budget.include_full_inventory {
        if let Some(ref inv) = ctx.inventory {
            p.push_str(&crate::project_ai_inventory::format_inventory_for_prompt(
                inv, 14000,
            ));
            p.push('\n');
        }
    } else {
        p.push_str("## Relevant mod ids (compact — not full inventory)\n");
        let mut ids: Vec<String> = ctx.suspected_mods.clone();
        for c in &ctx.culprit_details {
            if !ids.iter().any(|x| x.eq_ignore_ascii_case(&c.id)) {
                ids.push(c.id.clone());
            }
        }
        if let Some(ref gt) = ctx.group_test {
            for id in gt
                .defectives
                .iter()
                .chain(gt.covering.iter())
                .chain(gt.known_clean.iter())
            {
                if !ids.iter().any(|x| x.eq_ignore_ascii_case(id)) {
                    ids.push(id.clone());
                }
            }
        }
        if let Some(ref trail) = ctx.trail_covering {
            for id in trail.covering.iter().chain(trail.clean.iter().take(12)) {
                if !ids.iter().any(|x| x.eq_ignore_ascii_case(id)) {
                    ids.push(id.clone());
                }
            }
        }
        for d in missing_dep_hints_from_graph(&ctx.graph_diagnostics) {
            if !ids.iter().any(|x| x.eq_ignore_ascii_case(&d)) {
                ids.push(d);
            }
        }
        if ids.is_empty() {
            p.push_str("(none listed — use Crash Assistant findings)\n");
        } else {
            for id in ids.iter().take(24) {
                p.push_str(&format!("- {id}\n"));
            }
        }
        p.push('\n');
    }

    if !ctx.graph_diagnostics.is_empty() {
        p.push_str("## Graph Diagnostics\n");
        for d in ctx.graph_diagnostics.iter().take(budget.graph_lines) {
            p.push_str(&format!("- {d}\n"));
        }
        p.push('\n');
    }

    if !ctx.recent_changes.is_empty() {
        p.push_str("## Recent Changes (may have caused the crash)\n");
        for c in ctx
            .recent_changes
            .iter()
            .take(if budget.include_full_inventory {
                usize::MAX
            } else {
                6
            })
        {
            p.push_str(&format!("- {c}\n"));
        }
        p.push('\n');
    }

    p.push_str("## Crash Report (excerpt)\n```\n");
    p.push_str(&truncate(
        &smart_excerpt(&ctx.crash_report_excerpt, budget.crash_excerpt),
        budget.crash_excerpt,
    ));
    p.push_str("\n```\n\n");

    p.push_str("## Latest Log (excerpt)\n```\n");
    p.push_str(&truncate(
        &smart_excerpt(&ctx.latest_log_excerpt, budget.log_excerpt),
        budget.log_excerpt,
    ));
    p.push_str("\n```\n\n");

    p.push_str("## Instructions\n");
    p.push_str(
        "Apply AI Decision making in order: (1) Understand the context from sections above, \
(2) Isolate ONE primary problem, (3) Accept the risk on every action (risk + needsUserReview + confidence), \
(4) Map decision to minimal `actions` with `op`.\n",
    );
    p.push_str(
        "Fact priority: verified group test covering > player-trail covering > culprits/KB. \
Do not invent mods outside inventory. Do not collapse a covering of size > 1 into a single root-cause mod.\n",
    );
    p.push_str(CRASH_JSON_SCHEMA_HINT);
    p.push_str(
        "\n\nFollow the system rules. Prefer `actions` with `op` fields over legacy recommended_actions.\n",
    );
    if !budget.include_full_inventory {
        p.push_str(
            "Compact mode: do not invent mods, versions, or paths. Prefer disable_mod for culprits.\n",
        );
        p.push_str(
            "Every action MUST use op in {install_mod,remove_mod,disable_mod,update_mod,reinstall_mod,edit_config} with modId and reason. Leave version null unless known from KB.\n",
        );
    }

    p
}

/// Pull likely missing-dependency mod ids from graph diagnostic lines.
pub fn missing_dep_hints_from_graph(diags: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for line in diags {
        let lower = line.to_ascii_lowercase();
        if !(lower.contains("missing") || lower.contains("requires") || lower.contains("depend")) {
            continue;
        }
        for token in line.split(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_') {
            let t = token.trim();
            if t.len() < 3 || t.len() > 64 {
                continue;
            }
            let tl = t.to_ascii_lowercase();
            if matches!(
                tl.as_str(),
                "missing"
                    | "dependency"
                    | "requires"
                    | "required"
                    | "mod"
                    | "error"
                    | "warning"
                    | "info"
                    | "graph"
                    | "null"
            ) {
                continue;
            }
            if !out.iter().any(|x: &String| x.eq_ignore_ascii_case(t)) {
                out.push(t.to_string());
            }
        }
    }
    out.into_iter().take(16).collect()
}

/// Context for post-resolution distill (user already fixed the crash).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DistillContext {
    pub fingerprint_key: String,
    pub mc_version: String,
    pub loader: String,
    pub crash_excerpt: String,
    /// Chronological user/fix attempts (may include dead ends).
    pub action_timeline: Vec<String>,
    pub resolved_summary: String,
    pub verified_by: String,
    pub final_actions_summary: Vec<String>,
}

/// Builds a prompt that asks the model to compress trial-and-error into a minimal plan.
pub fn build_distill_prompt(ctx: &DistillContext) -> String {
    let mut p = String::new();
    p.push_str(crate::action_plan::DISTILL_SYSTEM_PROMPT);
    p.push_str("\n\n## System Context\n");
    p.push_str(&format!("- Minecraft: {}\n", ctx.mc_version));
    p.push_str(&format!("- Loader: {}\n", ctx.loader));
    p.push_str(&format!("- Fingerprint: {}\n", ctx.fingerprint_key));
    p.push_str(&format!("- Verified by: {}\n\n", ctx.verified_by));

    p.push_str("## Resolution outcome\n");
    p.push_str(&format!("{}\n\n", ctx.resolved_summary));
    if !ctx.final_actions_summary.is_empty() {
        p.push_str("Recorded successful actions:\n");
        for a in &ctx.final_actions_summary {
            p.push_str(&format!("- {a}\n"));
        }
        p.push('\n');
    }

    if !ctx.action_timeline.is_empty() {
        p.push_str("## User action timeline (may include inefficient / superseded steps)\n");
        for line in &ctx.action_timeline {
            p.push_str(&format!("- {line}\n"));
        }
        p.push('\n');
    }

    if !ctx.crash_excerpt.trim().is_empty() {
        p.push_str("## Crash excerpt (scrubbed)\n```\n");
        p.push_str(&truncate(&smart_excerpt(&ctx.crash_excerpt, 2500), 2500));
        p.push_str("\n```\n\n");
    }

    p.push_str("## Instructions\n");
    p.push_str(CRASH_JSON_SCHEMA_HINT);
    p.push_str(
        "\n\nProduce the minimal ActionPlan peers should reuse. Set source to \"distill\".\n",
    );
    p
}

/// Builds a shorter prompt for quick crash triage.
pub fn build_triage_prompt(ctx: &CrashAiContext) -> String {
    format!(
        "Minecraft {} crashed on {} {} with Java {}. {} mods installed. Crash excerpt: {}",
        ctx.mc_version,
        ctx.loader,
        ctx.loader_version,
        ctx.java_version,
        if ctx.installed_mod_count > 0 {
            ctx.installed_mod_count
        } else {
            ctx.installed_mods.len() as u32
        },
        truncate(&ctx.crash_report_excerpt, 1000),
    )
}

/// Builds a prompt for mod compatibility analysis.
pub fn build_compat_prompt(mods_to_check: &[(String, String)]) -> String {
    let mut p = String::from(
        "Analyze these Minecraft mod combinations for known compatibility issues:\n\n",
    );
    for (a, b) in mods_to_check {
        p.push_str(&format!("- {a} + {b}\n"));
    }
    p.push_str("\nRespond with JSON: [{mod_a, mod_b, compatible, reason}]\n");
    p
}

/// Parses an AI response JSON into structured data.
/// Accepts both ActionPlan (`actions`/`op`) and legacy `recommended_actions`.
pub fn parse_crash_response(json_str: &str) -> Result<CrashAiResponse, String> {
    let plan = crate::action_plan::parse_action_plan(json_str)?;
    Ok(CrashAiResponse {
        human_explanation: plan.human_explanation.clone(),
        confidence: plan.confidence,
        suspected_mods: plan.suspected_mods.clone(),
        recommended_actions: crate::action_plan::plan_to_legacy_ai_actions(&plan),
        needs_user_review: plan.needs_user_review,
        additional_context: plan.additional_context,
    })
}

/// Converts CrashAssistant findings to AI-compatible format.
pub fn findings_to_ai(findings: &[CrashAnalysisFinding]) -> Vec<CrashAiFinding> {
    findings
        .iter()
        .map(|f| CrashAiFinding {
            code: f.code.clone(),
            title: f.title.clone(),
            description: f.description.clone(),
            auto_fix: f.auto_fix.clone(),
        })
        .collect()
}

fn truncate(s: &str, max_len: usize) -> String {
    let cut = crate::crash_kb::truncate_at_char_boundary(s, max_len);
    if cut.len() == s.len() {
        return s.to_string();
    }
    format!("{cut}... (truncated)")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_compact_prompt_without_system_dup() {
        let ctx = CrashAiContext {
            mc_version: "1.20.1".into(),
            loader: "fabric".into(),
            loader_version: "0.15".into(),
            java_version: "17".into(),
            os: "Windows 11".into(),
            installed_mods: vec!["sodium".into()],
            installed_mod_count: 2,
            crash_report_excerpt: "NoClassDefFoundError".into(),
            latest_log_excerpt: "fail".into(),
            suspected_mods: vec!["sodium".into()],
            culprit_details: vec![],
            crash_assistant_findings: vec![],
            recent_changes: vec![],
            graph_diagnostics: vec!["[Error] MissingDependency: sodium requires indium".into()],
            similar_cases: vec![],
            fingerprint_key: "test".into(),
            report_id: None,
            inventory: None,
            group_test: None,
            trail_covering: None,
        };
        let prompt = build_compact_crash_prompt(&ctx);
        assert!(!prompt.starts_with("You are TuffBox"));
        assert!(prompt.contains("Relevant mod ids") || prompt.contains("indium"));
        assert!(prefers_compact_crash_prompt("ollama", "llama3.2:3b"));
        assert!(!prefers_compact_crash_prompt("ollama", "qwen2.5:7b"));
        assert!(!prefers_compact_crash_prompt(
            "openai-compatible",
            "gpt-4o-mini"
        ));
    }

    #[test]
    fn builds_prompt() {
        let ctx = CrashAiContext {
            mc_version: "1.20.1".into(),
            loader: "fabric".into(),
            loader_version: "0.15".into(),
            java_version: "17".into(),
            os: "Windows 11".into(),
            installed_mods: vec!["sodium".into()],
            installed_mod_count: 2,
            crash_report_excerpt: "NoClassDefFoundError: com/example/Foo".into(),
            latest_log_excerpt: "Mixin apply failed".into(),
            suspected_mods: vec!["sodium".into()],
            culprit_details: vec![CrashAiCulprit {
                id: "sodium".into(),
                name: "Sodium".into(),
                confidence: 96,
                authors: vec!["JellySquid".into()],
                blame_role: "primary".into(),
                match_sources: vec!["mod_file".into()],
                evidence: vec!["Mod File: sodium.jar".into()],
            }],
            crash_assistant_findings: vec![],
            recent_changes: vec!["Added iris 1.7.0".into()],
            graph_diagnostics: vec!["Missing dependency: indium".into()],
            similar_cases: vec![],
            fingerprint_key: "test".into(),
            report_id: Some("crash-2024".into()),
            inventory: None,
            group_test: None,
            trail_covering: None,
        };
        let prompt = build_crash_prompt(&ctx);
        assert!(prompt.contains("iris"));
        assert!(prompt.contains("Mixin"));
        assert!(prompt.contains("Culprits") || prompt.contains("JellySquid"));
        assert!(prompt.contains("humanExplanation") || prompt.contains("schemaVersion"));
        // Decision framework must be present in the canon system prompt.
        assert!(prompt.contains("Understand the context"));
        assert!(prompt.contains("Isolate the problem"));
        assert!(prompt.contains("Accept the risk"));
        assert!(prompt.contains("Map decision"));
        assert!(prompt.contains("verified group test"));
    }

    #[test]
    fn prompt_includes_group_test_and_trail_covering_not_toggle_spam() {
        let ctx = CrashAiContext {
            mc_version: "1.21.1".into(),
            loader: "neoforge".into(),
            loader_version: "21".into(),
            java_version: "21".into(),
            os: "Windows".into(),
            installed_mods: vec!["foo".into(), "bar".into(), "baz".into()],
            installed_mod_count: 3,
            crash_report_excerpt: "java.lang.Error".into(),
            latest_log_excerpt: "crash".into(),
            suspected_mods: vec!["foo".into()],
            culprit_details: vec![],
            crash_assistant_findings: vec![],
            recent_changes: vec!["Updated sodium".into()],
            graph_diagnostics: vec![],
            similar_cases: vec![],
            fingerprint_key: "fp".into(),
            report_id: None,
            inventory: None,
            group_test: Some(CrashAiGroupTest {
                phase: "done".into(),
                covering: vec![],
                known_clean: vec!["bar".into()],
                defectives: vec!["foo".into(), "baz".into()],
                verified: true,
            }),
            trail_covering: Some(CrashAiTrailCovering {
                clean: vec!["bar".into()],
                covering: vec!["foo".into(), "baz".into()],
                explanation: "Launch succeeded with these mods disabled: foo, baz.".into(),
            }),
        };
        let prompt = build_crash_prompt(&ctx);
        assert!(prompt.contains("defectives: [foo, baz]"), "{prompt}");
        assert!(prompt.contains("covering: [foo, baz]"), "{prompt}");
        assert!(prompt.contains("known_clean: [bar]"));
        assert!(prompt.contains("Verified covering"));
        assert!(prompt.contains("Player trail covering"));
        assert!(prompt.contains("Updated sodium"));
        assert!(!prompt.contains("Enable foo"));
        assert!(!prompt.contains("Disable bar"));
        assert!(prompt.contains("Do not collapse a covering"));
    }

    #[test]
    fn builds_distill_prompt() {
        let ctx = DistillContext {
            fingerprint_key: "Mixin||||1.20|fabric".into(),
            mc_version: "1.20.1".into(),
            loader: "fabric".into(),
            crash_excerpt: "MixinTransformerError".into(),
            action_timeline: vec![
                "[FIX] Disabled iris".into(),
                "[FIX] Updated sodium".into(),
                "[RESOLVED] Updated sodium".into(),
            ],
            resolved_summary: "Sodium update fixed the mixin crash".into(),
            verified_by: "successful_launch".into(),
            final_actions_summary: vec!["Updated sodium".into()],
        };
        let prompt = build_distill_prompt(&ctx);
        assert!(prompt.contains("Distiller") || prompt.contains("distill"));
        assert!(prompt.contains("Disabled iris"));
        assert!(prompt.contains("source to \"distill\"") || prompt.contains("distill"));
    }

    #[test]
    fn parses_response() {
        let json = r#"{"human_explanation":"Test","confidence":0.8,"suspected_mods":["sodium"],"recommended_actions":[{"action_type":"install","mod_id":"indium","description":"Install Indium","risk":"low"}],"needs_user_review":true}"#;
        let r = parse_crash_response(json).unwrap();
        assert_eq!(r.human_explanation, "Test");
        assert_eq!(r.suspected_mods, vec!["sodium"]);
    }
}
