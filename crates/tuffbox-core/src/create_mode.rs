//! Create Mode: AI proposes a CreateModeBrief; PackAssembler fills 50–100 mods via Modrinth search.

use crate::provider::{
    ContentProvider, ModrinthProvider, ProjectInfo, ProviderSearchQuery, SearchPage,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub const CREATE_MODE_SYSTEM_PROMPT: &str = r#"You are TuffBox Create Mode — a Minecraft modpack planner.
Your job is to turn the user's brief into (1) a machine search intent and (2) a CreateModeBrief JSON plan for searching Modrinth.
Never invent Modrinth project IDs or claim specific mods are installed. Use search queries only.
Do not output ActionPlan crash JSON.

Respond with a single JSON object:
{
  "reply": "short human reply (1-3 sentences)",
  "search": {
    "loader": "fabric"|"forge"|"neoforge"|"quilt"|null,
    "version": "1.21.1"|null,
    "theme": "tech"|"magic"|"sci-fi"|...|null,
    "keywords": ["airplanes", "flight", "create"]
  },
  "brief": {
    "title": string,
    "mcVersion": string,
    "loader": "fabric"|"forge"|"neoforge"|"quilt",
    "targetCount": number,
    "mustHave": [{"query": string, "slugHint": string|null, "reason": string}],
    "categories": [{"id": string, "query": string, "count": number, "reason": string}],
    "exclude": string[]
  }
}
Rules:
- Language: write reply, title, and reason fields in the same language as the user's latest message (Russian→Russian, English→English, etc.). Never switch to Chinese or another language unless the user wrote in it. JSON keys, search.theme, search.keywords, and Modrinth queries stay English.
- Always fill search: extract loader/version/theme/keywords from the user prompt (theme aliases: industrial→tech). keywords are English mod-name tokens for Modpack Index + Modrinth.
- targetCount should match the user's requested size (typically 40–120).
- categories[].count values should sum approximately to targetCount.
- Prefer concrete Modrinth search queries (short, 1–3 words: "create", "jei", "iron chests", "airplane").
- Use category ids aligned with Modrinth when possible: technology, magic, decoration, utility, adventure, worldgen, storage, food, equipment, transportation, library.
- Keep library/API budget small (about 8–12% of target) — gameplay mods first.
- mustHave is for named must-include mods (query by name; optional slugHint if known).
- exclude lists slugs/names to skip.
- If refining an existing brief, update fields and keep prior intent unless asked to change it.
- Entries in mustHave with reason "Locked by user" and every entry already in exclude were set
  manually by the player in the draft UI. They MUST be preserved verbatim (same slugHint/text)
  across refinements unless the user explicitly asks to unlock/remove/re-include them.
"#;

pub const CREATE_MODE_REFINE_PROMPT: &str = r#"You are refining a Create Mode CreateModeBrief for the TuffBox launcher.
You are given catalog candidates (name + short description + Modrinth slug) from Modrinth + community co-occurrence
(seeded from Modpack Index on the hub — never scraped from the user's IP).
Pick mods that match the user intent (e.g. airplanes/flight → Create Aeronautics) and put them in mustHave.
Always include the industrial/theme base mod when relevant (e.g. Create).

Do NOT output crash ActionPlan JSON. Create Mode uses CreateModeBrief + PackDraft only.

Return a single JSON object:
{
  "reply": "short human explanation in the user's language (1-3 sentences)",
  "search": { "loader", "version", "theme", "keywords" },
  "brief": {
    "title": string,
    "mcVersion": string,
    "loader": string,
    "targetCount": number,
    "mustHave": [{"query": string, "slugHint": string|null, "reason": string}],
    "categories": [{"id": string, "query": string, "count": number, "reason": string}],
    "exclude": string[]
  }
}
Rules:
- Prefer slugHint values from the candidate list when known.
- mustHave should cover base + matching addons (typically 2–8 entries).
- Keep categories budgets summing near targetCount.
- Never invent jar filenames or crash ActionPlan ops.
- Entries in mustHave with reason "Locked by user" and every entry already in exclude were set
  manually by the player in the draft UI. They MUST be preserved verbatim (same slugHint/text)
  across refinements unless the user explicitly asks to unlock/remove/re-include them.
"#;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MustHaveSpec {
    pub query: String,
    #[serde(default)]
    pub slug_hint: Option<String>,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CategoryBudget {
    pub id: String,
    pub query: String,
    pub count: u32,
    #[serde(default)]
    pub reason: String,
    /// Optional Modrinth category facet (e.g. "technology", "utility").
    #[serde(default)]
    pub facet: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CreateModeBrief {
    pub title: String,
    pub mc_version: String,
    pub loader: String,
    pub target_count: u32,
    #[serde(default)]
    pub must_have: Vec<MustHaveSpec>,
    #[serde(default)]
    pub categories: Vec<CategoryBudget>,
    #[serde(default)]
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PackDraftMod {
    pub slug: String,
    pub project_id: String,
    pub name: String,
    pub reason: String,
    pub category: String,
    #[serde(default)]
    pub downloads: u64,
    /// `modrinth` (default) or `curseforge`.
    #[serde(default = "default_provider_modrinth")]
    pub provider: String,
}

fn default_provider_modrinth() -> String {
    "modrinth".into()
}

fn provider_for_project(p: &ProjectInfo) -> String {
    if p.description.starts_with("curseforge:")
        || (!p.id.is_empty() && p.id.chars().all(|c| c.is_ascii_digit()))
    {
        "curseforge".into()
    } else {
        "modrinth".into()
    }
}

fn pack_draft_mod_from_project(p: ProjectInfo, reason: String, category: String) -> PackDraftMod {
    let provider = provider_for_project(&p);
    PackDraftMod {
        slug: p.slug,
        project_id: p.id,
        name: p.name,
        reason,
        category,
        downloads: p.downloads.unwrap_or(0),
        provider,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PackDraft {
    pub brief: CreateModeBrief,
    pub mods: Vec<PackDraftMod>,
    #[serde(default)]
    pub unresolved: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateModeAiResponse {
    #[serde(default)]
    pub reply: String,
    #[serde(default)]
    pub brief: Option<CreateModeBrief>,
    /// Machine search intent for Modpack Index / catalogs (step 1).
    #[serde(default)]
    pub search: Option<crate::modpack_index::MpiSearchQuery>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateChatMessage {
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateChatSession {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub messages: Vec<CreateChatMessage>,
    #[serde(default)]
    pub draft: Option<PackDraft>,
    /// Last Curate loop snapshot (pillars / memory / partial) — optional, forward-compatible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curation: Option<crate::create_mode_curation::CurationSessionPersist>,
    #[serde(default)]
    pub updated_at: String,
}

/// Progress callback for assemble/install phases.
pub type ProgressFn = Box<dyn FnMut(&str, usize, usize, &str) + Send>;

pub fn parse_pack_brief(raw: &str) -> Result<CreateModeBrief, String> {
    let trimmed = strip_json_fences(raw);
    // Accept either bare CreateModeBrief or { brief: ... } / CreateModeAiResponse.
    if let Ok(brief) = serde_json::from_str::<CreateModeBrief>(trimmed) {
        return Ok(normalize_brief(brief));
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Some(b) = v.get("brief") {
            let brief: CreateModeBrief =
                serde_json::from_value(b.clone()).map_err(|e| format!("invalid brief: {e}"))?;
            return Ok(normalize_brief(brief));
        }
    }
    Err("could not parse CreateModeBrief JSON".into())
}

pub fn parse_create_mode_ai_response(raw: &str) -> Result<CreateModeAiResponse, String> {
    let trimmed = strip_json_fences(raw);
    if let Ok(resp) = serde_json::from_str::<CreateModeAiResponse>(trimmed) {
        let brief = resp.brief.map(normalize_brief);
        let search = resp.search.or_else(|| {
            brief
                .as_ref()
                .map(search_from_brief)
                .filter(|s| s.theme.is_some() || !s.keywords.is_empty())
        });
        return Ok(CreateModeAiResponse {
            reply: resp.reply,
            brief,
            search,
        });
    }
    // Fallback: treat whole object as brief.
    if let Ok(brief) = parse_pack_brief(trimmed) {
        let search = search_from_brief(&brief);
        return Ok(CreateModeAiResponse {
            reply: format!(
                "Draft plan ready: {} ({} mods).",
                brief.title, brief.target_count
            ),
            brief: Some(brief),
            search: Some(search).filter(|s| s.theme.is_some() || !s.keywords.is_empty()),
        });
    }
    Err("AI response was not valid Create Mode JSON".into())
}

/// Build a coarse MpiSearchQuery from an existing CreateModeBrief (no LLM).
pub fn search_from_brief(brief: &CreateModeBrief) -> crate::modpack_index::MpiSearchQuery {
    let mut keywords: Vec<String> = brief
        .must_have
        .iter()
        .map(|m| m.query.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    for cat in &brief.categories {
        let q = cat.query.trim();
        if !q.is_empty() && !keywords.iter().any(|k| k.eq_ignore_ascii_case(q)) {
            keywords.push(q.to_string());
        }
    }
    let theme = brief
        .categories
        .iter()
        .find(|c| {
            matches!(
                c.id.to_ascii_lowercase().as_str(),
                "technology" | "tech" | "magic" | "adventure" | "transportation"
            )
        })
        .map(|c| match c.id.to_ascii_lowercase().as_str() {
            "technology" | "tech" => "tech".into(),
            "magic" => "magic".into(),
            "adventure" => "adventure-and-rpg".into(),
            "transportation" => "exploration".into(),
            other => other.to_string(),
        });
    crate::modpack_index::MpiSearchQuery {
        loader: Some(brief.loader.clone()),
        version: Some(brief.mc_version.clone()),
        theme,
        keywords,
    }
}

/// Merge Modpack Index hints into mustHave (skip duplicates).
pub fn merge_mpi_hints_into_brief(
    brief: &mut CreateModeBrief,
    hints: &[crate::modpack_index::MpiModHint],
    max_extra: usize,
) {
    let existing: HashSet<String> = brief
        .must_have
        .iter()
        .flat_map(|m| {
            [
                m.query.to_ascii_lowercase(),
                m.slug_hint.as_deref().unwrap_or("").to_ascii_lowercase(),
            ]
        })
        .filter(|s| !s.is_empty())
        .collect();
    let mut added = 0usize;
    for h in hints {
        if added >= max_extra {
            break;
        }
        let slug_l = h.slug.to_ascii_lowercase();
        let name_l = h.name.to_ascii_lowercase();
        if existing.contains(&slug_l) || existing.contains(&name_l) {
            continue;
        }
        // Prefer keyword-sourced hits (more specific than broad theme lists).
        if !h.source.starts_with("keyword:") && !h.source.starts_with("theme-name:") {
            continue;
        }
        brief.must_have.push(MustHaveSpec {
            query: h.name.clone(),
            slug_hint: if h.slug.is_empty() {
                None
            } else {
                Some(h.slug.clone())
            },
            reason: format!("Catalog hint ({})", h.source),
        });
        added += 1;
    }
}

fn strip_json_fences(raw: &str) -> &str {
    let mut s = raw.trim();
    if let Some(rest) = s.strip_prefix("```json") {
        s = rest;
    } else if let Some(rest) = s.strip_prefix("```") {
        s = rest;
    }
    s.trim_end_matches("```").trim()
}

fn normalize_brief(mut brief: CreateModeBrief) -> CreateModeBrief {
    brief.loader = brief.loader.trim().to_ascii_lowercase();
    brief.target_count = brief.target_count.clamp(40, 120);
    if brief.categories.is_empty() {
        brief.categories = default_categories(brief.target_count);
    } else {
        // Scale category budgets to approximately match target_count.
        let sum: u32 = brief.categories.iter().map(|c| c.count.max(1)).sum();
        if sum > 0 && sum != brief.target_count {
            let mut allocated = 0u32;
            let n = brief.categories.len();
            for (i, cat) in brief.categories.iter_mut().enumerate() {
                if i + 1 == n {
                    cat.count = brief.target_count.saturating_sub(allocated).max(1);
                } else {
                    let scaled = ((cat.count.max(1) as f64) * (brief.target_count as f64)
                        / (sum as f64))
                        .round() as u32;
                    cat.count = scaled.max(1);
                    allocated = allocated.saturating_add(cat.count);
                }
            }
        }
        for cat in &mut brief.categories {
            if cat.facet.is_none() {
                cat.facet = modrinth_facet_for_id(&cat.id);
            }
            // Prefer short primary query tokens for Modrinth relevance.
            cat.query = prefer_short_query(&cat.query);
        }
        // Soft-cap library categories so packs aren't API-heavy.
        soft_cap_library_budget(&mut brief.categories, brief.target_count);
        let sum: u32 = brief.categories.iter().map(|c| c.count).sum();
        if sum < brief.target_count {
            let extra = brief.target_count - sum;
            if let Some(u) = brief
                .categories
                .iter_mut()
                .find(|c| matches!(c.id.as_str(), "utility" | "qol" | "technology" | "tech"))
            {
                u.count += extra;
            } else if let Some(first) = brief.categories.first_mut() {
                first.count += extra;
            }
        }
    }
    brief
}

/// Strict Intent-phase validation after normalize. Call before Catalog assemble.
pub fn validate_pack_brief(brief: &CreateModeBrief) -> Result<(), String> {
    if brief.title.trim().is_empty() {
        return Err("CreateModeBrief.title is empty".into());
    }
    let loader = brief.loader.trim().to_ascii_lowercase();
    if !matches!(
        loader.as_str(),
        "fabric" | "forge" | "neoforge" | "quilt" | "vanilla"
    ) {
        return Err(format!(
            "CreateModeBrief.loader must be fabric|forge|neoforge|quilt (got '{loader}')"
        ));
    }
    let ver = brief.mc_version.trim();
    if ver.is_empty() {
        return Err("CreateModeBrief.mcVersion is empty".into());
    }
    // Coarse Minecraft version: 1.20, 1.20.1, 1.21.4-pre…
    let ver_ok = ver.chars().next().is_some_and(|c| c.is_ascii_digit()) && ver.contains('.');
    if !ver_ok {
        return Err(format!(
            "CreateModeBrief.mcVersion looks invalid: '{ver}' (expected e.g. 1.20.1)"
        ));
    }
    if brief.must_have.is_empty() && brief.categories.is_empty() {
        return Err("CreateModeBrief needs mustHave and/or categories".into());
    }
    if !(40..=120).contains(&brief.target_count) {
        return Err(format!(
            "CreateModeBrief.targetCount out of range: {}",
            brief.target_count
        ));
    }
    Ok(())
}

/// Ollama Structured Outputs schema for Create Mode Intent JSON (`format` field).
/// OpenAI-compat path keeps `response_format: json_object` + validate_pack_brief.
pub fn create_mode_response_json_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "required": ["reply", "search", "brief"],
        "properties": {
            "reply": { "type": "string" },
            "search": {
                "type": "object",
                "properties": {
                    "loader": { "type": ["string", "null"] },
                    "version": { "type": ["string", "null"] },
                    "theme": { "type": ["string", "null"] },
                    "keywords": {
                        "type": "array",
                        "items": { "type": "string" }
                    }
                }
            },
            "brief": {
                "type": "object",
                "required": ["title", "mcVersion", "loader", "targetCount"],
                "properties": {
                    "title": { "type": "string" },
                    "mcVersion": { "type": "string" },
                    "loader": { "type": "string" },
                    "targetCount": { "type": "integer" },
                    "mustHave": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "required": ["query"],
                            "properties": {
                                "query": { "type": "string" },
                                "slugHint": { "type": ["string", "null"] },
                                "reason": { "type": "string" }
                            }
                        }
                    },
                    "categories": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "required": ["id", "query", "count"],
                            "properties": {
                                "id": { "type": "string" },
                                "query": { "type": "string" },
                                "count": { "type": "integer" },
                                "reason": { "type": "string" }
                            }
                        }
                    },
                    "exclude": {
                        "type": "array",
                        "items": { "type": "string" }
                    }
                }
            }
        }
    })
}

/// Compact candidate line for Rank prompt (no URLs / file ids).
pub fn format_candidates_for_rank(mods: &[PackDraftMod], limit: usize) -> String {
    mods.iter()
        .take(limit)
        .enumerate()
        .map(|(i, m)| {
            format!(
                "{}. {} ({}) [{}] — {}",
                i + 1,
                m.name,
                m.slug,
                m.provider,
                if m.reason.is_empty() {
                    m.category.as_str()
                } else {
                    m.reason.as_str()
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn prefer_short_query(query: &str) -> String {
    let parts: Vec<&str> = query.split_whitespace().collect();
    if parts.len() <= 3 {
        return query.trim().to_string();
    }
    // Keep first 3 meaningful tokens; long bags of words hurt Modrinth search.
    parts.into_iter().take(3).collect::<Vec<_>>().join(" ")
}

/// Well-known mod names detected as must-have when present in a free-text prompt.
const KNOWN_MUST_HAVE: &[&str] = &[
    "create",
    "jei",
    "rei",
    "emi",
    "sodium",
    "iris",
    "lithium",
    "starlight",
    "ferritecore",
    "mekanism",
    "botania",
    "thermal",
    "immersive",
    "ae2",
    "applied energistics",
    "farmers delight",
    "farmersdelight",
    "supplementaries",
    "quark",
    "appleskin",
    "jade",
    "wthit",
    "xaeros",
    "journeymap",
    "voxelmap",
    "cloth config",
    "modmenu",
    "fabric api",
    "forge config",
];

/// Build a CreateModeBrief without an LLM: default category budgets + must-haves from known names / quotes.
pub fn brief_from_prompt(
    prompt: &str,
    mc_version: &str,
    loader: &str,
    target_count: u32,
) -> CreateModeBrief {
    let prompt = prompt.trim();
    let target = target_count.clamp(40, 120);
    let title = {
        let t: String = prompt.chars().take(48).collect();
        let t = t.trim();
        if t.is_empty() {
            "Pack draft".into()
        } else {
            t.to_string()
        }
    };
    let must_have = extract_must_haves(prompt);
    normalize_brief(CreateModeBrief {
        title,
        mc_version: mc_version.trim().to_string(),
        loader: loader.trim().to_ascii_lowercase(),
        target_count: target,
        must_have,
        categories: default_categories(target),
        exclude: Vec::new(),
    })
}

fn extract_must_haves(prompt: &str) -> Vec<MustHaveSpec> {
    let lower = prompt.to_ascii_lowercase();
    let mut out: Vec<MustHaveSpec> = Vec::new();
    let mut seen = HashSet::new();

    // Quoted phrases first (user intent).
    for q in extract_quoted_phrases(prompt) {
        let key = q.to_ascii_lowercase();
        if key.len() < 2 || !seen.insert(key) {
            continue;
        }
        out.push(MustHaveSpec {
            query: q,
            slug_hint: None,
            reason: "Quoted in prompt".into(),
        });
    }

    // Known mods: longest match first so "applied energistics" wins over shorter tokens.
    let mut known: Vec<&str> = KNOWN_MUST_HAVE.to_vec();
    known.sort_by_key(|s| std::cmp::Reverse(s.len()));
    for name in known {
        if !lower.contains(name) {
            continue;
        }
        let key = name.to_string();
        if !seen.insert(key) {
            continue;
        }
        out.push(MustHaveSpec {
            query: name.to_string(),
            slug_hint: None,
            reason: "Mentioned in prompt".into(),
        });
    }
    out
}

fn extract_quoted_phrases(prompt: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = prompt;
    while let Some(start) = rest.find('"') {
        rest = &rest[start + 1..];
        if let Some(end) = rest.find('"') {
            let phrase = rest[..end].trim();
            if !phrase.is_empty() {
                out.push(phrase.to_string());
            }
            rest = &rest[end + 1..];
        } else {
            break;
        }
    }
    out
}

fn soft_cap_library_budget(cats: &mut [CategoryBudget], target: u32) {
    let max_lib = ((target as f64) * 0.12).round() as u32;
    let max_lib = max_lib.max(4);
    let mut lib_total = 0u32;
    for cat in cats.iter_mut() {
        if is_library_category_id(&cat.id) {
            if lib_total >= max_lib {
                cat.count = 0;
            } else if lib_total + cat.count > max_lib {
                cat.count = max_lib - lib_total;
                lib_total = max_lib;
            } else {
                lib_total += cat.count;
            }
        }
    }
}

fn is_library_category_id(id: &str) -> bool {
    matches!(
        id.to_ascii_lowercase().as_str(),
        "library" | "libraries" | "api" | "lib"
    )
}

/// Map Create Mode category ids → Modrinth `categories:` facet values.
pub fn modrinth_facet_for_id(id: &str) -> Option<String> {
    let key = id.trim().to_ascii_lowercase().replace(' ', "-");
    let mapped = match key.as_str() {
        "tech" | "technology" | "automation" => "technology",
        "magic" | "magic-and-spells" => "magic",
        "decoration" | "decor" | "furniture" | "building" => "decoration",
        "qol" | "utility" | "quality-of-life" | "qualityoflife" => "utility",
        "adventure" | "rpg" | "exploration" => "adventure",
        "library" | "libraries" | "api" | "lib" => "library",
        "worldgen" | "world-generation" | "biomes" => "worldgen",
        "storage" => "storage",
        "food" | "farming" | "agriculture" => "food",
        "equipment" | "armor" | "tools" => "equipment",
        "transport" | "transportation" | "vehicles" => "transportation",
        "social" | "multiplayer" => "social",
        "management" | "server" => "management",
        "mobs" | "entities" => "mobs",
        "game-mechanics" | "mechanics" => "game-mechanics",
        other if !other.is_empty() && other != "fill" && other != "musthave" => other,
        _ => return None,
    };
    Some(mapped.to_string())
}

pub fn default_categories(target: u32) -> Vec<CategoryBudget> {
    // Relative weights for a balanced kitchen-sink pack (Modrinth-aligned).
    let weights: &[(&str, &str, &str, u32)] = &[
        ("technology", "create", "technology", 18),
        ("magic", "magic", "magic", 12),
        ("decoration", "furniture", "decoration", 12),
        ("utility", "jei", "utility", 14),
        ("adventure", "adventure", "adventure", 10),
        ("worldgen", "biomes", "worldgen", 8),
        ("storage", "storage", "storage", 8),
        ("food", "farming", "food", 6),
        ("equipment", "armor", "equipment", 5),
        ("library", "api", "library", 7),
    ];
    let weight_sum: u32 = weights.iter().map(|(_, _, _, w)| *w).sum();
    let mut cats = Vec::with_capacity(weights.len());
    let mut allocated = 0u32;
    for (i, (id, query, facet, w)) in weights.iter().enumerate() {
        let count = if i + 1 == weights.len() {
            target.saturating_sub(allocated).max(1)
        } else {
            let c = ((*w as f64) * (target as f64) / (weight_sum as f64)).round() as u32;
            c.max(1)
        };
        allocated = allocated.saturating_add(count);
        cats.push(CategoryBudget {
            id: id.to_string(),
            query: query.to_string(),
            count,
            reason: format!("Default {id} budget"),
            facet: Some(facet.to_string()),
        });
    }
    soft_cap_library_budget(&mut cats, target);
    // Rebalance leftover after library soft-cap into utility/technology.
    let sum: u32 = cats.iter().map(|c| c.count).sum();
    if sum < target {
        let extra = target - sum;
        if let Some(u) = cats.iter_mut().find(|c| c.id == "utility") {
            u.count += extra;
        } else if let Some(t) = cats.iter_mut().find(|c| c.id == "technology") {
            t.count += extra;
        }
    }
    cats
}

/// Abstraction over Modrinth search for unit tests.
pub trait ModSearch: Sync {
    fn search(&self, query: &ProviderSearchQuery) -> Result<SearchPage, String>;
    fn get_project(&self, id_or_slug: &str) -> Result<ProjectInfo, String>;
}

pub struct LiveModrinthSearch(pub ModrinthProvider);

impl ModSearch for LiveModrinthSearch {
    fn search(&self, query: &ProviderSearchQuery) -> Result<SearchPage, String> {
        self.0.search(query).map_err(|e| e.to_string())
    }
    fn get_project(&self, id_or_slug: &str) -> Result<ProjectInfo, String> {
        self.0.get_project(id_or_slug).map_err(|e| e.to_string())
    }
}

/// Modrinth-first catalog search with CurseForge → Modrinth slug bridge.
/// Install path stays Modrinth: CF hits are only used to discover a Modrinth slug.
pub struct LiveCatalogSearch {
    pub modrinth: ModrinthProvider,
    pub curseforge: crate::provider::curseforge::CurseForgeProvider,
}

impl Default for LiveCatalogSearch {
    fn default() -> Self {
        Self {
            modrinth: ModrinthProvider::new(),
            curseforge: crate::provider::curseforge::CurseForgeProvider::new(),
        }
    }
}

impl LiveCatalogSearch {
    pub fn new() -> Self {
        Self::default()
    }

    fn resolve_cf_to_modrinth(&self, query: &ProviderSearchQuery) -> Result<SearchPage, String> {
        let q = query.query.as_deref().unwrap_or("").trim();
        if q.is_empty() {
            return Ok(SearchPage {
                results: vec![],
                total: 0,
            });
        }
        let loader = query
            .loader
            .as_deref()
            .and_then(crate::provider::curseforge::CurseForgeProvider::mod_loader_type);
        let page = self
            .curseforge
            .search_content(
                6, // CLASS_MOD
                q,
                query.minecraft_version.as_deref(),
                loader,
                query.offset.unwrap_or(0),
                query.limit.unwrap_or(10).clamp(1, 20),
                Some(2), // popularity
                None,
            )
            .map_err(|e| e.to_string())?;

        let mut results = Vec::new();
        let mut seen = HashSet::new();
        for hit in page.hits.into_iter().take(8) {
            let slug = hit.slug.trim();
            if slug.is_empty() || !seen.insert(slug.to_ascii_lowercase()) {
                continue;
            }
            // Bridge: same slug on Modrinth → keep install_pack_draft Modrinth-only.
            if let Ok(proj) = self.modrinth.get_project(slug) {
                results.push(proj);
            } else {
                // CF-only: keep numeric id so install_pack_draft can use CurseForge.
                results.push(ProjectInfo {
                    id: hit.id.to_string(),
                    slug: if slug.is_empty() {
                        format!("cf-{}", hit.id)
                    } else {
                        hit.slug.clone()
                    },
                    name: hit.name.clone(),
                    description: format!("curseforge:{}", hit.id),
                    project_type: "mod".into(),
                    icon_url: hit.icon_url.clone(),
                    author: hit.authors.first().cloned(),
                    downloads: Some(hit.download_count),
                    follows: Some(hit.thumbs_up_count),
                    date_modified: hit.date_modified.clone(),
                    categories: hit.categories.clone(),
                    license: None,
                    client_side: None,
                    server_side: None,
                    issues_url: None,
                    source_url: None,
                    date_created: None,
                });
            }
        }
        let total = results.len() as u32;
        Ok(SearchPage { results, total })
    }
}

impl ModSearch for LiveCatalogSearch {
    fn search(&self, query: &ProviderSearchQuery) -> Result<SearchPage, String> {
        let page = self.modrinth.search(query).map_err(|e| e.to_string())?;
        if !page.results.is_empty() {
            return Ok(page);
        }
        self.resolve_cf_to_modrinth(query)
    }

    fn get_project(&self, id_or_slug: &str) -> Result<ProjectInfo, String> {
        match self.modrinth.get_project(id_or_slug) {
            Ok(p) => Ok(p),
            Err(mr_err) => {
                // CF lookup by slug → retry Modrinth with CF slug (often identical).
                let loader = None;
                if let Ok(page) =
                    self.curseforge
                        .search_content(6, id_or_slug, None, loader, 0, 5, Some(2), None)
                {
                    for hit in page.hits {
                        if hit.slug.eq_ignore_ascii_case(id_or_slug)
                            || hit.name.eq_ignore_ascii_case(id_or_slug)
                        {
                            if let Ok(p) = self.modrinth.get_project(&hit.slug) {
                                return Ok(p);
                            }
                        }
                    }
                }
                Err(mr_err.to_string())
            }
        }
    }
}

pub struct AssembleOptions<'a> {
    pub brief: &'a CreateModeBrief,
    pub installed_ids: HashSet<String>,
    pub max_pages_per_category: u32,
    pub page_size: u32,
    pub on_progress: Option<&'a mut dyn FnMut(&str, usize, usize, &str)>,
}

fn base_query(
    brief: &CreateModeBrief,
    query: Option<String>,
    category: Option<String>,
) -> ProviderSearchQuery {
    ProviderSearchQuery {
        query,
        minecraft_version: Some(brief.mc_version.clone()),
        loader: Some(brief.loader.clone()),
        sort: Some("downloads".into()),
        limit: None,
        project_type: Some("mod".into()),
        offset: None,
        category,
        ..Default::default()
    }
}

fn category_search_plans(
    cat: &CategoryBudget,
) -> Vec<(Option<String>, Option<String>, &'static str)> {
    // (query, facet, sort) — try focused strategies until budget filled.
    let mut plans = Vec::new();
    let facet = cat.facet.clone().or_else(|| modrinth_facet_for_id(&cat.id));
    let primary = cat.query.trim().to_string();

    if !primary.is_empty() {
        plans.push((Some(primary.clone()), facet.clone(), "downloads"));
        plans.push((Some(primary.clone()), facet.clone(), "relevance"));
    }
    // Facet-only popular mods in this Modrinth category.
    if let Some(f) = &facet {
        plans.push((None, Some(f.clone()), "downloads"));
    }
    // Token fallbacks (e.g. "create mekanism tech" → "create", "mekanism").
    for token in primary.split_whitespace() {
        let t = token.trim();
        if t.len() < 3 {
            continue;
        }
        if primary.eq_ignore_ascii_case(t) {
            continue;
        }
        plans.push((Some(t.to_string()), facet.clone(), "downloads"));
    }
    // Last resort: query without facet.
    if !primary.is_empty() {
        plans.push((Some(primary), None, "downloads"));
    }
    plans
}

fn fill_search_plans() -> Vec<(Option<String>, Option<String>)> {
    // Diverse top-up queries so fill isn't 100% Sodium/Iris clones.
    [
        (None, Some("utility")),
        (None, Some("decoration")),
        (None, Some("adventure")),
        (None, Some("technology")),
        (None, Some("worldgen")),
        (None, Some("storage")),
        (None, Some("food")),
        (Some("quality of life"), Some("utility")),
        (Some("performance"), Some("utility")),
        (None, None),
    ]
    .into_iter()
    .map(|(q, f)| (q.map(str::to_string), f.map(str::to_string)))
    .collect()
}

fn must_have_score(query: &str, p: &ProjectInfo) -> u64 {
    let q = query.trim().to_ascii_lowercase();
    if q.is_empty() {
        return p.downloads.unwrap_or(0);
    }
    let slug = p.slug.to_ascii_lowercase();
    let name = p.name.to_ascii_lowercase();
    let mut score = 0u64;
    if slug == q || name == q {
        score += 50_000;
    }
    if slug.replace('-', " ") == q || name == q {
        score += 40_000;
    }
    if slug.contains(&q) || name.contains(&q) {
        score += 10_000;
    }
    for token in q.split_whitespace() {
        if token.len() < 2 {
            continue;
        }
        if slug.contains(token) || name.contains(token) {
            score += 2_000;
        }
    }
    // Prefer well-known mods when ties.
    score + p.downloads.unwrap_or(0).min(5_000_000) / 1_000
}

fn pick_best_must_have(query: &str, results: Vec<ProjectInfo>) -> Option<ProjectInfo> {
    results
        .into_iter()
        .max_by_key(|p| must_have_score(query, p))
}

fn is_library_mod(p: &ProjectInfo) -> bool {
    let cats = p
        .categories
        .iter()
        .map(|c| c.to_ascii_lowercase())
        .collect::<Vec<_>>();
    if cats.iter().any(|c| c == "library") {
        return true;
    }
    let slug = p.slug.to_ascii_lowercase();
    let name = p.name.to_ascii_lowercase();
    slug.ends_with("-api")
        || slug.ends_with("-lib")
        || name.ends_with(" api")
        || (name.contains(" library") && !name.contains("librarian"))
}

fn try_add_mod(
    mods: &mut Vec<PackDraftMod>,
    seen: &mut HashSet<String>,
    exclude: &HashSet<String>,
    p: ProjectInfo,
    reason: String,
    category: String,
    target: usize,
    library_cap: usize,
    library_count: &mut usize,
    allow_library: bool,
) -> bool {
    if mods.len() >= target {
        return false;
    }
    let id_key = p.id.to_ascii_lowercase();
    let slug_key = p.slug.to_ascii_lowercase();
    if seen.contains(&id_key) || seen.contains(&slug_key) || is_excluded(&p, exclude) {
        return false;
    }
    let lib = is_library_mod(&p);
    if lib {
        if !allow_library || *library_count >= library_cap {
            return false;
        }
    }
    seen.insert(id_key);
    seen.insert(slug_key);
    if lib {
        *library_count += 1;
    }
    mods.push(pack_draft_mod_from_project(p, reason, category));
    true
}

fn search_pages<S: ModSearch>(
    searcher: &S,
    mut q: ProviderSearchQuery,
    page_size: u32,
    max_pages: u32,
    sort: &str,
) -> Result<Vec<ProjectInfo>, String> {
    q.limit = Some(page_size);
    q.sort = Some(sort.to_string());
    let mut out = Vec::new();
    for page_idx in 0..max_pages {
        q.offset = Some(page_idx * page_size);
        let page = searcher.search(&q)?;
        if page.results.is_empty() {
            break;
        }
        out.extend(page.results);
        if out.len() as u32 >= page.total && page.total > 0 {
            break;
        }
    }
    Ok(out)
}

pub fn assemble_pack_draft<S: ModSearch>(
    searcher: &S,
    opts: AssembleOptions<'_>,
) -> Result<PackDraft, String> {
    use rayon::prelude::*;

    let brief = opts.brief;
    validate_pack_brief(brief)?;
    let target = brief.target_count.clamp(40, 120) as usize;
    let page_size = opts.page_size.clamp(1, 100);
    let max_pages = opts.max_pages_per_category.max(1);
    let library_cap = ((target as f64) * 0.15).round() as usize;
    let library_cap = library_cap.max(6).min(target / 4).max(4);

    let exclude: HashSet<String> = brief
        .exclude
        .iter()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect();

    let mut seen: HashSet<String> = opts
        .installed_ids
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .collect();
    let mut mods: Vec<PackDraftMod> = Vec::new();
    let mut unresolved: Vec<String> = Vec::new();
    let mut library_count = 0usize;

    let report = |on_progress: &mut Option<&mut dyn FnMut(&str, usize, usize, &str)>,
                  phase: &str,
                  done: usize,
                  total: usize,
                  current: &str| {
        if let Some(cb) = on_progress.as_mut() {
            cb(phase, done, total, current);
        }
    };

    let mut on_progress = opts.on_progress;
    let work_units = brief.must_have.len() + brief.categories.len() + 1;
    let mut done_units = 0usize;

    report(
        &mut on_progress,
        "catalog",
        0,
        work_units.max(1),
        "Resolving must-have…",
    );

    // 1) Parallel mustHave HTTP lookups, then sequential merge into draft.
    let must_results: Vec<(usize, String, String, Option<ProjectInfo>)> = brief
        .must_have
        .par_iter()
        .enumerate()
        .map(|(idx, mh)| {
            let mut found = None;
            if let Some(hint) = mh.slug_hint.as_deref().filter(|s| !s.trim().is_empty()) {
                if let Ok(p) = searcher.get_project(hint.trim()) {
                    found = Some(p);
                }
            }
            if found.is_none() {
                let mut q = base_query(brief, Some(mh.query.clone()), None);
                q.limit = Some(page_size.min(25));
                q.offset = Some(0);
                q.sort = Some("relevance".into());
                if let Ok(page) = searcher.search(&q) {
                    found = pick_best_must_have(&mh.query, page.results);
                }
            }
            if found.is_none() {
                let mut q = base_query(brief, Some(mh.query.clone()), None);
                q.limit = Some(page_size.min(25));
                q.offset = Some(0);
                q.sort = Some("downloads".into());
                if let Ok(page) = searcher.search(&q) {
                    found = pick_best_must_have(&mh.query, page.results);
                }
            }
            let reason = if mh.reason.is_empty() {
                "Must-have".into()
            } else {
                mh.reason.clone()
            };
            (idx, mh.query.clone(), reason, found)
        })
        .collect();

    for (_idx, query, reason, found) in must_results {
        if mods.len() >= target {
            break;
        }
        report(
            &mut on_progress,
            "catalog",
            done_units,
            work_units.max(1),
            &query,
        );
        match found {
            Some(p) => {
                if !force_push_must_have(
                    &mut mods,
                    &mut seen,
                    &exclude,
                    p,
                    reason,
                    &mut library_count,
                ) {
                    unresolved.push(query);
                }
            }
            None => unresolved.push(query),
        }
        done_units += 1;
    }

    // 2) Parallel category page fetches (first plan each), then sequential fill.
    report(
        &mut on_progress,
        "catalog",
        done_units,
        work_units.max(1),
        "Searching categories…",
    );

    let cat_hits: Vec<(usize, String, String, bool, Vec<ProjectInfo>)> = brief
        .categories
        .par_iter()
        .enumerate()
        .map(|(idx, cat)| {
            let allow_lib = is_library_category_id(&cat.id);
            let reason = if cat.reason.is_empty() {
                format!("Category {}", cat.id)
            } else {
                cat.reason.clone()
            };
            let mut hits = Vec::new();
            if cat.count > 0 {
                for (query, facet, sort) in category_search_plans(cat) {
                    let pages = max_pages + 1;
                    if let Ok(page_hits) = search_pages(
                        searcher,
                        base_query(brief, query, facet),
                        page_size,
                        pages,
                        sort,
                    ) {
                        hits.extend(page_hits);
                    }
                    if hits.len() >= (cat.count as usize).saturating_mul(3) {
                        break;
                    }
                }
            }
            (idx, cat.id.clone(), reason, allow_lib, hits)
        })
        .collect();

    let mut shortfall = 0usize;
    for (idx, cat_id, reason, allow_lib, hits) in cat_hits {
        if mods.len() >= target {
            break;
        }
        let cat = &brief.categories[idx];
        if cat.count == 0 {
            done_units += 1;
            continue;
        }
        report(
            &mut on_progress,
            "catalog",
            done_units,
            work_units.max(1),
            &cat.query,
        );
        let want = cat.count.max(1) as usize;
        let mut taken = 0usize;
        for p in hits {
            if mods.len() >= target || taken >= want {
                break;
            }
            let allow = allow_lib || library_count < library_cap;
            if try_add_mod(
                &mut mods,
                &mut seen,
                &exclude,
                p,
                reason.clone(),
                cat_id.clone(),
                target,
                library_cap,
                &mut library_count,
                allow,
            ) {
                taken += 1;
            }
        }
        if taken < want {
            shortfall += want - taken;
        }
        done_units += 1;
    }

    // 3) Diverse top-up (facet rotations) if under target / category shortfall
    if mods.len() < target {
        report(
            &mut on_progress,
            "catalog",
            done_units,
            work_units.max(1),
            "diverse fill",
        );
        let fill_pages = max_pages.max(3) + if shortfall > 20 { 1 } else { 0 };
        for (query, facet) in fill_search_plans() {
            if mods.len() >= target {
                break;
            }
            let hits = search_pages(
                searcher,
                base_query(brief, query.clone(), facet.clone()),
                page_size,
                fill_pages,
                "downloads",
            )?;
            for p in hits {
                if mods.len() >= target {
                    break;
                }
                let allow_lib = library_count < library_cap / 2;
                try_add_mod(
                    &mut mods,
                    &mut seen,
                    &exclude,
                    p,
                    "Fill to reach target count".into(),
                    "fill".into(),
                    target,
                    library_cap,
                    &mut library_count,
                    allow_lib,
                );
            }
        }
    }

    if mods.len() > target {
        mods.truncate(target);
    }

    report(
        &mut on_progress,
        "catalog",
        work_units.max(1),
        work_units.max(1),
        "done",
    );

    Ok(PackDraft {
        brief: brief.clone(),
        mods,
        unresolved,
    })
}

/// Resolve a must-have even when library cap would block it (exact insert).
fn force_push_must_have(
    mods: &mut Vec<PackDraftMod>,
    seen: &mut HashSet<String>,
    exclude: &HashSet<String>,
    p: ProjectInfo,
    reason: String,
    library_count: &mut usize,
) -> bool {
    let id_key = p.id.to_ascii_lowercase();
    let slug_key = p.slug.to_ascii_lowercase();
    if seen.contains(&id_key) || seen.contains(&slug_key) || is_excluded(&p, exclude) {
        return false;
    }
    if is_library_mod(&p) {
        *library_count += 1;
    }
    seen.insert(id_key);
    seen.insert(slug_key);
    mods.push(pack_draft_mod_from_project(p, reason, "mustHave".into()));
    true
}

fn is_excluded(p: &ProjectInfo, exclude: &HashSet<String>) -> bool {
    exclude.contains(&p.slug.to_ascii_lowercase())
        || exclude.contains(&p.id.to_ascii_lowercase())
        || exclude.contains(&p.name.to_ascii_lowercase())
}

pub fn installed_mod_keys(manifest: &crate::manifest::ProjectManifest) -> HashSet<String> {
    let mut set = HashSet::new();
    for m in &manifest.mods {
        set.insert(m.id.to_ascii_lowercase());
        if let Some(pid) = &m.source.project_id {
            set.insert(pid.to_ascii_lowercase());
        }
    }
    set
}

// --- Chat persistence under project/.tuffbox/chats/ ---

pub fn chats_dir(project_dir: &Path) -> PathBuf {
    project_dir.join(".tuffbox").join("chats")
}

pub fn list_create_chats(project_dir: &Path) -> Result<Vec<CreateChatSession>, String> {
    let dir = chats_dir(project_dir);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut sessions = Vec::new();
    let entries = std::fs::read_dir(&dir).map_err(|e| e.to_string())?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        if let Ok(session) = serde_json::from_str::<CreateChatSession>(&text) {
            sessions.push(session);
        }
    }
    sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(sessions)
}

pub fn save_create_chat(
    project_dir: &Path,
    session: &CreateChatSession,
) -> Result<PathBuf, String> {
    let dir = chats_dir(project_dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{}.json", session.id));
    let text = serde_json::to_string_pretty(session).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| e.to_string())?;
    Ok(path)
}

pub fn load_create_chat(project_dir: &Path, chat_id: &str) -> Result<CreateChatSession, String> {
    let path = chats_dir(project_dir).join(format!("{chat_id}.json"));
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

pub fn delete_create_chat(project_dir: &Path, chat_id: &str) -> Result<(), String> {
    let path = chats_dir(project_dir).join(format!("{chat_id}.json"));
    if path.exists() {
        std::fs::remove_file(path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn new_chat_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("chat-{ms}")
}

pub fn now_iso() -> String {
    // Simple UTC-ish stamp without chrono dependency.
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ProjectInfo;

    struct MockSearch {
        by_query: Vec<(String, Vec<ProjectInfo>)>,
        projects: Vec<ProjectInfo>,
    }

    impl ModSearch for MockSearch {
        fn search(&self, query: &ProviderSearchQuery) -> Result<SearchPage, String> {
            let q = query.query.clone().unwrap_or_default().to_ascii_lowercase();
            let facet = query.category.as_deref().unwrap_or("").to_ascii_lowercase();
            // Prefer exact query key; empty query matches fill/"", optionally filtered by facet tag in key "facet:utility".
            for (key, hits) in &self.by_query {
                let key_l = key.to_ascii_lowercase();
                let matches_query = if q.is_empty() {
                    key_l.is_empty() || key_l.starts_with("facet:")
                } else {
                    q.contains(&key_l) || key_l.contains(&q)
                };
                let matches_facet = facet.is_empty()
                    || key_l == format!("facet:{facet}")
                    || !key_l.starts_with("facet:");
                if matches_query && matches_facet {
                    let mut filtered = hits.clone();
                    if !facet.is_empty() {
                        filtered.retain(|p| {
                            p.categories.iter().any(|c| c.eq_ignore_ascii_case(&facet))
                                || p.categories.is_empty()
                        });
                    }
                    let offset = query.offset.unwrap_or(0) as usize;
                    let limit = query.limit.unwrap_or(24) as usize;
                    let slice: Vec<_> = filtered.into_iter().skip(offset).take(limit).collect();
                    return Ok(SearchPage {
                        total: hits.len() as u32,
                        results: slice,
                    });
                }
            }
            Ok(SearchPage {
                total: 0,
                results: vec![],
            })
        }
        fn get_project(&self, id_or_slug: &str) -> Result<ProjectInfo, String> {
            self.projects
                .iter()
                .find(|p| p.id == id_or_slug || p.slug == id_or_slug)
                .cloned()
                .ok_or_else(|| format!("missing {id_or_slug}"))
        }
    }

    fn fake_mod(id: &str, slug: &str, name: &str, downloads: u64) -> ProjectInfo {
        fake_mod_cats(id, slug, name, downloads, &[])
    }

    fn fake_mod_cats(
        id: &str,
        slug: &str,
        name: &str,
        downloads: u64,
        categories: &[&str],
    ) -> ProjectInfo {
        ProjectInfo {
            id: id.into(),
            slug: slug.into(),
            name: name.into(),
            description: String::new(),
            project_type: "mod".into(),
            icon_url: None,
            author: None,
            downloads: Some(downloads),
            follows: None,
            date_modified: None,
            categories: categories.iter().map(|s| (*s).to_string()).collect(),
            license: None,
            client_side: None,
            server_side: None,
            issues_url: None,
            source_url: None,
            date_created: None,
        }
    }

    #[test]
    fn system_prompt_requires_user_language() {
        assert!(
            CREATE_MODE_SYSTEM_PROMPT.contains("same language as the user's latest message"),
            "Create Mode must pin reply language to the user message"
        );
    }

    #[test]
    fn validate_pack_brief_rejects_bad_loader_and_version() {
        let ok = CreateModeBrief {
            title: "Industrial".into(),
            mc_version: "1.20.1".into(),
            loader: "fabric".into(),
            target_count: 80,
            must_have: vec![MustHaveSpec {
                query: "create".into(),
                slug_hint: None,
                reason: "".into(),
            }],
            categories: vec![],
            exclude: vec![],
        };
        assert!(validate_pack_brief(&ok).is_ok());

        let mut bad = ok.clone();
        bad.loader = "spigot".into();
        assert!(validate_pack_brief(&bad).is_err());

        bad = ok.clone();
        bad.mc_version = "twenty".into();
        assert!(validate_pack_brief(&bad).is_err());

        bad = ok.clone();
        bad.must_have.clear();
        bad.categories.clear();
        assert!(validate_pack_brief(&bad).is_err());
    }

    #[test]
    fn prompts_preserve_locked_and_excluded_entries() {
        assert!(
            CREATE_MODE_SYSTEM_PROMPT.contains("Locked by user"),
            "system prompt must instruct the model to preserve user-locked mustHave entries"
        );
        assert!(
            CREATE_MODE_REFINE_PROMPT.contains("Locked by user"),
            "refine prompt must instruct the model to preserve user-locked mustHave entries"
        );
        assert!(
            CREATE_MODE_SYSTEM_PROMPT.contains("preserved verbatim")
                && CREATE_MODE_REFINE_PROMPT.contains("preserved verbatim"),
            "both prompts must require verbatim preservation of manual lock/exclude choices"
        );
    }

    #[test]
    fn parse_brief_bare_and_wrapped() {
        let bare = r#"{
            "title": "Tech",
            "mcVersion": "1.20.1",
            "loader": "fabric",
            "targetCount": 80,
            "mustHave": [],
            "categories": [{"id":"tech","query":"create","count":80,"reason":"x"}],
            "exclude": []
        }"#;
        let b = parse_pack_brief(bare).unwrap();
        assert_eq!(b.title, "Tech");
        assert_eq!(b.target_count, 80);
        assert_eq!(b.categories[0].facet.as_deref(), Some("technology"));

        let wrapped = r#"{"reply":"ok","brief":{"title":"Magic","mcVersion":"1.21","loader":"neoforge","targetCount":60,"mustHave":[],"categories":[],"exclude":[]}}"#;
        let b2 = parse_pack_brief(wrapped).unwrap();
        assert_eq!(b2.title, "Magic");
        assert!(!b2.categories.is_empty()); // defaults filled
    }

    #[test]
    fn brief_from_prompt_picks_must_haves() {
        let b = brief_from_prompt("tech with Create and JEI", "1.20.1", "fabric", 80);
        assert!(!b.categories.is_empty());
        assert_eq!(b.mc_version, "1.20.1");
        assert_eq!(b.loader, "fabric");
        assert_eq!(b.target_count, 80);
        let queries: Vec<&str> = b.must_have.iter().map(|m| m.query.as_str()).collect();
        assert!(queries.iter().any(|q| q.eq_ignore_ascii_case("create")));
        assert!(queries.iter().any(|q| q.eq_ignore_ascii_case("jei")));
        assert!(b.title.to_ascii_lowercase().contains("tech"));
    }

    #[test]
    fn assembler_dedupes_and_fills_target() {
        let mut hits = Vec::new();
        for i in 0..120 {
            hits.push(fake_mod(
                &format!("id{i}"),
                &format!("mod-{i}"),
                &format!("Mod {i}"),
                1000 - i as u64,
            ));
        }
        // Duplicate id0 under another query path
        let searcher = MockSearch {
            by_query: vec![
                ("sodium".into(), vec![hits[0].clone()]),
                ("".into(), hits.clone()),
                ("technology".into(), hits.clone()),
            ],
            projects: vec![hits[0].clone()],
        };

        let brief = CreateModeBrief {
            title: "Test".into(),
            mc_version: "1.20.1".into(),
            loader: "fabric".into(),
            target_count: 50,
            must_have: vec![MustHaveSpec {
                query: "sodium".into(),
                slug_hint: Some("mod-0".into()),
                reason: "perf".into(),
            }],
            categories: vec![CategoryBudget {
                id: "tech".into(),
                query: "technology".into(),
                count: 40,
                reason: "tech".into(),
                facet: Some("technology".into()),
            }],
            exclude: vec!["mod-1".into()],
        };

        let draft = assemble_pack_draft(
            &searcher,
            AssembleOptions {
                brief: &brief,
                installed_ids: HashSet::from(["id2".into()]),
                max_pages_per_category: 2,
                page_size: 50,
                on_progress: None,
            },
        )
        .unwrap();

        assert_eq!(draft.mods.len(), 50);
        let ids: HashSet<_> = draft.mods.iter().map(|m| m.project_id.clone()).collect();
        assert_eq!(ids.len(), 50);
        assert!(!draft.mods.iter().any(|m| m.slug == "mod-1"));
        assert!(!draft.mods.iter().any(|m| m.project_id == "id2"));
        assert_eq!(draft.mods[0].category, "mustHave");
    }

    #[test]
    fn must_have_picks_best_name_match() {
        let brief = CreateModeBrief {
            title: "T".into(),
            mc_version: "1.20.1".into(),
            loader: "fabric".into(),
            target_count: 40,
            must_have: vec![MustHaveSpec {
                query: "create".into(),
                slug_hint: None,
                reason: "core".into(),
            }],
            categories: vec![CategoryBudget {
                id: "utility".into(),
                query: "utility".into(),
                count: 39,
                reason: "".into(),
                facet: Some("utility".into()),
            }],
            exclude: vec![],
        };
        let mut pool = Vec::new();
        for i in 0..80 {
            pool.push(fake_mod_cats(
                &format!("u{i}"),
                &format!("util-{i}"),
                &format!("Util {i}"),
                100,
                &["utility"],
            ));
        }
        let searcher = MockSearch {
            by_query: vec![
                (
                    "create".into(),
                    vec![
                        fake_mod("a", "create-steam-n-rails", "Create Steam n' Rails", 500),
                        fake_mod("b", "create", "Create", 9_000_000),
                        fake_mod("c", "create-craftables", "Create Craftables", 200),
                    ],
                ),
                ("utility".into(), pool.clone()),
                ("".into(), pool),
            ],
            projects: vec![],
        };
        let draft = assemble_pack_draft(
            &searcher,
            AssembleOptions {
                brief: &brief,
                installed_ids: HashSet::new(),
                max_pages_per_category: 2,
                page_size: 50,
                on_progress: None,
            },
        )
        .unwrap();
        assert_eq!(draft.mods[0].slug, "create");
        assert_eq!(draft.mods.len(), 40);
    }

    #[test]
    fn library_cap_limits_api_spam_in_fill() {
        let mut libs = Vec::new();
        for i in 0..40 {
            libs.push(fake_mod_cats(
                &format!("lib{i}"),
                &format!("thing-api-{i}"),
                &format!("Thing API {i}"),
                10_000 - i as u64,
                &["library"],
            ));
        }
        let mut utils = Vec::new();
        for i in 0..80 {
            utils.push(fake_mod_cats(
                &format!("u{i}"),
                &format!("util-{i}"),
                &format!("Util {i}"),
                5_000 - i as u64,
                &["utility"],
            ));
        }
        let searcher = MockSearch {
            by_query: vec![
                ("api".into(), libs.clone()),
                ("library".into(), libs),
                ("utility".into(), utils.clone()),
                ("".into(), utils),
            ],
            projects: vec![],
        };
        let brief = CreateModeBrief {
            title: "T".into(),
            mc_version: "1.20.1".into(),
            loader: "fabric".into(),
            target_count: 40,
            must_have: vec![],
            categories: vec![
                CategoryBudget {
                    id: "library".into(),
                    query: "api".into(),
                    count: 30,
                    reason: "".into(),
                    facet: Some("library".into()),
                },
                CategoryBudget {
                    id: "utility".into(),
                    query: "utility".into(),
                    count: 10,
                    reason: "".into(),
                    facet: Some("utility".into()),
                },
            ],
            exclude: vec![],
        };
        // normalize soft-caps library
        let brief = normalize_brief(brief);
        assert!(
            brief
                .categories
                .iter()
                .find(|c| c.id == "library")
                .unwrap()
                .count
                <= 5
        );

        let draft = assemble_pack_draft(
            &searcher,
            AssembleOptions {
                brief: &brief,
                installed_ids: HashSet::new(),
                max_pages_per_category: 2,
                page_size: 50,
                on_progress: None,
            },
        )
        .unwrap();
        assert_eq!(draft.mods.len(), 40);
        let lib_mods = draft
            .mods
            .iter()
            .filter(|m| m.slug.contains("-api-") || m.category == "library")
            .count();
        assert!(
            lib_mods <= 12,
            "expected library-heavy mods capped, got {lib_mods}"
        );
    }

    #[test]
    fn empty_draft_install_guard() {
        let draft = PackDraft {
            brief: CreateModeBrief {
                title: "x".into(),
                mc_version: "1.20.1".into(),
                loader: "fabric".into(),
                target_count: 40,
                must_have: vec![],
                categories: default_categories(40),
                exclude: vec![],
            },
            mods: vec![],
            unresolved: vec![],
        };
        assert!(draft.mods.is_empty());
    }

    #[test]
    fn catalog_cf_bridge_skips_empty_query() {
        let catalog = LiveCatalogSearch::new();
        let page = catalog
            .resolve_cf_to_modrinth(&ProviderSearchQuery {
                query: Some("  ".into()),
                ..Default::default()
            })
            .unwrap();
        assert!(page.results.is_empty());
    }
}

#[cfg(test)]
mod pack_theme_tests {
    #[test]
    fn pack_themes_are_nonempty_offline() {
        let cats = crate::modpack_index::list_pack_theme_categories();
        assert!(cats
            .iter()
            .any(|c| c.slug == "sci-fi" || c.kind == "modpack"));
        assert!(cats.len() >= 5);
    }
}
