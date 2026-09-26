//! Quest AI chat sessions + multi-pass quest-line generation.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use once_cell::sync::Lazy;
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tuffbox_core::{
    auto_layout_plan, build_branch_user_message, build_lore_user_message, build_outline_user_message,
    delete_quest_chat, detect_target_chapter_count, detect_target_quest_count, fill_template_lore,
    filter_plan_selection, ground_items_in_plan, list_quest_chats_detailed, load_quest_chat,
    merge_quest_plan, merge_quest_plan_strict, new_quest_chat_id, now_iso, parse_quest_plan,
    pin_target_chapter,
    quest_plan_has_quests, save_quest_chat, stitch_extend_plan, stitch_lore_into_plan,
    try_heuristic_quest_plan,
    AnchorQuest, ExistingChapter, ExistingGroup, QuestAuthorContext, QuestChatMessage,
    QuestChatSession, AiTokenUsage, QuestPlan, QuestPlanMergeResult, QUEST_LORE_SYSTEM_PROMPT,
    QUEST_OUTLINE_SYSTEM_PROMPT, QUEST_PLAN_SYSTEM_PROMPT,
};

use crate::helpers::QUEST_IO_LOCK;
use crate::integrations;
use crate::manifest_parent;

static QUEST_AI_CANCEL: AtomicBool = AtomicBool::new(false);
static QUEST_AI_LAST_TURN: Lazy<Mutex<Option<Instant>>> = Lazy::new(|| Mutex::new(None));
const QUEST_AI_MIN_INTERVAL: Duration = Duration::from_secs(1);
const HISTORY_CHAR_BUDGET: usize = 6000;
const HISTORY_MAX_MESSAGES: usize = 8;

#[tauri::command(rename_all = "camelCase")]
pub fn cancel_quest_chat_turn() -> Result<(), String> {
    QUEST_AI_CANCEL.store(true, Ordering::SeqCst);
    Ok(())
}

fn clear_quest_ai_cancel() {
    QUEST_AI_CANCEL.store(false, Ordering::SeqCst);
}

fn check_quest_ai_cancel(progress: &ProgressSink<'_>, log: &mut Vec<String>) -> Result<(), String> {
    if QUEST_AI_CANCEL.load(Ordering::SeqCst) {
        progress.push(log, "cancel", "Cancelled");
        return Err("Cancelled".into());
    }
    Ok(())
}

fn enforce_quest_ai_cooldown() -> Result<(), String> {
    let mut guard = QUEST_AI_LAST_TURN
        .lock()
        .map_err(|_| "quest AI cooldown lock poisoned".to_string())?;
    if let Some(prev) = *guard {
        let elapsed = prev.elapsed();
        if elapsed < QUEST_AI_MIN_INTERVAL {
            let wait = QUEST_AI_MIN_INTERVAL - elapsed;
            return Err(format!(
                "Please wait {}ms before another Quest AI request",
                wait.as_millis()
            ));
        }
    }
    *guard = Some(Instant::now());
    Ok(())
}

fn history_for_ai(history: &[QuestChatMessage]) -> Vec<QuestChatMessage> {
    let mut out = Vec::new();
    let mut chars = 0usize;
    for m in history.iter().rev() {
        if m.role != "user" && m.role != "assistant" {
            continue;
        }
        let len = m.content.chars().count();
        if out.len() >= HISTORY_MAX_MESSAGES || chars.saturating_add(len) > HISTORY_CHAR_BUDGET {
            break;
        }
        chars = chars.saturating_add(len);
        out.push(m.clone());
    }
    out.reverse();
    out
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct QuestAiProgressEvent {
    chat_id: String,
    line: String,
    phase: String,
    i: Option<usize>,
    n: Option<usize>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct QuestAiTokenEvent {
    chat_id: String,
    text: String,
    phase: String,
}

pub(crate) struct ProgressSink<'a> {

    app: Option<&'a AppHandle>,
    chat_id: String,
}

impl ProgressSink<'_> {
    fn emit(&self, phase: &str, line: &str, i: Option<usize>, n: Option<usize>) {
        if let Some(app) = self.app {
            let _ = app.emit(
                "quest-ai-progress",
                QuestAiProgressEvent {
                    chat_id: self.chat_id.clone(),
                    line: line.to_string(),
                    phase: phase.to_string(),
                    i,
                    n,
                },
            );
        }
    }

    fn emit_token(&self, phase: &str, text: &str) {
        if text.is_empty() {
            return;
        }
        if let Some(app) = self.app {
            let _ = app.emit(
                "quest-ai-token",
                QuestAiTokenEvent {
                    chat_id: self.chat_id.clone(),
                    text: text.to_string(),
                    phase: phase.to_string(),
                },
            );
        }
    }

    fn push(&self, log: &mut Vec<String>, phase: &str, line: impl Into<String>) {
        let line = line.into();
        self.emit(phase, &line, None, None);
        log.push(line);
    }

    fn push_n(
        &self,
        log: &mut Vec<String>,
        phase: &str,
        line: impl Into<String>,
        i: usize,
        n: usize,
    ) {
        let line = line.into();
        self.emit(phase, &line, Some(i), Some(n));
        log.push(line);
    }
}

