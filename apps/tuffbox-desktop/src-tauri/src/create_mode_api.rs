//! Create Mode chat / pack assemble Tauri commands.

use crate::{
    auto_snapshot, download_project_mods_tracked, install_curseforge_with_dependencies_rounds,
    install_modrinth_with_dependencies_rounds, manifest_parent, save_manifest,
};
use serde::Serialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use tauri::{AppHandle, Emitter};
use tuffbox_core::create_mode::{
    assemble_pack_draft as run_assemble_pack_draft, brief_from_prompt, create_mode_response_json_schema,
    delete_create_chat as delete_create_chat_file,
    list_create_chats as list_create_chat_files, load_create_chat as load_create_chat_file,
    merge_mpi_hints_into_brief, new_chat_id, now_iso, parse_create_mode_ai_response,
    save_create_chat as save_create_chat_file, search_from_brief, validate_pack_brief,
    AssembleOptions, CreateChatMessage, CreateChatSession, CreateModeBrief, LiveCatalogSearch, PackDraft,
    CREATE_MODE_SYSTEM_PROMPT,
};
use tuffbox_core::graph::loader_kind_slug;
use tuffbox_core::mod_suggest::{
    enrich_partners_with_descriptions, merge_partner_stats, partners_from_pairs, resolve_seed_mods,
    soft_boost_partners, CandidateAddon,
};
use tuffbox_core::modpack_index::{format_tags_for_prompt, MpiModHint, MpiSearchQuery};
use tuffbox_core::swarm::ModPairStat;
use tuffbox_core::swarm_supabase::{partners_for_mod_mpi_supabase, partners_for_mod_supabase};
use tuffbox_core::{ModrinthProvider, ProjectManifest};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateModeProgress {
    phase: String,
    done: usize,
    total: usize,
    current: String,
}

fn emit_create_progress(
    app: &AppHandle,
    phase: &str,
    done: usize,
    total: usize,
    current: &str,
) {
    let _ = app.emit(
        "create-mode://progress",
        CreateModeProgress {
            phase: phase.to_string(),
            done,
            total,
            current: current.to_string(),
        },
    );
}

fn ensure_brief_from_manifest(mut brief: CreateModeBrief, manifest: &ProjectManifest) -> CreateModeBrief {
    if brief.mc_version.trim().is_empty() {
        brief.mc_version = manifest.minecraft.version.clone();
    }
    if brief.loader.trim().is_empty() {
        brief.loader = loader_kind_slug(&manifest.loader.kind).to_string();
    }
    brief
}

/// Pull reply / brief / search from an AI JSON value.
fn parse_ai_value(raw: &Value) -> (String, Option<CreateModeBrief>, Option<MpiSearchQuery>) {
    let reply = raw
        .get("reply")
        .and_then(|v| v.as_str())
        .unwrap_or("Here is a pack plan.")
        .to_string();
    let brief = raw
        .get("brief")
        .cloned()
        .and_then(|v| serde_json::from_value::<CreateModeBrief>(v).ok())
        .or_else(|| {
            if raw.get("title").is_some() {
                serde_json::from_value::<CreateModeBrief>(raw.clone()).ok()
            } else {
                None
            }
        });
    let search = raw
        .get("search")
        .cloned()
        .and_then(|v| serde_json::from_value::<MpiSearchQuery>(v).ok())
        .or_else(|| brief.as_ref().map(search_from_brief));
    (reply, brief, search)
}

fn candidates_to_mpi_hints(candidates: &[CandidateAddon]) -> Vec<MpiModHint> {
    candidates
        .iter()
        .map(|c| MpiModHint {
            name: c.name.clone(),
            slug: c.slug.clone(),
            summary: c.summary.clone(),
            categories: vec![],
            // merge_mpi_hints_into_brief only keeps keyword:/theme-name: sources
            source: format!("keyword:{}", c.source),
        })
        .collect()
}

/// Seed mods + hub/Supabase co-occurrence partners → catalog candidates for CreateModeBrief refine.
/// Prefer hub GET /v1/mods/cooccurrence (via get_creation_trends) so clients never hit MPI.
async fn collect_candidates(
    path: &str,
    search: &MpiSearchQuery,
    mc: &str,
    loader: &str,
) -> Vec<CandidateAddon> {
    let search_owned = search.clone();
    let mc_owned = mc.to_string();
    let loader_owned = loader.to_string();
    let seeds = match tokio::task::spawn_blocking(move || {
        resolve_seed_mods(&search_owned, &mc_owned, &loader_owned)
    })
    .await
    {
        Ok(v) => v,
        Err(_) => Vec::new(),
    };
    if seeds.is_empty() {
        return seeds;
    }

    // 1) Supabase TuffSwarm partners_for_mod — primary for suggestions.
    let mut primary = Vec::new();
    if let (Some(url), Some(key)) = (
        crate::integrations::swarm_supabase_url(),
        crate::integrations::swarm_supabase_anon_key(),
    ) {
        let mut batches = Vec::new();
        for seed in seeds.iter().take(3) {
            if let Ok(batch) = partners_for_mod_supabase(
                &url,
                &key,
                &seed.slug,
                12,
                Some(loader),
                Some(mc),
            )
            .await
            {
                batches.push(batch);
            }
        }
        primary = merge_partner_stats(&batches, 24);

        // Soft boost from separate Modpack Index graph.
        let mut mpi_batches = Vec::new();
        for seed in seeds.iter().take(3) {
            if let Ok(batch) = partners_for_mod_mpi_supabase(
                &url,
                &key,
                &seed.slug,
                12,
                Some(loader),
                Some(mc),
                None,
            )
            .await
            {
                mpi_batches.push(batch);
            }
        }
        let mpi = merge_partner_stats(&mpi_batches, 24);
        if primary.is_empty() {
            primary = mpi;
        } else if primary.len() < 5 {
            primary = merge_partner_stats(&[primary, mpi], 24);
        } else {
            primary = soft_boost_partners(&primary, &mpi, 24);
        }
    }

    // 2) Trends/local pairs — soft boost matches or fallback when SB empty.
    let mut local = Vec::new();
    if let Ok(trends) = crate::swarm_api::get_creation_trends(path.to_string(), Some(40)).await {
        let pairs: Vec<ModPairStat> = trends
            .get("mergedPairs")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        let mut batches = Vec::new();
        for seed in seeds.iter().take(3) {
            batches.push(partners_from_pairs(&seed.slug, &pairs, 12));
        }
        local = merge_partner_stats(&batches, 24);
    }
    let partners = soft_boost_partners(&primary, &local, 24);

    let seeds_for_enrich = seeds.clone();
    match tokio::task::spawn_blocking(move || {
        enrich_partners_with_descriptions(&seeds_for_enrich, &partners, 24)
    })
    .await
    {
        Ok(v) => v,
        Err(_) => seeds,
    }
}

