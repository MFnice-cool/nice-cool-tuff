//! L3 draft→verify assist for crash Explain (opt-in).
//!
//! Not token-level llama.cpp speculative decoding yet — that stays behind a future
//! `llamacpp-draft` cargo feature. This module is the product path: a small local
//! draft model proposes an ActionPlan JSON, then the configured main model
//! validates / rewrites it. Inference stays in the desktop process (never Fog node).

use crate::integrations::AiSettings;

/// Default Ollama tag for the local draft model (≈0.5B coder).
pub const DEFAULT_DRAFT_MODEL: &str = "qwen2.5-coder:0.5b";

#[derive(Debug, Clone, Default)]
pub struct SpeculativeMeta {
    pub used: bool,
    pub draft_model: Option<String>,
}

pub fn should_run(settings: &AiSettings) -> bool {
    if !settings.speculative_decoding {
        return false;
    }
    let draft = settings.draft_model.trim();
    if draft.is_empty() {
        return false;
    }
    draft != settings.model.trim()
}

pub fn resolve_draft_model(settings: &AiSettings) -> String {
    let draft = settings.draft_model.trim();
    if draft.is_empty() {
        DEFAULT_DRAFT_MODEL.into()
    } else {
        draft.to_string()
    }
}

/// Prompt for the draft model — same crash context, explicit "rough JSON ok".
pub fn build_draft_prompt(user_prompt: &str) -> String {
    format!(
        "{user_prompt}\n\n\
         Draft a schemaVersion 1 ActionPlan JSON quickly. \
         Prefer a short humanExplanation and a minimal actions list. \
         Return ONLY the JSON object."
    )
}

/// Prompt for the main / cloud model to validate a draft plan.
pub fn build_verify_prompt(user_prompt: &str, draft_json: &str) -> String {
    let draft = truncate_chars(draft_json.trim(), 12_000);
    format!(
        "{user_prompt}\n\n\
         ## Draft ActionPlan (local draft model — validate, fix, or rewrite)\n\
         ```json\n{draft}\n```\n\n\
         Return ONLY the final schemaVersion 1 ActionPlan JSON. \
         Prefer correcting the draft over inventing unrelated actions. \
         Keep needsUserReview honest."
    )
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_run_requires_flag_and_distinct_model() {
        let mut s = AiSettings::default();
        assert!(!should_run(&s));
        s.speculative_decoding = true;
        s.draft_model = DEFAULT_DRAFT_MODEL.into();
        s.model = "qwen2.5:7b".into();
        assert!(should_run(&s));
        s.model = DEFAULT_DRAFT_MODEL.into();
        assert!(!should_run(&s));
    }

    #[test]
    fn verify_prompt_embeds_draft() {
        let p = build_verify_prompt("crash context", r#"{"schemaVersion":1}"#);
        assert!(p.contains("Draft ActionPlan"));
        assert!(p.contains(r#""schemaVersion":1"#));
        assert!(p.contains("crash context"));
    }
}