fn project_dir(path: &str) -> Result<PathBuf, String> {
    manifest_parent(path)
}

fn collect_items(path: &str) -> (Vec<String>, Option<String>) {
    let Ok(manifest_path) = crate::resolve_manifest_path(path) else {
        return (
            Vec::new(),
            Some("item catalog: could not resolve project manifest".into()),
        );
    };
    let ids = crate::collect_catalog_item_ids_click_path(&manifest_path);
    if ids.is_empty() {
        (Vec::new(), None)
    } else {
        (ids.into_iter().take(120).collect(), None)
    }
}

fn author_ctx(
    path: &str,
    book: &tuffbox_core::unified::QuestBook,
    pack_hint: Option<String>,
) -> (QuestAuthorContext, Option<String>) {
    let mut lore: Vec<(String, String, String)> = Vec::new();
    let mut lore_chars = 0usize;
    const LORE_CHAR_BUDGET: usize = 3500;
    for c in &book.chapters {
        for q in &c.quests {
            if lore.len() >= 24 || lore_chars >= LORE_CHAR_BUDGET {
                break;
            }
            let desc = q
                .description
                .iter()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .take(3)
                .collect::<Vec<_>>()
                .join(" / ");
            if desc.is_empty() {
                continue;
            }
            let truncated = truncate_chars(&desc, 180);
            lore_chars += truncated.len();
            lore.push((q.id.clone(), q.title.clone(), truncated));
        }
    }

    let resolved_hint = pack_hint.or_else(|| {
        let manifest_path = crate::resolve_manifest_path(path).ok()?;
        let manifest = tuffbox_core::ProjectManifest::load_from_path(&manifest_path).ok()?;
        let brief = manifest.brief?;
        let mut parts: Vec<String> = Vec::new();
        if let Some(title) = &book.title {
            parts.push(title.clone());
        }
        if !brief.goal.is_empty() {
            parts.push(format!("Goal: {}", brief.goal));
        }
        if !brief.target_audience.is_empty() {
            parts.push(format!("Audience: {}", brief.target_audience));
        }
        if !brief.gameplay_pillars.is_empty() {
            parts.push(format!("Pillars: {}", brief.gameplay_pillars.join(", ")));
        }
        if !brief.constraints.is_empty() {
            parts.push(format!("Constraints: {}", brief.constraints.join(", ")));
        }
        if parts.is_empty() {
            None
        } else {
            Some(parts.join(" | "))
        }
    });

    let (sample_items, items_warn) = collect_items(path);

    (
        QuestAuthorContext {
            existing_chapters: book
                .chapters
                .iter()
                .map(|c| ExistingChapter {
                    id: c.id.clone(),
                    title: c.title.clone(),
                    group: c.group.clone(),
                })
                .collect(),
            existing_groups: book
                .chapter_groups
                .iter()
                .map(|g| ExistingGroup {
                    id: g.id.clone(),
                    title: g.title.clone(),
                })
                .collect(),
            sample_items,
            pack_hint: resolved_hint,
            existing_quests: book
                .chapters
                .iter()
                .flat_map(|c| {
                    c.quests
                        .iter()
                        .map(|q| (q.id.clone(), q.title.clone()))
                })
                .take(60)
                .collect(),
            existing_quest_lore: lore,
            anchor_quest: None,
            target_chapter: None,
        },
        items_warn,
    )
}