#[tauri::command(rename_all = "camelCase")]
pub async fn create_mode_chat(
    app: AppHandle,
    path: String,
    chat_id: Option<String>,
    message: String,
    target_count: Option<u32>,
    history: Option<Vec<CreateChatMessage>>,
    existing_brief: Option<CreateModeBrief>,
) -> Result<Value, String> {
    let message = message.trim().to_string();
    if message.is_empty() {
        return Err("message is empty".into());
    }

    let project_dir = manifest_parent(&path)?;
    let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
    let mc = manifest.minecraft.version.clone();
    let loader = loader_kind_slug(&manifest.loader.kind).to_string();
    let target = target_count.unwrap_or(80).clamp(40, 120);

    emit_create_progress(&app, "intent", 0, 0, "Calling AI…");

    let tags = format_tags_for_prompt();
    let system = format!(
        "{CREATE_MODE_SYSTEM_PROMPT}\n\n{tags}\n\nProject context: Minecraft {mc}, loader {loader}, preferred targetCount {target}.\n\nImportant: reply, title, and reasons must use the same language as the latest user message (not Chinese unless the user wrote Chinese)."
    );

    let mut messages: Vec<Value> = Vec::new();
    if let Some(hist) = &history {
        for m in hist {
            if m.role == "user" || m.role == "assistant" {
                messages.push(json!({"role": m.role, "content": m.content}));
            }
        }
    }
    let mut user_content = message.clone();
    if let Some(brief) = &existing_brief {
        if let Ok(s) = serde_json::to_string_pretty(brief) {
            user_content = format!(
                "{user_content}\n\nCurrent CreateModeBrief (refine this if appropriate):\n{s}"
            );
        }
    }
    messages.push(json!({"role": "user", "content": user_content}));

    let settings = crate::integrations::read_settings().ai;
    let schema = create_mode_response_json_schema();
    let raw = crate::integrations::call_ai_messages_with_schema(
        &settings,
        &system,
        &messages,
        true,
        Some(schema),
    )
    .await?;
    emit_create_progress(&app, "intent", 0, 0, "Parsing brief…");
    let raw_str = serde_json::to_string(&raw).unwrap_or_else(|_| "{}".into());
    let parsed = match parse_create_mode_ai_response(&raw_str) {
        Ok(p) => p,
        Err(_) => {
            let (reply, brief, search) = parse_ai_value(&raw);
            tuffbox_core::create_mode::CreateModeAiResponse {
                reply,
                brief,
                search,
            }
        }
    };

    let used_prompt_fallback = parsed.brief.is_none();
    let mut brief = match parsed.brief {
        Some(b) => ensure_brief_from_manifest(b, &manifest),
        None => ensure_brief_from_manifest(
            brief_from_prompt(&message, &mc, &loader, target),
            &manifest,
        ),
    };
    if used_prompt_fallback {
        brief.target_count = target;
    }

    // Intent validation; one repair pass if AI brief fails checks.
    if let Err(ve) = validate_pack_brief(&brief) {
        emit_create_progress(&app, "intent", 0, 0, "Repairing brief…");
        let repair_user = format!(
            "Your previous CreateModeBrief failed validation: {ve}\nFix and return a complete Create Mode JSON (reply + search + brief). User request was:\n{message}"
        );
        let repair_msgs = vec![json!({"role": "user", "content": repair_user})];
        if let Ok(repaired) = crate::integrations::call_ai_messages_with_schema(
            &settings,
            &system,
            &repair_msgs,
            true,
            Some(create_mode_response_json_schema()),
        )
        .await
        {
            let repaired_str = serde_json::to_string(&repaired).unwrap_or_else(|_| "{}".into());
            if let Ok(p2) = parse_create_mode_ai_response(&repaired_str) {
                if let Some(b2) = p2.brief {
                    let b2 = ensure_brief_from_manifest(b2, &manifest);
                    if validate_pack_brief(&b2).is_ok() {
                        brief = b2;
                    }
                }
            }
        }
        if validate_pack_brief(&brief).is_err() {
            brief = ensure_brief_from_manifest(
                brief_from_prompt(&message, &mc, &loader, target),
                &manifest,
            );
        }
    }

    let search = parsed
        .search
        .unwrap_or_else(|| search_from_brief(&brief));

    emit_create_progress(&app, "catalog", 0, 0, "Collecting catalog candidates…");
    let candidates = collect_candidates(&path, &search, &mc, &loader).await;
    let hints = candidates_to_mpi_hints(&candidates);
    merge_mpi_hints_into_brief(&mut brief, &hints, 8);
    let brief = ensure_brief_from_manifest(brief, &manifest);
    let _ = validate_pack_brief(&brief);

    let mut reply = parsed.reply.trim().to_string();
    if reply.is_empty() {
        reply = if used_prompt_fallback {
            "Built a draft brief from your prompt (AI JSON incomplete).".into()
        } else {
            format!(
                "Pack brief ready: {} ({} must-have from catalog candidates).",
                brief.title,
                brief.must_have.len()
            )
        };
    } else if used_prompt_fallback {
        reply = format!(
            "{reply}\n\n(Note: AI JSON had no CreateModeBrief — filled from your prompt.)"
        );
    }

    emit_create_progress(&app, "intent", 0, 0, "Saving session…");
    let id = chat_id
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(new_chat_id);
    let mut session = load_create_chat_file(&project_dir, &id).unwrap_or_else(|_| CreateChatSession {
        id: id.clone(),
        title: brief.title.clone(),
        messages: history.unwrap_or_default(),
        draft: None,
        curation: None,
        updated_at: now_iso(),
    });
    session.messages.push(CreateChatMessage {
        role: "user".into(),
        content: message,
        created_at: Some(now_iso()),
    });
    session.messages.push(CreateChatMessage {
        role: "assistant".into(),
        content: reply.clone(),
        created_at: Some(now_iso()),
    });
    if session.title == "New chat" || session.title.is_empty() {
        session.title = brief.title.clone();
    }
    session.updated_at = now_iso();
    save_create_chat_file(&project_dir, &session)?;

    Ok(json!({
        "chatId": id,
        "reply": reply,
        "brief": brief,
        "search": search,
        "candidates": candidates,
        "session": session,
    }))
}

