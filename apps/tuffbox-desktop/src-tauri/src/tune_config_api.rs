//! Tune Config Advisor — Tauri commands + research agent loop.

use crate::integrations;
use crate::web_research::{
    fetch_page, lookup_modrinth_mod, web_search, ResearchBudget, ResearchLogEntry,
};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter};
use tuffbox_core::action_plan::ActionPlan;
use tuffbox_core::manifest::ProjectManifest;
use tuffbox_core::project_ai_inventory::collect_project_ai_inventory;
use tuffbox_core::tune_config_ai::{
    build_tune_advisor_user_message, config_actions_only, dry_run_config_diffs,
    extract_key_hints, guess_mod_from_config_path, merge_template_and_ai_actions,
    parse_tune_advise_draft, validate_tune_action_plan, ConfigPatchDiff, TuneAdviseDraft,
    TuneConfigGoal, TuneContext, TUNE_CONFIG_SYSTEM_PROMPT,
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TuneAdviseResult {
    pub plan: ActionPlan,
    pub explanation: String,
    pub research_log: Vec<ResearchLogEntry>,
    pub unknown_keys: Vec<tuffbox_core::tune_config_ai::UnknownConfigKey>,
    pub diffs: Vec<ConfigPatchDiff>,
    pub validation_ok: bool,
    pub validation_errors: Vec<String>,
    pub validation_warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<serde_json::Value>,
}

fn emit_progress(app: &AppHandle, session_id: &str, line: &str, phase: &str) {
    let _ = app.emit(
        "tune-ai-progress",
        serde_json::json!({
            "sessionId": session_id,
            "line": line,
            "phase": phase,
        }),
    );
}

fn resolve_project(path: &str) -> Result<(PathBuf, PathBuf, ProjectManifest), String> {
    let manifest_path = {
        let p = PathBuf::from(path);
        if p.is_file() {
            p
        } else {
            p.join("tuffbox.json")
        }
    };
    let project_dir = manifest_path
        .parent()
        .ok_or_else(|| "invalid project path".to_string())?
        .to_path_buf();
    let manifest = ProjectManifest::load_from_path(&manifest_path).map_err(|e| e.to_string())?;
    Ok((manifest_path, project_dir, manifest))
}

fn loader_slug(manifest: &ProjectManifest) -> String {
    tuffbox_core::graph::loader_kind_slug(&manifest.loader.kind).to_string()
}

fn build_context(
    project_dir: &Path,
    manifest: &ProjectManifest,
    focus_path: Option<&str>,
    focus_keys: &[String],
    goal: TuneConfigGoal,
) -> Result<TuneContext, String> {
    let inventory = collect_project_ai_inventory(project_dir, manifest);
    let mut ctx = TuneContext {
        minecraft_version: manifest.minecraft.version.clone(),
        loader: loader_slug(manifest),
        java_hint: None,
        inventory,
        focus_path: focus_path.map(|s| s.replace('\\', "/")),
        focus_content: None,
        focus_keys: focus_keys.to_vec(),
        key_hints: Vec::new(),
        template_actions: Vec::new(),
        research_snippets: Vec::new(),
        lint_notes: Vec::new(),
    };

    if let Some(rel) = focus_path {
        let rel = rel.replace('\\', "/");
        let fp = project_dir.join(&rel);
        if fp.is_file() {
            let content = std::fs::read_to_string(&fp).unwrap_or_default();
            ctx.key_hints = extract_key_hints(&rel, &content);
            ctx.focus_content = Some(content);
        }
    }

    // Deterministic templates for performance-ish goals
    if matches!(
        goal,
        TuneConfigGoal::FpsClient | TuneConfigGoal::CompatSafe | TuneConfigGoal::FreeText
    ) {
        let tokens = tuffbox_core::optimize_pack::inventory_tokens(manifest);
        let deny = tuffbox_core::optimize_pack::modernfix_denylist_hit(&tokens);
        let (actions, warnings) = tuffbox_core::optimize_pack::build_optimize_config_actions(
            project_dir,
            manifest,
            deny.is_empty(),
        );
        ctx.template_actions = actions;
        ctx.lint_notes.extend(warnings);
    }

    Ok(ctx)
}

async fn call_tune_llm(
    user_msg: &str,
) -> Result<(TuneAdviseDraft, Option<serde_json::Value>), String> {
    let settings = integrations::get_integration_status().settings;
    let messages = vec![serde_json::json!({"role": "user", "content": user_msg})];
    let value = integrations::call_ai_messages(
        &settings.ai,
        TUNE_CONFIG_SYSTEM_PROMPT,
        &messages,
        true,
    )
    .await?;
    let raw = if value.is_string() {
        value.as_str().unwrap_or("").to_string()
    } else {
        value.to_string()
    };
    let draft = parse_tune_advise_draft(&raw)?;
    Ok((draft, None))
}

async fn run_research_round(
    draft: &TuneAdviseDraft,
    ctx: &mut TuneContext,
    budget: &mut ResearchBudget,
    log: &mut Vec<ResearchLogEntry>,
    app: &AppHandle,
    session_id: &str,
) {
    let mut queries = draft.research_queries.clone();
    for uk in &draft.unknown_keys {
        let mod_hint = uk
            .mod_hint
            .clone()
            .or_else(|| guess_mod_from_config_path(&uk.path));
        let q = match &mod_hint {
            Some(m) => format!("{m} {} minecraft config", uk.key),
            None => format!("{} {} minecraft config", uk.path, uk.key),
        };
        if !queries.iter().any(|x| x == &q) {
            queries.push(q);
        }
        if let Some(m) = mod_hint {
            if budget.can_tool() {
                emit_progress(app, session_id, &format!("Looking up mod {m}…"), "research");
                if let Ok(snip) = lookup_modrinth_mod(&m, budget, log).await {
                    ctx.research_snippets.push(snip);
                }
            }
        }
    }

    for q in queries.into_iter().take(4) {
        if !budget.can_tool() {
            break;
        }
        emit_progress(app, session_id, &format!("Searching: {q}"), "research");
        let hits = match web_search(&q, budget, log).await {
            Ok(h) => h,
            Err(_) => continue,
        };
        let mut summary = format!("Search results for {q:?}:\n");
        for h in hits.iter().take(3) {
            summary.push_str(&format!("- {} — {}\n  {}\n", h.title, h.url, h.snippet));
        }
        ctx.research_snippets.push(summary);

        // Fetch top allowlisted hit
        if let Some(top) = hits.first() {
            if budget.can_fetch() {
                emit_progress(
                    app,
                    session_id,
                    &format!("Reading {}", top.url),
                    "research",
                );
                if let Ok(page) = fetch_page(&top.url, budget, log).await {
                    ctx.research_snippets
                        .push(format!("Page {}:\n{page}", top.url));
                }
            }
        }
    }
}

/// Primary Tune advise entry (with optional web research loop).
#[tauri::command(rename_all = "camelCase")]
pub async fn tune_config_advise(
    app: AppHandle,
    path: String,
    goal: Option<String>,
    user_message: Option<String>,
    focus_path: Option<String>,
    focus_keys: Option<Vec<String>>,
    session_id: Option<String>,
    enable_web_research: Option<bool>,
) -> Result<TuneAdviseResult, String> {
    let session = session_id.unwrap_or_else(|| "default".into());
    let goal = TuneConfigGoal::parse(goal.as_deref().unwrap_or("free_text"));
    let user_message = user_message.unwrap_or_default();
    let focus_keys = focus_keys.unwrap_or_default();

    let (_manifest_path, project_dir, manifest) = resolve_project(&path)?;
    let settings = integrations::get_integration_status().settings;
    let web_on = enable_web_research.unwrap_or(settings.ai.tune_web_research);

    emit_progress(&app, &session, "Building Tune context…", "context");
    let mut ctx = build_context(
        &project_dir,
        &manifest,
        focus_path.as_deref(),
        &focus_keys,
        goal,
    )?;

    // explain_file without LLM if no AI? still call LLM for explanation
    emit_progress(&app, &session, "Asking Config Advisor…", "llm");
    let user_msg = build_tune_advisor_user_message(goal, &user_message, &ctx);
    let (mut draft, usage) = call_tune_llm(&user_msg).await?;

    let mut log: Vec<ResearchLogEntry> = Vec::new();
    let mut budget = ResearchBudget::new(6, 4);

    if web_on
        && (!draft.unknown_keys.is_empty() || !draft.research_queries.is_empty())
        && goal != TuneConfigGoal::ExplainFile
    {
        emit_progress(&app, &session, "Researching unknown keys…", "research");
        run_research_round(&draft, &mut ctx, &mut budget, &mut log, &app, &session).await;
        emit_progress(&app, &session, "Refining plan with research…", "llm");
        let user_msg2 = build_tune_advisor_user_message(goal, &user_message, &ctx);
        if let Ok((draft2, _)) = call_tune_llm(&user_msg2).await {
            draft = draft2;
        }
    } else if !web_on
        && (!draft.unknown_keys.is_empty() || !draft.research_queries.is_empty())
    {
        log.push(ResearchLogEntry {
            step: "web_research".into(),
            detail: "Web research disabled in Settings — skipping unknown key lookup".into(),
            ok: false,
            url: None,
        });
    }

    // For fps goals, merge templates under AI
    if matches!(goal, TuneConfigGoal::FpsClient | TuneConfigGoal::CompatSafe) {
        let templates = ctx.template_actions.clone();
        draft.plan.actions =
            merge_template_and_ai_actions(templates, draft.plan.actions.clone());
    }

    if goal == TuneConfigGoal::ExplainFile {
        draft.plan.actions.clear();
    }

    draft.plan = config_actions_only(&draft.plan);
    draft.plan.source = Some("tune_config".into());
    draft.plan.needs_user_review = true;
    enrich_reasons_with_research_sources(&mut draft.plan, &log, &ctx);

    let validation = validate_tune_action_plan(&draft.plan);
    let diffs = dry_run_config_diffs(&project_dir, &draft.plan.actions);

    emit_progress(&app, &session, "Plan ready for review", "done");

    Ok(TuneAdviseResult {
        explanation: draft.plan.human_explanation.clone(),
        research_log: log,
        unknown_keys: draft.unknown_keys,
        diffs,
        validation_ok: validation.ok,
        validation_errors: validation.errors,
        validation_warnings: validation.warnings,
        usage,
        plan: draft.plan,
    })
}

fn enrich_reasons_with_research_sources(
    plan: &mut ActionPlan,
    log: &[ResearchLogEntry],
    ctx: &TuneContext,
) {
    let urls: Vec<String> = log
        .iter()
        .filter(|e| e.ok)
        .filter_map(|e| e.url.clone())
        .filter(|u| !u.contains("duckduckgo.com") && !u.contains("api.modrinth.com"))
        .collect();
    let has_templates = !ctx.template_actions.is_empty();
    let has_comments = ctx.key_hints.iter().any(|h| h.comment.is_some());
    let has_research = !ctx.research_snippets.is_empty() || !urls.is_empty();

    for a in &mut plan.actions {
        if a.op != "edit_config" {
            continue;
        }
        let reason = a.reason.get_or_insert_with(String::new);
        let lower = reason.to_ascii_lowercase();
        if !lower.contains("source:") {
            let mut tag = String::from("source: ");
            let mut parts = Vec::new();
            if has_comments {
                parts.push("local_comment");
            }
            if has_templates {
                parts.push("template");
            }
            if has_research {
                parts.push("research");
            }
            if parts.is_empty() {
                parts.push("inventory");
            }
            tag.push_str(&parts.join("+"));
            if !reason.is_empty() {
                reason.push_str(" · ");
            }
            reason.push_str(&tag);
        }
        if has_research && !urls.is_empty() && !reason.contains("http") {
            let cite: Vec<&str> = urls.iter().take(2).map(|s| s.as_str()).collect();
            reason.push_str(" · cite: ");
            reason.push_str(&cite.join(" | "));
        }
    }
}

/// Dry-run diffs for an existing ActionPlan (review UI).
#[tauri::command(rename_all = "camelCase")]
pub fn tune_config_preview_diffs(
    path: String,
    plan: ActionPlan,
) -> Result<Vec<ConfigPatchDiff>, String> {
    let (_mp, project_dir, _manifest) = resolve_project(&path)?;
    let filtered = config_actions_only(&plan);
    Ok(dry_run_config_diffs(&project_dir, &filtered.actions))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TuneChatSessionsList {
    pub sessions: Vec<tuffbox_core::tune_chat::TuneChatSession>,
    pub corrupt_skipped: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TuneChatTurnResult {
    pub session: tuffbox_core::tune_chat::TuneChatSession,
    pub advise: TuneAdviseResult,
}

#[tauri::command(rename_all = "camelCase")]
pub fn list_tune_chat_sessions(path: String) -> Result<TuneChatSessionsList, String> {
    let (_mp, project_dir, _) = resolve_project(&path)?;
    let detailed = tuffbox_core::tune_chat::list_tune_chats_detailed(&project_dir)?;
    Ok(TuneChatSessionsList {
        sessions: detailed.sessions,
        corrupt_skipped: detailed.corrupt_skipped,
    })
}

#[tauri::command(rename_all = "camelCase")]
pub fn save_tune_chat_session(
    path: String,
    session: tuffbox_core::tune_chat::TuneChatSession,
) -> Result<(), String> {
    let (_mp, project_dir, _) = resolve_project(&path)?;
    tuffbox_core::tune_chat::save_tune_chat(&project_dir, &session)?;
    Ok(())
}

#[tauri::command(rename_all = "camelCase")]
pub fn load_tune_chat_session(
    path: String,
    chat_id: String,
) -> Result<tuffbox_core::tune_chat::TuneChatSession, String> {
    let (_mp, project_dir, _) = resolve_project(&path)?;
    tuffbox_core::tune_chat::load_tune_chat(&project_dir, &chat_id)
}

#[tauri::command(rename_all = "camelCase")]
pub fn delete_tune_chat_session(path: String, chat_id: String) -> Result<(), String> {
    let (_mp, project_dir, _) = resolve_project(&path)?;
    tuffbox_core::tune_chat::delete_tune_chat(&project_dir, &chat_id)
}

#[tauri::command(rename_all = "camelCase")]
pub fn new_tune_chat_session(
    path: String,
    title: Option<String>,
) -> Result<tuffbox_core::tune_chat::TuneChatSession, String> {
    let (_mp, project_dir, _) = resolve_project(&path)?;
    let session = tuffbox_core::tune_chat::new_tune_chat_session(title.as_deref());
    tuffbox_core::tune_chat::save_tune_chat(&project_dir, &session)?;
    Ok(session)
}

/// Chat turn: persist user/assistant messages + pending advise into a TuneChatSession.
#[tauri::command(rename_all = "camelCase")]
pub async fn tune_chat_turn(
    app: AppHandle,
    path: String,
    message: String,
    chat_id: Option<String>,
    goal: Option<String>,
    focus_path: Option<String>,
    focus_keys: Option<Vec<String>>,
    enable_web_research: Option<bool>,
) -> Result<TuneChatTurnResult, String> {
    let (_mp, project_dir, _) = resolve_project(&path)?;
    let mut session = if let Some(id) = chat_id.filter(|s| !s.is_empty()) {
        tuffbox_core::tune_chat::load_tune_chat(&project_dir, &id).unwrap_or_else(|_| {
            let mut s = tuffbox_core::tune_chat::new_tune_chat_session(None);
            s.id = id;
            s
        })
    } else {
        tuffbox_core::tune_chat::new_tune_chat_session(None)
    };

    let goal_parsed = TuneConfigGoal::parse(goal.as_deref().unwrap_or("free_text"));
    let user_text = message.trim().to_string();
    let display_user = if user_text.is_empty() {
        format!("[{}]", goal_parsed.label())
    } else {
        user_text.clone()
    };

    session.messages.push(tuffbox_core::tune_chat::TuneChatMessage {
        role: "user".into(),
        content: display_user,
        created_at: Some(tuffbox_core::quest_chat::now_iso()),
    });
    if let Some(fp) = focus_path.as_ref() {
        session.focus_path = Some(fp.clone());
    }

    let advise = tune_config_advise(
        app,
        path,
        goal,
        Some(user_text),
        focus_path.or_else(|| session.focus_path.clone()),
        focus_keys,
        Some(session.id.clone()),
        enable_web_research,
    )
    .await?;

    let unknown_note = if advise.unknown_keys.is_empty() {
        String::new()
    } else {
        format!(
            "\n\nStill unknown ({}): {}",
            advise.unknown_keys.len(),
            advise
                .unknown_keys
                .iter()
                .take(5)
                .map(|k| k.key.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let research_note = if advise.research_log.is_empty() {
        String::new()
    } else {
        let ok = advise.research_log.iter().filter(|e| e.ok).count();
        format!(
            "\n\nResearch: {ok}/{} steps ok",
            advise.research_log.len()
        )
    };

    session.messages.push(tuffbox_core::tune_chat::TuneChatMessage {
        role: "assistant".into(),
        content: format!(
            "{}{}{}",
            advise.explanation, research_note, unknown_note
        ),
        created_at: Some(tuffbox_core::quest_chat::now_iso()),
    });

    // Auto-title from first user turn when still default
    if session.title == "Tune configs" || session.title.is_empty() {
        let first = session
            .messages
            .iter()
            .find(|m| m.role == "user")
            .map(|m| m.content.as_str())
            .unwrap_or("Tune configs");
        let title: String = first.chars().take(48).collect();
        session.title = title;
    }

    session.pending_advise = Some(tuffbox_core::tune_chat::TunePendingAdvise {
        plan: advise.plan.clone(),
        explanation: advise.explanation.clone(),
        research_log: advise
            .research_log
            .iter()
            .filter_map(|e| serde_json::to_value(e).ok())
            .collect(),
        unknown_keys: advise
            .unknown_keys
            .iter()
            .filter_map(|e| serde_json::to_value(e).ok())
            .collect(),
        diffs: advise
            .diffs
            .iter()
            .filter_map(|e| serde_json::to_value(e).ok())
            .collect(),
        validation_ok: advise.validation_ok,
        validation_errors: advise.validation_errors.clone(),
        validation_warnings: advise.validation_warnings.clone(),
    });
    session.updated_at = tuffbox_core::quest_chat::now_iso();
    tuffbox_core::tune_chat::save_tune_chat(&project_dir, &session)?;

    Ok(TuneChatTurnResult { session, advise })
}