async fn ai_json(
    system: &str,
    user: &str,
    history: &[QuestChatMessage],
) -> Result<(serde_json::Value, Option<AiTokenUsage>), String> {
    let settings = integrations::get_integration_status().settings;
    let capped = history_for_ai(history);
    let mut messages = Vec::new();
    for m in &capped {
        messages.push(serde_json::json!({
            "role": m.role,
            "content": m.content,
        }));
    }
    messages.push(serde_json::json!({"role": "user", "content": user}));
    integrations::call_ai_messages_with_usage(&settings.ai, system, &messages, true, None).await
}

async fn ai_json_stream(
    system: &str,
    user: &str,
    history: &[QuestChatMessage],
    progress: &ProgressSink<'_>,
    phase: &str,
) -> Result<(serde_json::Value, Option<AiTokenUsage>), String> {
    let settings = integrations::get_integration_status().settings;
    let capped = history_for_ai(history);
    let mut messages = Vec::new();
    for m in &capped {
        messages.push(serde_json::json!({
            "role": m.role,
            "content": m.content,
        }));
    }
    messages.push(serde_json::json!({"role": "user", "content": user}));
    integrations::call_ai_messages_stream(&settings.ai, system, &messages, true, |delta| {
        progress.emit_token(phase, delta);
    })
    .await
}

fn merge_usage(into: &mut AiTokenUsage, extra: Option<AiTokenUsage>) {
    if let Some(u) = extra {
        into.merge_in(&u);
    }
}

fn value_to_raw(value: serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s,
        other => other.to_string(),
    }
}

fn split_quest_raw_error(err: &str) -> (String, Option<String>) {
    const START: &str = "<<<QUEST_RAW_JSON>>>";
    const END: &str = "<<<END_QUEST_RAW_JSON>>>";
    if let Some(i) = err.find(START) {
        let after = &err[i + START.len()..];
        let after = after.strip_prefix('\n').unwrap_or(after);
        if let Some(j) = after.find(END) {
            let raw = after[..j].trim().to_string();
            let short = err[..i].trim().to_string();
            return (short, if raw.is_empty() { None } else { Some(raw) });
        }
    }
    (err.to_string(), None)
}

/// Outline AI call with one repair retry on invalid QuestPlan JSON.
/// On final failure the error includes `<<<QUEST_RAW_JSON>>>…<<<END_QUEST_RAW_JSON>>>`
/// so the UI / session can keep the model output.
async fn ai_quest_plan(
    system: &str,
    user: &str,
    history: &[QuestChatMessage],
    progress: Option<&ProgressSink<'_>>,
    usage_acc: &mut AiTokenUsage,
) -> Result<QuestPlan, String> {
    let (value, usage) = if let Some(p) = progress {
        ai_json_stream(system, user, history, p, "outline").await?
    } else {
        ai_json(system, user, history).await?
    };
    merge_usage(usage_acc, usage);
    let raw1 = value_to_raw(value);
    let first_parse = parse_quest_plan(&raw1);
    let first_err = match first_parse {
        Ok(plan) if quest_plan_has_quests(&plan) => return Ok(plan),
        Ok(_) => {
            "chapters contain no quests — each chapter MUST include a non-empty \"quests\" array of quest objects"
                .to_string()
        }
        Err(e) => e,
    };

    let repair = format!(
        "{user}\n\nYour previous answer was invalid QuestPlan JSON ({first_err}).\n\
Return ONLY compact JSON. desc/deps/tasks/rewards MUST be arrays (never plain strings).\n\
Every chapter MUST contain a non-empty \"quests\" array of quest objects ({{\"title\", \"tasks\": [...] }}) — never a top-level \"quests\" list beside chapters, and never empty chapters.\n\
Example: {{\"why\": \"early game\", \"chapters\": [{{\"title\": \"Chapter 1\", \"quests\": [{{\"title\": \"First Quest\", \"desc\": [\"Collect 10 oak wood\"], \"tasks\": [{{\"type\": \"item\", \"item\": \"minecraft:oak_log\", \"count\": 10}}], \"rewards\": [{{\"type\": \"xp\", \"xp\": 10}}]}}]}}.\n\
Omit ids/schemaVersion. No markdown fences."
    );
    let (value2, usage2) = if let Some(p) = progress {
        ai_json_stream(system, &repair, history, p, "outline").await?
    } else {
        ai_json(system, &repair, history).await?
    };
    merge_usage(usage_acc, usage2);
    let raw2 = value_to_raw(value2);
    match parse_quest_plan(&raw2) {
        Ok(plan) if quest_plan_has_quests(&plan) => Ok(plan),
        Ok(_) => Err(format!(
            "QuestPlan has chapters but no quests after retry\n<<<QUEST_RAW_JSON>>>\n{raw2}\n<<<END_QUEST_RAW_JSON>>>"
        )),
        Err(e) => Err(format!(
            "Invalid QuestPlan JSON after retry: {e}\n<<<QUEST_RAW_JSON>>>\n{raw2}\n<<<END_QUEST_RAW_JSON>>>"
        )),
    }
}