/// Deterministic CreateModeBrief from free text (no LLM) — fallback when AI is unavailable.
#[tauri::command(rename_all = "camelCase")]
pub async fn create_mode_quick_brief(
    app: AppHandle,
    path: String,
    chat_id: Option<String>,
    message: String,
    target_count: Option<u32>,
) -> Result<Value, String> {
    let message = message.trim().to_string();
    if message.is_empty() {
        return Err("message is empty".into());
    }

    emit_create_progress(&app, "intent", 0, 0, "Building brief…");

    let project_dir = manifest_parent(&path)?;
    let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
    let mc = manifest.minecraft.version.clone();
    let loader = loader_kind_slug(&manifest.loader.kind).to_string();
    let target = target_count.unwrap_or(80).clamp(40, 120);

    let mut brief = ensure_brief_from_manifest(
        brief_from_prompt(&message, &mc, &loader, target),
        &manifest,
    );
    validate_pack_brief(&brief)?;
    let search = search_from_brief(&brief);
    emit_create_progress(&app, "catalog", 0, 0, "Collecting catalog candidates…");
    let candidates = collect_candidates(&path, &search, &mc, &loader).await;
    let hints = candidates_to_mpi_hints(&candidates);
    merge_mpi_hints_into_brief(&mut brief, &hints, 8);
    let brief = ensure_brief_from_manifest(brief, &manifest);

    let reply = format!(
        "Quick assemble plan: {} ({} mods, {} must-have from catalog). No AI — default category budgets + names from your prompt.",
        brief.title,
        brief.target_count,
        brief.must_have.len()
    );

    emit_create_progress(&app, "intent", 0, 0, "Saving session…");
    let id = chat_id
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(new_chat_id);
    let mut session = load_create_chat_file(&project_dir, &id).unwrap_or_else(|_| CreateChatSession {
        id: id.clone(),
        title: brief.title.clone(),
        messages: vec![],
        draft: None,
        curation: None,
        updated_at: now_iso(),
    });
    session.messages.push(CreateChatMessage {
        role: "user".into(),
        content: message.clone(),
        created_at: Some(now_iso()),
    });
    session.messages.push(CreateChatMessage {
        role: "assistant".into(),
        content: reply.clone(),
        created_at: Some(now_iso()),
    });
    if session.title == "New chat" || session.title.is_empty() {
        session.title = brief.title.clone();
    }
    session.updated_at = now_iso();
    save_create_chat_file(&project_dir, &session)?;

    Ok(json!({
        "chatId": id,
        "reply": reply,
        "brief": brief,
        "search": search,
        "candidates": candidates,
        "session": session,
    }))
}