/// Multi-pass: outline → lore chunks → ground items → layout.
pub async fn run_generate_quest_line(
    path: &str,
    prompt: &str,
    book: &tuffbox_core::unified::QuestBook,
    history: &[QuestChatMessage],
    force_ai: bool,
    intent: Option<&str>,
    pending_plan: Option<&QuestPlan>,
    anchor_quest_id: Option<&str>,
    target_chapter_id: Option<&str>,
    progress: ProgressSink<'_>,
) -> Result<(QuestPlan, Vec<String>, AiTokenUsage), String> {
    let mut log = Vec::new();
    let mut usage_acc = AiTokenUsage::default();
    let intent = intent.unwrap_or("generate");
    let (mut ctx, items_warn) = author_ctx(path, book, None);
    let target = detect_target_quest_count(prompt);
    let chapter_target = detect_target_chapter_count(prompt);
    progress.push(&mut log, "init", format!("Target quest count ≈ {target}"));
    if let Some(c) = chapter_target {
        progress.push(&mut log, "init", format!("Target chapter count ≈ {c}"));
    }
    if let Some(w) = items_warn {
        progress.push(&mut log, "init", w);
    }
    check_quest_ai_cancel(&progress, &mut log)?;

    if let Some(tid) = target_chapter_id.filter(|s| !s.is_empty()) {
        if let Some(ch) = book.chapters.iter().find(|c| c.id == tid) {
            ctx.target_chapter = Some(ExistingChapter {
                id: ch.id.clone(),
                title: ch.title.clone(),
                group: ch.group.clone(),
            });
            progress.push(
                &mut log,
                "init",
                format!("Target chapter: {} ({})", ch.title, ch.id),
            );
        }
    }

    if intent == "branch" {
        let aid = anchor_quest_id
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                "Branch requires a selected quest as anchor — select a quest on the canvas first."
                    .to_string()
            })?;
        let found = book.chapters.iter().find_map(|c| {
            c.quests.iter().find(|q| q.id == aid).map(|q| AnchorQuest {
                id: q.id.clone(),
                title: q.title.clone(),
                chapter_title: Some(c.title.clone()),
            })
        });
        let aq = found.ok_or_else(|| {
            format!(
                "Branch anchor quest id `{aid}` was not found in the loaded quest book. \
                 If the quest is new or uncommitted, save the chapter first, then retry."
            )
        })?;
        progress.push(
            &mut log,
            "branch",
            format!("Branch anchor resolved: {} ({})", aq.title, aq.id),
        );
        ctx.anchor_quest = Some(aq);
    }

    if intent == "extend" && pending_plan.is_none() {
        return Err(
            "Nothing to extend — generate a quest line first (or load a chat with a pending plan)."
                .into(),
        );
    }

    progress.push(&mut log, "outline", "Outline: starting…");
    let mut plan = if intent == "branch" {
        progress.push(&mut log, "outline", "Outline: AI branch…");
        let user = build_branch_user_message(prompt, &ctx, target)?;
        ai_quest_plan(
            QUEST_OUTLINE_SYSTEM_PROMPT,
            &user,
            history,
            Some(&progress),
            &mut usage_acc,
        )
        .await?
    } else if !force_ai && intent == "generate" {
        if let Some(p) = try_heuristic_quest_plan(prompt) {
            progress.push(&mut log, "outline", "Outline: offline heuristic");
            p
        } else {
            progress.push(&mut log, "outline", "Outline: AI…");
            let user = build_outline_user_message(prompt, &ctx, target);
            ai_quest_plan(
                QUEST_OUTLINE_SYSTEM_PROMPT,
                &user,
                history,
                Some(&progress),
                &mut usage_acc,
            )
            .await?
        }
    } else if intent == "generate" || intent == "extend" {
        progress.push(
            &mut log,
            "outline",
            if intent == "extend" {
                "Outline: AI extend…"
            } else {
                "Outline: AI…"
            },
        );
        let user = if intent == "extend" {
            let pending_json = pending_plan
                .and_then(|p| serde_json::to_string_pretty(p).ok())
                .unwrap_or_else(|| "(no pending plan yet)".into());
            format!(
                "{prompt}\n\nExtend / append about {target} additional quests to the existing pending plan below. Keep prior quests; add new ones with dependencies.\n\nPending plan JSON:\n{pending_json}"
            )
        } else {
            build_outline_user_message(prompt, &ctx, target)
        };
        ai_quest_plan(
            QUEST_OUTLINE_SYSTEM_PROMPT,
            &user,
            history,
            Some(&progress),
            &mut usage_acc,
        )
        .await?
    } else {
        return Err(format!("unknown intent: {intent}"));
    };
    progress.push(&mut log, "outline", "Outline: done");
    check_quest_ai_cancel(&progress, &mut log)?;

    // Pin single-chapter generate onto the editor's current chapter unless multi-chapter was requested.
    if intent == "generate" {
        let multi = chapter_target.map(|c| c > 1).unwrap_or(false);
        if !multi {
            if let Some(target_ch) = ctx.target_chapter.clone() {
                pin_target_chapter(&mut plan, &target_ch);
                progress.push(
                    &mut log,
                    "outline",
                    format!("Pinned outline to chapter {}", target_ch.id),
                );
            }
        }
    }

    if intent == "extend" {
        if let Some(pending) = pending_plan {
            let (stitched, notes) = stitch_extend_plan(pending, plan);
            plan = stitched;
            for n in notes {
                progress.push(&mut log, "extend", n);
            }
        }
    }

    plan.source = Some(if plan.source.as_deref() == Some("heuristic") {
        "heuristic+multipass".into()
    } else if intent == "extend" {
        "ai-multipass-extend".into()
    } else {
        "ai-multipass".into()
    });

    // Lore pass in chunks of 6
    let indices: Vec<(usize, usize)> = plan
        .chapters
        .iter()
        .enumerate()
        .flat_map(|(ci, ch)| (0..ch.quests.len()).map(move |qi| (ci, qi)))
        .collect();
    let chunk_size = 6usize;
    let total_chunks = indices.chunks(chunk_size).count().max(1);
    let mut chunk_i = 0usize;
    for chunk in indices.chunks(chunk_size) {
        check_quest_ai_cancel(&progress, &mut log)?;
        chunk_i += 1;
        progress.push_n(
            &mut log,
            "lore",
            format!("Lore {chunk_i}/{total_chunks}…"),
            chunk_i,
            total_chunks,
        );
        let user = build_lore_user_message(&plan, chunk);
        match ai_json(QUEST_LORE_SYSTEM_PROMPT, &user, &[]).await {
            Ok((value, usage)) => {
                merge_usage(&mut usage_acc, usage);
                match stitch_lore_into_plan(&mut plan, &value_to_raw(value)) {
                    Ok(n) => progress.push(
                        &mut log,
                        "lore",
                        format!("Lore chunk updated {n} quest(s)"),
                    ),
                    Err(e) => progress.push(&mut log, "lore", format!("Lore stitch skip: {e}")),
                }
            }
            Err(e) => {
                progress.push(
                    &mut log,
                    "lore",
                    format!("Lore AI unavailable ({e}) — template fill later"),
                );
            }
        }
    }

    check_quest_ai_cancel(&progress, &mut log)?;
    fill_template_lore(&mut plan);
    progress.push(&mut log, "lore", "Lore: ensured ≥2 description lines");

    check_quest_ai_cancel(&progress, &mut log)?;
    let ground_notes = ground_items_in_plan(&mut plan, &ctx.sample_items);
    for n in ground_notes.iter().take(12) {
        progress.push(&mut log, "ground", n.clone());
    }
    progress.push(
        &mut log,
        "ground",
        format!("Grounding: {} note(s)", ground_notes.len()),
    );

    check_quest_ai_cancel(&progress, &mut log)?;
    if auto_layout_plan(&mut plan) {
        progress.push(&mut log, "layout", "Layout: DAG auto-layout applied");
    } else {
        progress.push(&mut log, "layout", "Layout: skipped — coords present");
    }

    if plan.human_explanation.trim().is_empty() {
        plan.human_explanation =
            format!("Generated quest line (~{target} quests) from author request.");
    }

    Ok((plan, log, usage_acc))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestChatTurnResult {
    pub session: QuestChatSession,
    pub merge: QuestPlanMergeResult,
    pub progress_log: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<AiTokenUsage>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestChatSessionsList {
    pub sessions: Vec<QuestChatSession>,
    pub corrupt_skipped: u32,
}

#[tauri::command(rename_all = "camelCase")]
pub fn list_quest_chat_sessions(path: String) -> Result<QuestChatSessionsList, String> {
    let dir = project_dir(&path)?;
    let detailed = list_quest_chats_detailed(&dir)?;
    Ok(QuestChatSessionsList {
        sessions: detailed.sessions,
        corrupt_skipped: detailed.corrupt_skipped,
    })
}

#[tauri::command(rename_all = "camelCase")]
pub fn save_quest_chat_session(path: String, session: QuestChatSession) -> Result<(), String> {
    let _guard = QUEST_IO_LOCK
        .lock()
        .map_err(|_| "quest I/O lock poisoned".to_string())?;
    let dir = project_dir(&path)?;
    save_quest_chat(&dir, &session)?;
    Ok(())
}

#[tauri::command(rename_all = "camelCase")]
pub fn load_quest_chat_session(path: String, chat_id: String) -> Result<QuestChatSession, String> {
    let _guard = QUEST_IO_LOCK
        .lock()
        .map_err(|_| "quest I/O lock poisoned".to_string())?;
    let dir = project_dir(&path)?;
    load_quest_chat(&dir, &chat_id)
}

#[tauri::command(rename_all = "camelCase")]
pub fn delete_quest_chat_session(path: String, chat_id: String) -> Result<(), String> {
    let _guard = QUEST_IO_LOCK
        .lock()
        .map_err(|_| "quest I/O lock poisoned".to_string())?;
    let dir = project_dir(&path)?;
    delete_quest_chat(&dir, &chat_id)
}

#[tauri::command(rename_all = "camelCase")]
pub fn new_quest_chat_session(
    path: String,
    title: Option<String>,
) -> Result<QuestChatSession, String> {
    let _guard = QUEST_IO_LOCK
        .lock()
        .map_err(|_| "quest I/O lock poisoned".to_string())?;
    let dir = project_dir(&path)?;
    let session = QuestChatSession {
        id: new_quest_chat_id(),
        title: title.unwrap_or_else(|| "Quest line".into()),
        messages: vec![],
        pending_plan: None,
        updated_at: now_iso(),
    };
    save_quest_chat(&dir, &session)?;
    Ok(session)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn quest_chat_turn(
    app: AppHandle,
    path: String,
    chat_id: Option<String>,
    message: String,
    force_ai: Option<bool>,
    intent: Option<String>,
    anchor_quest_id: Option<String>,
    target_chapter_id: Option<String>,
) -> Result<QuestChatTurnResult, String> {
    let message = message.trim().to_string();
    if message.is_empty() {
        return Err("Empty message".into());
    }
    clear_quest_ai_cancel();
    enforce_quest_ai_cooldown()?;
    let project_dir = project_dir(&path)?;
    let project_dir_load = project_dir.clone();
    let chat_id_load = chat_id.clone();
    let title_src = message.clone();
    let (book, mut session) = tokio::task::spawn_blocking(move || {
        let _guard = QUEST_IO_LOCK
            .lock()
            .map_err(|_| "quest I/O lock poisoned".to_string())?;
        let book = tuffbox_core::unified::QuestBook::load_from_project(&project_dir_load)?;
        let session = if let Some(id) = chat_id_load.filter(|s| !s.is_empty()) {
            load_quest_chat(&project_dir_load, &id).unwrap_or_else(|_| QuestChatSession {
                id: new_quest_chat_id(),
                title: truncate_title(&title_src),
                messages: vec![],
                pending_plan: None,
                updated_at: now_iso(),
            })
        } else {
            QuestChatSession {
                id: new_quest_chat_id(),
                title: truncate_title(&title_src),
                messages: vec![],
                pending_plan: None,
                updated_at: now_iso(),
            }
        };
        Ok::<_, String>((book, session))
    })
    .await
    .map_err(|e| e.to_string())??;
    let force_ai = force_ai.unwrap_or(false);
    let intent_s = intent.unwrap_or_else(|| "generate".into());

    let progress = ProgressSink {
        app: Some(&app),
        chat_id: session.id.clone(),
    };
    progress.emit("init", "Starting…", None, None);

    session.messages.push(QuestChatMessage {
        role: "user".into(),
        content: message.clone(),
        created_at: Some(now_iso()),
        plan: None,
        progress_log: None,
        usage: None,
    });

    let mut usage_acc = AiTokenUsage::default();
    let (mut plan, log) = if intent_s == "lore" {
        let mut base = session
            .pending_plan
            .clone()
            .ok_or_else(|| "No pending plan to regenerate lore".to_string())?;
        let mut lore_log = Vec::new();
        log_lore_only(&path, &mut base, &mut lore_log, &progress, &mut usage_acc).await?;
        fill_template_lore(&mut base);
        if auto_layout_plan(&mut base) {
            progress.push(&mut lore_log, "layout", "Layout: DAG auto-layout applied");
        } else {
            progress.push(&mut lore_log, "layout", "Layout: skipped — coords present");
        }
        progress.push(&mut lore_log, "done", "Lore-only regenerate done");
        (base, lore_log)
    } else {
        match run_generate_quest_line(
            &path,
            &message,
            &book,
            &session.messages,
            force_ai,
            Some(intent_s.as_str()),
            session.pending_plan.as_ref(),
            anchor_quest_id.as_deref(),
            target_chapter_id.as_deref(),
            progress,
        )
        .await
        {
            Ok((plan, log, usage)) => {
                usage_acc = usage;
                (plan, log)
            }
            Err(e) => {
                // Keep the long AI run: persist user turn + raw JSON (if present) into the chat.
                let (short_err, raw) = split_quest_raw_error(&e);
                let mut content = format!("Generation failed: {short_err}");
                if let Some(raw_json) = &raw {
                    content.push_str(
                        "\n\nRaw model JSON was saved below — open JSON mode, fix, and Apply.\n\n",
                    );
                    content.push_str(raw_json);
                }
                session.messages.push(QuestChatMessage {
                    role: "assistant".into(),
                    content,
                    created_at: Some(now_iso()),
                    plan: None,
                    progress_log: Some(vec![short_err.clone()]),
                    usage: if usage_acc.is_empty() {
                        None
                    } else {
                        Some(usage_acc.clone())
                    },
                });
                session.updated_at = now_iso();
                {
                    let dir = project_dir.clone();
                    let session_to_save = session.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        let _guard = QUEST_IO_LOCK.lock().ok();
                        let _ = save_quest_chat(&dir, &session_to_save);
                    })
                    .await;
                }
                return Err(e);
            }
        }
    };

    // Prefer full system prompt validation path
    if plan.schema_version == 0 {
        plan.schema_version = 1;
    }

    // Auto-title session from first chapter when still generic
    if session.title == "Quest line" || session.title.is_empty() {
        if let Some(ch) = plan.chapters.first() {
            if !ch.title.trim().is_empty() {
                session.title = truncate_title(&ch.title);
            }
        }
    }

    let progress_merge = ProgressSink {
        app: Some(&app),
        chat_id: session.id.clone(),
    };
    progress_merge.emit("merge", "Merging into book preview…", None, None);

    let merge = merge_quest_plan(&book, &plan)?;
    session.pending_plan = Some(plan.clone());
    let assistant_text = format!(
        "{}\n\n(progress: {})",
        plan.human_explanation,
        log.join(" · ")
    );
    let usage = if usage_acc.is_empty() {
        None
    } else {
        Some(usage_acc.clone())
    };
    session.messages.push(QuestChatMessage {
        role: "assistant".into(),
        content: assistant_text,
        created_at: Some(now_iso()),
        plan: Some(plan),
        progress_log: Some(log.clone()),
        usage: usage.clone(),
    });
    session.updated_at = now_iso();
    {
        let dir = project_dir.clone();
        let session_to_save = session.clone();
        tokio::task::spawn_blocking(move || {
            let _guard = QUEST_IO_LOCK
                .lock()
                .map_err(|_| "quest I/O lock poisoned".to_string())?;
            save_quest_chat(&dir, &session_to_save)
        })
        .await
        .map_err(|e| format!("quest chat save join: {e}"))??;
    }

    Ok(QuestChatTurnResult {
        session,
        merge,
        progress_log: log,
        usage,
    })
}

async fn log_lore_only(
    path: &str,
    plan: &mut QuestPlan,
    log: &mut Vec<String>,
    progress: &ProgressSink<'_>,
    usage_acc: &mut AiTokenUsage,
) -> Result<(), String> {
    let indices: Vec<(usize, usize)> = plan
        .chapters
        .iter()
        .enumerate()
        .flat_map(|(ci, ch)| (0..ch.quests.len()).map(move |qi| (ci, qi)))
        .collect();
    let total = indices.chunks(6).count().max(1);
    let mut i = 0usize;
    for chunk in indices.chunks(6) {
        check_quest_ai_cancel(progress, log)?;
        i += 1;
        progress.push_n(log, "lore", format!("Lore {i}/{total}…"), i, total);
        let user = build_lore_user_message(plan, chunk);
        match ai_json(QUEST_LORE_SYSTEM_PROMPT, &user, &[]).await {
            Ok((value, usage)) => {
                merge_usage(usage_acc, usage);
                let _ = stitch_lore_into_plan(plan, &value_to_raw(value));
                progress.push(log, "lore", "Lore chunk ok");
            }
            Err(e) => progress.push(log, "lore", format!("Lore fail: {e}")),
        }
    }
    let _ = path;
    Ok(())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn generate_quest_line(
    path: String,
    prompt: String,
    force_ai: Option<bool>,
) -> Result<serde_json::Value, String> {
    let prompt = prompt.trim().to_string();
    if prompt.is_empty() {
        return Err("Empty prompt".into());
    }
    clear_quest_ai_cancel();
    enforce_quest_ai_cooldown()?;
    let project_dir = project_dir(&path)?;
    let project_dir_load = project_dir.clone();
    let book = tokio::task::spawn_blocking(move || {
        let _guard = QUEST_IO_LOCK
            .lock()
            .map_err(|_| "quest I/O lock poisoned".to_string())?;
        tuffbox_core::unified::QuestBook::load_from_project(&project_dir_load)
    })
    .await
    .map_err(|e| e.to_string())??;
    let sink = ProgressSink {
        app: None,
        chat_id: String::new(),
    };
    let (plan, log, _usage) = run_generate_quest_line(
        &path,
        &prompt,
        &book,
        &[],
        force_ai.unwrap_or(false),
        Some("generate"),
        None,
        None,
        None,
        sink,
    )
    .await?;
    let mut merge = merge_quest_plan(&book, &plan)?;
    merge.notes.extend(log);
    serde_json::to_value(merge).map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub fn filter_and_merge_quest_plan(
    path: String,
    plan: QuestPlan,
    chapter_keys: Vec<String>,
    quest_keys: Vec<String>,
) -> Result<serde_json::Value, String> {
    let project_dir = project_dir(&path)?;
    let book = tuffbox_core::unified::QuestBook::load_from_project(&project_dir)?;
    let filtered = filter_plan_selection(&plan, &chapter_keys, &quest_keys);
    let result = merge_quest_plan_strict(&book, &filtered)?;
    serde_json::to_value(result).map_err(|e| e.to_string())
}

fn truncate_title(s: &str) -> String {
    let t = truncate_chars(s, 48);
    if t.is_empty() || t == "…" {
        "Quest line".into()
    } else {
        t
    }
}

/// Truncate by Unicode scalar values (not bytes) so multi-byte UTF-8 cannot panic.
pub(crate) fn truncate_chars(s: &str, max: usize) -> String {
    let mut it = s.chars();
    let head: String = it.by_ref().take(max).collect();
    if it.next().is_some() {
        format!("{head}…")
    } else {
        head
    }
}

#[cfg(test)]
mod tests {
    use super::truncate_chars;

    #[test]
    fn truncate_chars_handles_cyrillic_past_byte_limit() {
        // 100 Cyrillic letters = 200 bytes; max 180 chars must not panic on byte slice.
        let s: String = "я".repeat(100);
        assert!(s.len() > 180);
        let out = truncate_chars(&s, 50);
        assert_eq!(out.chars().count(), 51); // 50 + ellipsis
        assert!(out.ends_with('…'));
    }

    #[test]
    fn truncate_chars_short_unchanged() {
        assert_eq!(truncate_chars("hi", 180), "hi");
    }
}

// Silence unused import if QUEST_PLAN_SYSTEM_PROMPT kept for future
#[allow(dead_code)]
fn _prompt_ref() -> &'static str {
    QUEST_PLAN_SYSTEM_PROMPT
}