#[tauri::command(rename_all = "camelCase")]
pub async fn assemble_pack_draft(
    app: AppHandle,
    path: String,
    brief: CreateModeBrief,
) -> Result<PackDraft, String> {
    tokio::task::spawn_blocking(move || {
        let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
        let brief = ensure_brief_from_manifest(brief, &manifest);
        let installed = tuffbox_core::create_mode::installed_mod_keys(&manifest);
        let searcher = LiveCatalogSearch::new();
        let app2 = app.clone();
        let mut progress =
            |phase: &str, done: usize, total: usize, current: &str| {
                emit_create_progress(&app2, phase, done, total, current);
            };
        run_assemble_pack_draft(
            &searcher,
            AssembleOptions {
                brief: &brief,
                installed_ids: installed,
                max_pages_per_category: 3,
                page_size: 100,
                on_progress: Some(&mut progress),
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

async fn fetch_curation_priors(
    path: &str,
    seed_slugs: &[String],
    mc: &str,
    loader: &str,
    pillars: &[tuffbox_core::create_mode_curation::GameplayPillar],
) -> (
    Vec<tuffbox_core::create_mode_curation::CooccurPrior>,
    Vec<tuffbox_core::create_mode_curation::CooccurPartner>,
) {
    use tuffbox_core::create_mode_curation::{
        build_cooccur_partners, filter_partners_for_pillars, priority1_unmet, compute_pillar_status,
    };

    let mut priors = Vec::new();
    if let (Some(url), Some(key)) = (
        crate::integrations::swarm_supabase_url(),
        crate::integrations::swarm_supabase_anon_key(),
    ) {
        for seed in seed_slugs.iter().take(5) {
            let mut launcher = Vec::new();
            if let Ok(batch) =
                partners_for_mod_supabase(&url, &key, seed, 12, Some(loader), Some(mc)).await
            {
                launcher = batch;
            }
            let mut mpi = Vec::new();
            if let Ok(batch) = partners_for_mod_mpi_supabase(
                &url,
                &key,
                seed,
                12,
                Some(loader),
                Some(mc),
                None,
            )
            .await
            {
                mpi = batch;
            }
            let merged = if launcher.is_empty() {
                mpi
            } else if mpi.is_empty() {
                launcher
            } else {
                soft_boost_partners(&launcher, &mpi, 24)
            };
            if !merged.is_empty() {
                priors.push(build_cooccur_partners(seed, &merged, "mixed", pillars, 16));
            }
        }
    }

    // Local / hub trends soft-boost
    if let Ok(trends) = crate::swarm_api::get_creation_trends(path.to_string(), Some(40)).await {
        let pairs: Vec<ModPairStat> = trends
            .get("mergedPairs")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();
        for seed in seed_slugs.iter().take(3) {
            let local = partners_from_pairs(seed, &pairs, 12);
            if !local.is_empty() {
                priors.push(build_cooccur_partners(seed, &local, "local", pillars, 12));
            }
        }
    }

    let empty_mods: Vec<tuffbox_core::create_mode::PackDraftMod> = Vec::new();
    let status = compute_pillar_status(pillars, &empty_mods);
    let unmet = priority1_unmet(&status) || status.iter().any(|s| !s.covered);
    let partners = filter_partners_for_pillars(&priors, unmet, 24);
    (priors, partners)
}

fn reassemble_draft_blocking(
    path: &str,
    brief: CreateModeBrief,
    app: &AppHandle,
) -> Result<PackDraft, String> {
    let manifest = ProjectManifest::load_from_path(path).map_err(|e| e.to_string())?;
    let brief = ensure_brief_from_manifest(brief, &manifest);
    let installed = tuffbox_core::create_mode::installed_mod_keys(&manifest);
    let searcher = LiveCatalogSearch::new();
    let app2 = app.clone();
    let mut progress = |phase: &str, done: usize, total: usize, current: &str| {
        emit_create_progress(&app2, phase, done, total, current);
    };
    run_assemble_pack_draft(
        &searcher,
        AssembleOptions {
            brief: &brief,
            installed_ids: installed,
            max_pages_per_category: 3,
            page_size: 100,
            on_progress: Some(&mut progress),
        },
    )
}

fn search_keywords_blocking(
    keywords: &[String],
    mc: &str,
    loader: &str,
    blacklist: &[String],
    cache: &mut tuffbox_core::create_mode_curation::KeywordSearchCache,
    limit_per_kw: usize,
) -> (Vec<tuffbox_core::create_mode::PackDraftMod>, u32) {
    use tuffbox_core::create_mode::{ModSearch, PackDraftMod};
    use tuffbox_core::create_mode_curation::project_info_to_draft_mod;
    use tuffbox_core::provider::ProviderSearchQuery;

    let searcher = LiveCatalogSearch::new();
    let bl: std::collections::HashSet<_> =
        blacklist.iter().map(|s| s.to_ascii_lowercase()).collect();
    let mut out: Vec<PackDraftMod> = Vec::new();
    let mut empty_streak = 0u32;
    let mut seen = std::collections::HashSet::new();

    for kw in keywords {
        let kw = kw.trim();
        if kw.is_empty() {
            continue;
        }
        if cache.seen(loader, mc, kw) {
            continue;
        }
        cache.mark(loader, mc, kw);
        let query = ProviderSearchQuery {
            query: Some(kw.to_string()),
            minecraft_version: Some(mc.to_string()),
            loader: Some(loader.to_string()),
            sort: Some("downloads".into()),
            limit: Some(limit_per_kw as u32),
            project_type: Some("mod".into()),
            ..Default::default()
        };
        let page = match searcher.search(&query) {
            Ok(p) => p,
            Err(_) => {
                empty_streak += 1;
                continue;
            }
        };
        if page.results.is_empty() {
            empty_streak += 1;
            continue;
        }
        empty_streak = 0;
        for p in page.results.into_iter().take(limit_per_kw) {
            let slug = p.slug.to_ascii_lowercase();
            if slug.is_empty() || bl.contains(&slug) || !seen.insert(slug.clone()) {
                continue;
            }
            out.push(project_info_to_draft_mod(
                p.id,
                p.slug,
                p.name,
                p.description,
                &p.categories,
                p.downloads.unwrap_or(0),
                format!("keyword:{kw}"),
            ));
        }
    }
    (out, empty_streak)
}

static CURATE_CANCEL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[tauri::command(rename_all = "camelCase")]
pub fn cancel_curate_pack_loop() -> Result<(), String> {
    CURATE_CANCEL.store(true, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

/// Iterative curation: pillars-first + co-occurrence priors + Reviewer loop.
#[tauri::command(rename_all = "camelCase")]
pub async fn curate_pack_loop(
    app: AppHandle,
    path: String,
    brief: CreateModeBrief,
    draft: PackDraft,
    note: Option<String>,
    user_goal: Option<String>,
    max_iterations: Option<u32>,
) -> Result<Value, String> {
    use tuffbox_core::create_mode_curation::{
        apply_verdict_to_brief, build_graph_hints, compact_draft_cards, compute_pillar_status,
        curation_search_json_schema, curation_verdict_json_schema, extract_pillars_from_brief,
        filter_draft_by_keep_reject, format_cooccur_block, format_graph_hints_block,
        format_pillars_block, keep_fingerprint, keywords_for_unmet_pillars, known_slugs_from_draft,
        launcher_score, maybe_save_best, memory_push_verdict, merge_mods_into_draft,
        min_keep_for_complete, parse_curation_search, parse_curation_verdict, priority1_unmet,
        sanitize_search_keywords, update_stuck, validate_and_sync_verdict, CurationMemory,
        CurationStopReason, CurationTier, CurationVerdict, KeywordSearchCache,
        CURATION_REVIEWER_PROMPT, CURATION_SEARCH_PROMPT,
    };
    use std::collections::{HashMap, HashSet};
    use std::sync::atomic::Ordering;
    use std::time::Instant;

    CURATE_CANCEL.store(false, Ordering::SeqCst);

    let project_dir = manifest_parent(&path)?;
    let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
    let mc = manifest.minecraft.version.clone();
    let loader = loader_kind_slug(&manifest.loader.kind).to_string();
    let installed: HashSet<String> = tuffbox_core::create_mode::installed_mod_keys(&manifest)
        .into_iter()
        .map(|s| s.to_ascii_lowercase())
        .collect();

    let potato = crate::launcher_settings::load_launcher_settings().potato_pc;
    let tier = CurationTier::from_potato_flag(potato);
    let max_iter = max_iterations
        .unwrap_or_else(|| tier.default_max_iterations())
        .clamp(1, 8);
    let time_budget = std::time::Duration::from_secs(tier.time_budget_secs());
    let started = Instant::now();

    let goal = user_goal
        .or(note.clone())
        .unwrap_or_else(|| brief.title.clone());
    let mut brief = ensure_brief_from_manifest(brief, &manifest);
    let mut draft = draft;
    if draft.mods.is_empty() {
        return Err("curate_pack_loop requires a non-empty catalog draft".into());
    }

    let pillars = extract_pillars_from_brief(&brief, &goal);
    let mut memory = CurationMemory {
        keep_mod_ids: draft.mods.iter().map(|m| m.slug.clone()).take(40).collect(),
        last_keep_fingerprint: keep_fingerprint(
            &draft
                .mods
                .iter()
                .map(|m| m.slug.clone())
                .collect::<Vec<_>>(),
        ),
        ..Default::default()
    };

    let seed_slugs: Vec<String> = brief
        .must_have
        .iter()
        .filter_map(|m| m.slug_hint.clone().or_else(|| Some(m.query.clone())))
        .chain(draft.mods.iter().take(5).map(|m| m.slug.clone()))
        .map(|s| s.to_ascii_lowercase())
        .collect::<HashSet<_>>()
        .into_iter()
        .take(5)
        .collect();

    emit_create_progress(
        &app,
        "curate",
        0,
        max_iter as usize,
        "Fetching co-occurrence priors…",
    );
    let (_priors, mut partners) =
        fetch_curation_priors(&path, &seed_slugs, &mc, &loader, &pillars).await;
    let mut partner_map: HashMap<String, _> = partners
        .iter()
        .map(|p| (p.slug.to_ascii_lowercase(), p.clone()))
        .collect();

    // Seed draft with prior partners resolved via keyword=slug search (cheap).
    {
        let prior_kws: Vec<String> = partners
            .iter()
            .filter(|p| {
                !p.covers_pillars.is_empty()
                    || matches!(
                        p.role,
                        tuffbox_core::create_mode_curation::CandidateRole::Gameplay
                    )
            })
            .map(|p| p.slug.clone())
            .take(8)
            .collect();
        if !prior_kws.is_empty() {
            let mut cache0 = KeywordSearchCache::new();
            let mc2 = mc.clone();
            let loader2 = loader.clone();
            let bl = memory.blacklisted_mod_ids.clone();
            let (extra, _) = tokio::task::spawn_blocking(move || {
                search_keywords_blocking(&prior_kws, &mc2, &loader2, &bl, &mut cache0, 4)
            })
            .await
            .map_err(|e| e.to_string())?;
            draft = merge_mods_into_draft(&draft, &extra, &memory.blacklisted_mod_ids);
        }
    }

    let settings = crate::integrations::read_settings().ai;
    let mut stop = CurationStopReason::MaxIterations;
    let mut last_note = String::new();
    let mut prev_score = 0.0f32;
    let mut search_cache = KeywordSearchCache::new();
    let mut empty_hit_streak = 0u32;
    let mut ai_fail_streak = 0u32;
    let mut pending_keywords: Vec<String> = keywords_for_unmet_pillars(
        &pillars,
        &compute_pillar_status(&pillars, &draft.mods),
        &[],
        6,
    );

    for i in 1..=max_iter {
        if CURATE_CANCEL.load(Ordering::SeqCst) {
            stop = CurationStopReason::Cancelled;
            break;
        }
        if started.elapsed() > time_budget {
            stop = CurationStopReason::Timeout;
            break;
        }

        emit_create_progress(
            &app,
            "curate",
            (i - 1) as usize,
            max_iter as usize,
            &format!("Curation iteration {i}/{max_iter}…"),
        );

        let status = compute_pillar_status(&pillars, &draft.mods);

        // ── Search keywords (launcher pillar force + optional SearchRole) ──
        let mut keywords = sanitize_search_keywords(
            &pending_keywords,
            &pillars,
            &status,
            &memory.searched_keywords,
        );
        if keywords.is_empty() {
            keywords = keywords_for_unmet_pillars(
                &pillars,
                &status,
                &memory.searched_keywords,
                6,
            );
        }
        if keywords.is_empty() && tier.search_role_llm() && priority1_unmet(&status) {
            emit_create_progress(&app, "curate", (i - 1) as usize, max_iter as usize, "SearchRole…");
            let unmet_labels: Vec<_> = status
                .iter()
                .filter(|s| !s.covered)
                .map(|s| s.label.clone())
                .collect();
            let search_user = format!(
                "User goal: {goal}\nUnmet pillars: {unmet_labels:?}\n\
                 Already searched: {:?}\nBlacklist ids: {:?}\n\
                 Propose keywords only for unmet gameplay pillars.",
                memory.searched_keywords, memory.blacklisted_mod_ids
            );
            if let Ok(raw) = crate::integrations::call_ai_messages_with_schema(
                &settings,
                &format!("{CURATION_SEARCH_PROMPT}\nMC {mc} / {loader}"),
                &[json!({"role": "user", "content": search_user})],
                true,
                Some(curation_search_json_schema()),
            )
            .await
            {
                let s = serde_json::to_string(&raw).unwrap_or_default();
                if let Ok(q) = parse_curation_search(&s) {
                    keywords = sanitize_search_keywords(
                        &q.keywords,
                        &pillars,
                        &status,
                        &memory.searched_keywords,
                    );
                }
            }
        }

        // ── Catalog keyword fan-out (cached) ──
        if !keywords.is_empty() {
            emit_create_progress(
                &app,
                "catalog",
                (i - 1) as usize,
                max_iter as usize,
                &format!("Catalog search: {}", keywords.join(", ")),
            );
            let kw = keywords.clone();
            let mc2 = mc.clone();
            let loader2 = loader.clone();
            let bl = memory.blacklisted_mod_ids.clone();
            let mut cache_move = std::mem::take(&mut search_cache);
            let ((extra_mods, empty), cache_back) = tokio::task::spawn_blocking(move || {
                let r = search_keywords_blocking(&kw, &mc2, &loader2, &bl, &mut cache_move, 12);
                (r, cache_move)
            })
            .await
            .map_err(|e| e.to_string())?;
            search_cache = cache_back;
            if empty > 0 && extra_mods.is_empty() {
                empty_hit_streak += 1;
            } else {
                empty_hit_streak = 0;
            }
            draft = merge_mods_into_draft(&draft, &extra_mods, &memory.blacklisted_mod_ids);
        }

        let cards = compact_draft_cards(&draft, &pillars, &partner_map, tier.max_cards());
        let hints = build_graph_hints(
            &draft.mods,
            &installed,
            potato,
            tier.max_graph_hints(),
        );
        let cards_json = serde_json::to_string_pretty(&cards).unwrap_or_else(|_| "[]".into());
        let pillars_block = format_pillars_block(&status);
        let cooccur_block = format_cooccur_block(&partners, if potato { 12 } else { 20 });
        let hints_block = format_graph_hints_block(&hints);

        let user = format!(
            "User goal:\n{goal}\n\nMinecraft {mc} / {loader}\n\n{pillars_block}\n{cooccur_block}\n\
             {hints_block}\n## Compact candidates\n{cards_json}\n\n\
             Previous missing: {:?}\nBlacklist: {:?}\nSearched keywords: {:?}\n",
            memory.missing_aspects,
            memory.blacklisted_mod_ids.iter().take(80).collect::<Vec<_>>(),
            memory.searched_keywords.iter().take(60).collect::<Vec<_>>()
        );

        let system = format!(
            "{CURATION_REVIEWER_PROMPT}\n\nProject: Minecraft {mc}, loader {loader}."
        );
        let messages = vec![json!({"role": "user", "content": user})];
        let raw = crate::integrations::call_ai_messages_with_schema(
            &settings,
            &system,
            &messages,
            true,
            Some(curation_verdict_json_schema()),
        )
        .await;

        let mut verdict = match raw {
            Ok(v) => {
                ai_fail_streak = 0;
                let s = serde_json::to_string(&v).unwrap_or_else(|_| "{}".into());
                parse_curation_verdict(&s).unwrap_or_else(|_| {
                    ai_fail_streak = 1;
                    CurationVerdict {
                    is_complete: false,
                    coverage_score: 0.4,
                    missing_aspects: status
                        .iter()
                        .filter(|s| !s.covered)
                        .map(|s| s.label.clone())
                        .collect(),
                    rejected_mod_ids: vec![],
                    keep_mod_ids: draft.mods.iter().take(24).map(|m| m.slug.clone()).collect(),
                    next_search_keywords: keywords_for_unmet_pillars(
                        &pillars,
                        &status,
                        &memory.searched_keywords,
                        6,
                    ),
                    human_note: "Reviewer parse fallback — continuing with launcher keywords."
                        .into(),
                    pillar_status: status.clone(),
                }
                })
            }
            Err(e) => {
                ai_fail_streak += 1;
                CurationVerdict {
                is_complete: false,
                coverage_score: 0.3,
                missing_aspects: status
                    .iter()
                    .filter(|s| !s.covered)
                    .map(|s| s.label.clone())
                    .collect(),
                rejected_mod_ids: vec![],
                keep_mod_ids: draft.mods.iter().take(24).map(|m| m.slug.clone()).collect(),
                next_search_keywords: keywords_for_unmet_pillars(
                    &pillars,
                    &status,
                    &memory.searched_keywords,
                    6,
                ),
                human_note: format!(
                    "AI unavailable ({e}); launcher continues with pillar keywords."
                ),
                pillar_status: status.clone(),
            }
            }
        };

        let known = known_slugs_from_draft(&draft);
        let keep_mods: Vec<_> = if verdict.keep_mod_ids.is_empty() {
            draft.mods.clone()
        } else {
            draft
                .mods
                .iter()
                .filter(|m| {
                    verdict
                        .keep_mod_ids
                        .iter()
                        .any(|k| k.eq_ignore_ascii_case(&m.slug))
                })
                .cloned()
                .collect()
        };
        let min_keep = min_keep_for_complete(brief.target_count);
        verdict = validate_and_sync_verdict(
            verdict,
            &known,
            &pillars,
            if keep_mods.is_empty() {
                &draft.mods
            } else {
                &keep_mods
            },
            &memory.searched_keywords,
            min_keep,
        );
        last_note = verdict.human_note.clone();

        let filtered =
            filter_draft_by_keep_reject(&draft, &verdict.keep_mod_ids, &verdict.rejected_mod_ids);
        brief = apply_verdict_to_brief(&brief, &verdict, &partners);
        for k in &keywords {
            if !memory.searched_keywords.iter().any(|s| s == k) {
                memory.searched_keywords.push(k.clone());
            }
        }
        pending_keywords = verdict.next_search_keywords.clone();

        emit_create_progress(
            &app,
            "catalog",
            i as usize,
            max_iter as usize,
            &format!("Re-assembling after curation {i}…"),
        );
        let path_c = path.clone();
        let brief_c = brief.clone();
        let app_c = app.clone();
        draft = tokio::task::spawn_blocking(move || {
            reassemble_draft_blocking(&path_c, brief_c, &app_c)
        })
        .await
        .map_err(|e| e.to_string())??;

        if !filtered.mods.is_empty() {
            let mut seen: HashSet<String> = draft
                .mods
                .iter()
                .map(|m| m.slug.to_ascii_lowercase())
                .collect();
            let mut merged = filtered.mods.clone();
            for m in &draft.mods {
                let k = m.slug.to_ascii_lowercase();
                if seen.insert(k) {
                    merged.push(m.clone());
                }
            }
            let capped = tuffbox_core::create_mode_curation::apply_role_caps(
                &merged,
                brief.target_count,
            );
            draft.mods = capped;
            draft.brief = brief.clone();
        }

        let status_after = compute_pillar_status(&pillars, &draft.mods);
        let partner_slugs: HashSet<String> =
            partners.iter().map(|p| p.slug.to_ascii_lowercase()).collect();
        let score = launcher_score(&draft, &pillars, &status_after, &partner_slugs);
        maybe_save_best(
            &mut memory,
            i,
            draft.clone(),
            verdict.coverage_score,
            score,
            status_after.clone(),
        );

        let stuck = update_stuck(&mut memory, &verdict.keep_mod_ids, score - prev_score);
        prev_score = score;
        memory_push_verdict(&mut memory, verdict.clone());

        if empty_hit_streak >= tier.empty_keyword_streak_limit() && priority1_unmet(&status_after)
        {
            stop = CurationStopReason::EmptyPool;
            break;
        }

        // Two consecutive AI failures → stop with best so far (offer Quick in UI).
        if ai_fail_streak >= 2 {
            stop = CurationStopReason::AiDown;
            break;
        }

        if verdict.is_complete && !priority1_unmet(&status_after) {
            stop = CurationStopReason::Complete;
            break;
        }
        if stuck {
            stop = if priority1_unmet(&status_after) {
                CurationStopReason::PillarsUnmet
            } else {
                CurationStopReason::Stuck
            };
            break;
        }

        if i < max_iter {
            let new_seeds: Vec<String> = verdict.keep_mod_ids.iter().take(5).cloned().collect();
            if !new_seeds.is_empty() {
                let (_p2, partners2) =
                    fetch_curation_priors(&path, &new_seeds, &mc, &loader, &pillars).await;
                if !partners2.is_empty() {
                    partners = partners2;
                    partner_map = partners
                        .iter()
                        .map(|p| (p.slug.to_ascii_lowercase(), p.clone()))
                        .collect();
                }
            }
        }
    }

    let best = memory.best.clone().unwrap_or_else(|| {
        let st = compute_pillar_status(&pillars, &draft.mods);
        tuffbox_core::create_mode_curation::CurationSnapshot {
            iteration: max_iter,
            coverage_score: 0.0,
            launcher_score: launcher_score(&draft, &pillars, &st, &HashSet::new()),
            draft: draft.clone(),
            pillar_status: st,
        }
    });

    let final_status = best.pillar_status.clone();
    let partial = priority1_unmet(&final_status) || stop != CurationStopReason::Complete;
    if partial && stop == CurationStopReason::MaxIterations {
        stop = CurationStopReason::PillarsUnmet;
    }

    let reply = if last_note.trim().is_empty() {
        format!(
            "Curated pack: {} mods, launcher_score {:.2}, stop={}, tier={:?}.",
            best.draft.mods.len(),
            best.launcher_score,
            stop.as_str(),
            tier
        )
    } else {
        last_note
    };

    let curation = tuffbox_core::create_mode_curation::CurationSessionPersist::from_loop_result(
        memory.clone(),
        final_status.clone(),
        partial,
        stop,
        best.launcher_score,
        tier,
    );

    Ok(json!({
        "reply": reply,
        "brief": best.draft.brief.clone(),
        "draft": best.draft.clone(),
        "pillars": pillars,
        "pillarStatus": final_status,
        "partial": partial,
        "stopReason": stop.as_str(),
        "launcherScore": best.launcher_score,
        "iteration": best.iteration,
        "iterationsRun": best.iteration,
        "tier": match tier {
            CurationTier::Potato => "potato",
            CurationTier::Normal => "normal",
            CurationTier::Strong => "strong",
        },
        "memory": memory,
        "curation": curation,
        "cooccurPartners": partners,
        "projectDir": project_dir.to_string_lossy(),
    }))
}

/// Thin alias: one curation iteration (Reviewer + catalog), same contracts as Curate.
#[tauri::command(rename_all = "camelCase")]
pub async fn rank_pack_draft(
    app: AppHandle,
    path: String,
    brief: CreateModeBrief,
    draft: PackDraft,
    note: Option<String>,
) -> Result<Value, String> {
    let mut out = curate_pack_loop(
        app,
        path,
        brief,
        draft,
        note,
        None,
        Some(1),
    )
    .await?;
    // Preserve Rank response shape for older UI callers.
    if let Some(obj) = out.as_object_mut() {
        if !obj.contains_key("search") {
            if let Some(brief) = obj.get("brief").cloned() {
                if let Ok(b) = serde_json::from_value::<CreateModeBrief>(brief) {
                    obj.insert(
                        "search".into(),
                        serde_json::to_value(search_from_brief(&b)).unwrap_or(json!({})),
                    );
                }
            }
        }
    }
    Ok(out)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn preview_pack_draft(
    path: String,
    draft: PackDraft,
    sample_limit: Option<u32>,
) -> Result<Value, String> {
    tokio::task::spawn_blocking(move || {
        use tuffbox_core::provider::{ContentProvider, ProviderFileInfo};

        let manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
        let provider = ModrinthProvider::new();
        let loader = loader_kind_slug(&manifest.loader.kind).to_string();
        let mc = manifest.minecraft.version.clone();
        let installed = tuffbox_core::create_mode::installed_mod_keys(&manifest);
        let installed_set: std::collections::HashSet<String> =
            installed.into_iter().map(|s| s.to_ascii_lowercase()).collect();

        let limit = sample_limit
            .unwrap_or(draft.mods.len() as u32)
            .clamp(1, draft.mods.len().max(1) as u32) as usize;
        let mut ok = 0usize;
        let mut skip = 0usize;
        let mut failures: Vec<Value> = Vec::new();
        let mut items: Vec<Value> = Vec::new();

        for m in draft.mods.iter().take(limit) {
            let id_key = m.project_id.to_ascii_lowercase();
            let slug_key = m.slug.to_ascii_lowercase();
            let already = (!id_key.is_empty() && installed_set.contains(&id_key))
                || (!slug_key.is_empty() && installed_set.contains(&slug_key));
            if already {
                skip += 1;
                items.push(json!({
                    "slug": m.slug,
                    "projectId": m.project_id,
                    "name": m.name,
                    "provider": m.provider,
                    "status": "skip",
                    "version": null,
                    "fileName": null,
                    "hashAlgo": null,
                    "hash": null,
                    "destPath": format!("mods/{}", m.slug),
                    "error": null,
                }));
                continue;
            }

            let id = if m.project_id.is_empty() {
                m.slug.as_str()
            } else {
                m.project_id.as_str()
            };
            let query = tuffbox_core::ProviderSearchQuery {
                minecraft_version: Some(mc.clone()),
                loader: Some(loader.clone()),
                ..Default::default()
            };

            let lookup_id = if m.provider == "curseforge" && !m.slug.is_empty() {
                m.slug.as_str()
            } else {
                id
            };
            let resolved = provider
                .get_versions(lookup_id, &query)
                .map_err(|e| e.to_string());

            match resolved {
                Ok(versions) if !versions.is_empty() => {
                    let ver = &versions[0];
                    let file = ProviderFileInfo::primary_file(ver)
                        .or_else(|| ver.files.first());
                    let (file_name, hash_algo, hash) = if let Some(f) = file {
                        let (algo, h) = if let Some(ref s) = f.hashes.sha512 {
                            ("sha512", Some(s.clone()))
                        } else if let Some(ref s) = f.hashes.sha1 {
                            ("sha1", Some(s.clone()))
                        } else {
                            ("", None)
                        };
                        (Some(f.filename.clone()), algo, h)
                    } else {
                        (None, "", None)
                    };
                    let dest = format!(
                        "mods/{}",
                        file_name
                            .as_deref()
                            .unwrap_or(if m.slug.is_empty() { id } else { &m.slug })
                    );
                    ok += 1;
                    items.push(json!({
                        "slug": m.slug,
                        "projectId": m.project_id,
                        "name": m.name,
                        "provider": m.provider,
                        "status": "ok",
                        "version": ver.version_number,
                        "fileName": file_name,
                        "hashAlgo": if hash_algo.is_empty() { Value::Null } else { json!(hash_algo) },
                        "hash": hash,
                        "destPath": dest,
                        "error": null,
                    }));
                }
                Ok(_) => {
                    failures.push(json!({
                        "slug": m.slug,
                        "projectId": m.project_id,
                        "error": "no compatible version",
                    }));
                    items.push(json!({
                        "slug": m.slug,
                        "projectId": m.project_id,
                        "name": m.name,
                        "provider": m.provider,
                        "status": "fail",
                        "version": null,
                        "fileName": null,
                        "hashAlgo": null,
                        "hash": null,
                        "destPath": format!("mods/{}", m.slug),
                        "error": "no compatible version",
                    }));
                }
                Err(e) => {
                    failures.push(json!({
                        "slug": m.slug,
                        "projectId": m.project_id,
                        "error": e,
                    }));
                    items.push(json!({
                        "slug": m.slug,
                        "projectId": m.project_id,
                        "name": m.name,
                        "provider": m.provider,
                        "status": "fail",
                        "version": null,
                        "fileName": null,
                        "hashAlgo": null,
                        "hash": null,
                        "destPath": format!("mods/{}", m.slug),
                        "error": e,
                    }));
                }
            }
        }

        Ok(json!({
            "checked": limit.min(draft.mods.len()),
            "ok": ok,
            "skip": skip,
            "failures": failures,
            "items": items,
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub async fn install_pack_draft(
    app: AppHandle,
    path: String,
    draft: PackDraft,
    confirmed: bool,
    side: Option<String>,
) -> Result<Value, String> {
    if !confirmed {
        return Err("install requires explicit confirm".into());
    }
    if draft.mods.is_empty() {
        return Err("pack draft has no mods to install".into());
    }

    let side = side.unwrap_or_else(|| "both".into());
    let cooccur_ids: Vec<String> = draft
        .mods
        .iter()
        .map(|m| {
            if !m.slug.is_empty() {
                m.slug.clone()
            } else if !m.project_id.is_empty() {
                m.project_id.clone()
            } else {
                m.name.clone()
            }
        })
        .collect();

    let mr_ids: Vec<String> = draft
        .mods
        .iter()
        .filter(|m| m.provider != "curseforge")
        .map(|m| {
            if !m.project_id.is_empty() {
                m.project_id.clone()
            } else {
                m.slug.clone()
            }
        })
        .collect();
    let cf_ids: Vec<String> = draft
        .mods
        .iter()
        .filter(|m| m.provider == "curseforge")
        .map(|m| m.project_id.clone())
        .filter(|id| !id.is_empty())
        .collect();
    let total = mr_ids.len() + cf_ids.len();
    let path_for_stats = path.clone();

    let result = tokio::task::spawn_blocking(move || {
        let manifest_path = PathBuf::from(&path);
        auto_snapshot(&manifest_path, "create-mode-pack-install").map_err(|e| e.to_string())?;
        let mut manifest = ProjectManifest::load_from_path(&path).map_err(|e| e.to_string())?;
        let mut all_installed = Vec::new();
        let mut done = 0usize;

        for cf_id in &cf_ids {
            emit_create_progress(
                &app,
                "install",
                done,
                total,
                &format!("curseforge:{cf_id}"),
            );
            match install_curseforge_with_dependencies_rounds(
                &mut manifest,
                &[cf_id.clone()],
                &side,
                50,
            ) {
                Ok(ids) => all_installed.extend(ids),
                Err(e) => {
                    // Continue remaining mods; surface later via counts.
                    let _ = e;
                }
            }
            done += 1;
        }

        const CHUNK: usize = 25;
        let chunks: Vec<_> = mr_ids.chunks(CHUNK).collect();
        let chunk_count = chunks.len().max(1);
        for (i, chunk) in chunks.into_iter().enumerate() {
            emit_create_progress(
                &app,
                "install",
                done + i * CHUNK,
                total,
                &format!("modrinth batch {}/{}", i + 1, chunk_count),
            );
            let installed =
                install_modrinth_with_dependencies_rounds(&mut manifest, chunk, &side, 200, None)?;
            all_installed.extend(installed);
        }

        save_manifest(&manifest_path, &manifest).map_err(|e| e.to_string())?;
        download_project_mods_tracked(&app, &manifest_path, &manifest, None, true);
        emit_create_progress(&app, "install", total, total, "done");

        Ok::<_, String>(json!({
            "installedCount": all_installed.len(),
            "installed": all_installed,
            "requested": total,
        }))
    })
    .await
    .map_err(|e| e.to_string())??;

    // Fire-and-forget local + Supabase co-occurrence (does not fail install).
    let stats = crate::swarm_api::record_and_upload_cooccurrence(
        &path_for_stats,
        &cooccur_ids,
        "create_mode_install",
    )
    .await
    .unwrap_or_else(|e| json!({ "local": false, "uploaded": false, "uploadError": e }));

    Ok(json!({
        "installedCount": result.get("installedCount").cloned().unwrap_or(json!(0)),
        "installed": result.get("installed").cloned().unwrap_or(json!([])),
        "requested": result.get("requested").cloned().unwrap_or(json!(0)),
        "cooccurrence": stats,
    }))
}

/// Resolve Modpack Index hints (blocking MPI HTTP from this process).
/// Prefer hub-seeded co-occurrence for Create Mode; this command is discovery-only
/// and should not be used as the default pack-builder data path (MPI privacy).
#[tauri::command(rename_all = "camelCase")]
pub async fn resolve_modpack_index_search(
    search: MpiSearchQuery,
    per_source: Option<u32>,
) -> Result<Vec<MpiModHint>, String> {
    let per = per_source.unwrap_or(8).clamp(1, 40) as usize;
    tokio::task::spawn_blocking(move || {
        tuffbox_core::modpack_index::gather_search_hints(&search, per)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command(rename_all = "camelCase")]
pub fn list_create_chats(path: String) -> Result<Vec<CreateChatSession>, String> {
    let project_dir = manifest_parent(&path)?;
    list_create_chat_files(&project_dir)
}

#[tauri::command(rename_all = "camelCase")]
pub fn save_create_chat(path: String, session: CreateChatSession) -> Result<(), String> {
    let project_dir = manifest_parent(&path)?;
    save_create_chat_file(&project_dir, &session)?;
    Ok(())
}

#[tauri::command(rename_all = "camelCase")]
pub fn load_create_chat(path: String, chat_id: String) -> Result<CreateChatSession, String> {
    let project_dir = manifest_parent(&path)?;
    load_create_chat_file(&project_dir, &chat_id)
}

#[tauri::command(rename_all = "camelCase")]
pub fn delete_create_chat(path: String, chat_id: String) -> Result<(), String> {
    let project_dir = manifest_parent(&path)?;
    delete_create_chat_file(&project_dir, &chat_id)
}

#[tauri::command(rename_all = "camelCase")]
pub fn new_create_chat(path: String, title: Option<String>) -> Result<CreateChatSession, String> {
    let project_dir = manifest_parent(&path)?;
    let session = CreateChatSession {
        id: new_chat_id(),
        title: title.unwrap_or_else(|| "New chat".into()),
        messages: vec![],
        draft: None,
        curation: None,
        updated_at: now_iso(),
    };
    save_create_chat_file(&project_dir, &session)?;
    Ok(session)
}
