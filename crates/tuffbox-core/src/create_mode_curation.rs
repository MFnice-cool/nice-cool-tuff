//! Iterative Create Mode curation: gameplay pillars, co-occurrence priors,
//! Reviewer verdict validation, and launcher-owned scoring.
//!
//! AI never picks jar URLs / file_id / checksums — only search intent and keep/reject slugs.

use crate::create_mode::{CreateModeBrief, PackDraft, PackDraftMod};
use crate::swarm_supabase::PartnerStat;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

// ─── Roles & pillars ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CandidateRole {
    Gameplay,
    Support,
    Performance,
    Library,
}

impl CandidateRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Gameplay => "gameplay",
            Self::Support => "support",
            Self::Performance => "performance",
            Self::Library => "library",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GameplayPillar {
    pub id: String,
    pub label: String,
    pub keywords: Vec<String>,
    /// 1 = must cover before is_complete; 2 = nice-to-have.
    pub priority: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PillarStatus {
    pub id: String,
    pub label: String,
    pub priority: u8,
    pub covered: bool,
    #[serde(default)]
    pub evidence_slugs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CooccurPartner {
    pub slug: String,
    pub pack_count: u64,
    /// `launcher` | `mpi` | `local` | `dep`
    pub graph: String,
    pub role: CandidateRole,
    #[serde(default)]
    pub covers_pillars: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CooccurPrior {
    pub seed_mod: String,
    pub partners: Vec<CooccurPartner>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CompactCandidateCard {
    pub slug: String,
    pub name: String,
    pub summary: String,
    #[serde(default)]
    pub categories: Vec<String>,
    pub role: CandidateRole,
    #[serde(default)]
    pub covers_pillars: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cooccur_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cooccur_graph: Option<String>,
    #[serde(default)]
    pub deps: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CurationVerdict {
    #[serde(default)]
    pub is_complete: bool,
    #[serde(default)]
    pub coverage_score: f32,
    #[serde(default)]
    pub missing_aspects: Vec<String>,
    #[serde(default)]
    pub rejected_mod_ids: Vec<String>,
    #[serde(default)]
    pub keep_mod_ids: Vec<String>,
    #[serde(default)]
    pub next_search_keywords: Vec<String>,
    #[serde(default)]
    pub human_note: String,
    #[serde(default)]
    pub pillar_status: Vec<PillarStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CurationSnapshot {
    pub iteration: u32,
    pub coverage_score: f32,
    pub launcher_score: f32,
    pub draft: PackDraft,
    #[serde(default)]
    pub pillar_status: Vec<PillarStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct CurationMemory {
    #[serde(default)]
    pub searched_keywords: Vec<String>,
    #[serde(default)]
    pub blacklisted_mod_ids: Vec<String>,
    #[serde(default)]
    pub keep_mod_ids: Vec<String>,
    #[serde(default)]
    pub missing_aspects: Vec<String>,
    #[serde(default)]
    pub best: Option<CurationSnapshot>,
    #[serde(default)]
    pub verdicts: Vec<CurationVerdict>,
    #[serde(default)]
    pub last_keep_fingerprint: String,
    #[serde(default)]
    pub stuck_streak: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CurationStopReason {
    Complete,
    MaxIterations,
    Stuck,
    PillarsUnmet,
    Timeout,
    Cancelled,
    EmptyPool,
    AiDown,
}

impl CurationStopReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::MaxIterations => "max_iterations",
            Self::Stuck => "stuck",
            Self::PillarsUnmet => "pillars_unmet",
            Self::Timeout => "timeout",
            Self::Cancelled => "cancelled",
            Self::EmptyPool => "empty_pool",
            Self::AiDown => "ai_down",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CurationSearchQuery {
    pub keywords: Vec<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub reason: String,
}

pub const CURATION_REVIEWER_PROMPT: &str = r#"You are TuffBox Create Mode Reviewer.
Score whether the candidate pack covers the user's gameplay pillars — NOT QoL/FPS alone.

You receive: user goal, pillar checklist, compact mod cards (with roles + co-occurrence hints), optional graph hints.
Prefer gameplay mods that cover unmet pillars. Performance/library/support only as filler after pillars.

Return ONE JSON object:
{
  "is_complete": boolean,
  "coverage_score": 0.0-1.0,
  "missing_aspects": ["pillar labels still missing"],
  "rejected_mod_ids": ["slug"],
  "keep_mod_ids": ["slug"],
  "next_search_keywords": ["english tokens for unmet pillars"],
  "human_note": "1-2 sentences in the user's language",
  "pillar_status": [{"id":"...","label":"...","priority":1,"covered":false,"evidence_slugs":[]}]
}

Rules:
- keep_mod_ids / rejected_mod_ids MUST be slugs from the candidate cards only.
- next_search_keywords target unmet gameplay pillars — never "fps", "sodium", "optimization" alone.
- Do NOT invent jar URLs, file ids, or checksums.
- is_complete=true only if every priority-1 pillar looks covered by keep_mod_ids.
"#;

pub const CURATION_SEARCH_PROMPT: &str = r#"You are TuffBox Create Mode Search.
Propose 1–6 English Modrinth search keywords that cover unmet gameplay pillars.
Never propose QoL/FPS-only keywords when pillars remain unmet.

Return ONE JSON object:
{ "keywords": ["aircraft","aeronautics"], "category": "transportation"|null, "reason": "cover <pillar>" }
"#;

// ─── Role classification ─────────────────────────────────────────────────────

const PERF_SLUGS: &[&str] = &[
    "sodium",
    "lithium",
    "starlight",
    "ferritecore",
    "entityculling",
    "iris",
    "immediatelyfast",
    "modernfix",
    "rubidium",
    "embeddium",
    "optifine",
    "dynamicefps",
    "krypton",
];

const SUPPORT_SLUGS: &[&str] = &[
    "appleskin",
    "modmenu",
    "jade",
    "wthit",
    "xaeros-minimap",
    "xaeros-world-map",
    "journeymap",
    "voxelmap",
    "inventory-hud",
    "mouse-tweaks",
    "controlling",
    "reeses-sodium-options",
    "sodium-extra",
];

const GAMEPLAY_CATS: &[&str] = &[
    "technology",
    "magic",
    "adventure",
    "worldgen",
    "transportation",
    "food",
    "equipment",
    "mobs",
    "storage",
];

pub fn classify_role(slug: &str, name: &str, categories: &[String]) -> CandidateRole {
    let s = slug.trim().to_ascii_lowercase();
    let n = name.trim().to_ascii_lowercase();
    let cats: Vec<String> = categories.iter().map(|c| c.to_ascii_lowercase()).collect();

    if cats.iter().any(|c| c == "library")
        || s.ends_with("-api")
        || s.ends_with("-lib")
        || n.ends_with(" api")
        || (n.contains(" library") && !n.contains("librarian"))
        || s == "fabric-api"
        || s == "cloth-config"
        || s.contains("cloth-config")
    {
        return CandidateRole::Library;
    }
    if PERF_SLUGS
        .iter()
        .any(|p| s == *p || s.starts_with(&format!("{p}-")))
        || cats
            .iter()
            .any(|c| c == "optimization" || c == "performance")
        || n.contains("fps")
        || n.contains("optimization")
    {
        return CandidateRole::Performance;
    }
    if SUPPORT_SLUGS.iter().any(|p| s == *p)
        || cats
            .iter()
            .any(|c| matches!(c.as_str(), "utility" | "management" | "information" | "hud"))
            && !cats.iter().any(|c| GAMEPLAY_CATS.contains(&c.as_str()))
    {
        return CandidateRole::Support;
    }
    if cats.iter().any(|c| GAMEPLAY_CATS.contains(&c.as_str())) {
        return CandidateRole::Gameplay;
    }
    // Default: treat unknown content mods as gameplay (better than QoL bias).
    CandidateRole::Gameplay
}

// ─── Pillar extraction ───────────────────────────────────────────────────────

struct PillarTemplate {
    id: &'static str,
    label: &'static str,
    keywords: &'static [&'static str],
    aliases: &'static [&'static str],
    priority: u8,
}

const PILLAR_TEMPLATES: &[PillarTemplate] = &[
    PillarTemplate {
        id: "create_automation",
        label: "Create / automation",
        keywords: &[
            "create",
            "create-steam-n-rails",
            "create-enchantment-industry",
            "mechanisms",
        ],
        aliases: &[
            "create",
            "automation",
            "factory",
            "industrial",
            "contraption",
        ],
        priority: 1,
    },
    PillarTemplate {
        id: "flight",
        label: "Flight / aircraft",
        keywords: &[
            "aircraft",
            "aeronautics",
            "plane",
            "immersive-aircraft",
            "create-aeronautics",
        ],
        aliases: &[
            "flight",
            "airplane",
            "aircraft",
            "plane",
            "flying",
            "самолёт",
            "самолет",
            "авиа",
        ],
        priority: 1,
    },
    PillarTemplate {
        id: "tech_power",
        label: "Power / tech progression",
        keywords: &["mekanism", "powah", "energy", "tech", "rftools"],
        aliases: &[
            "mekanism",
            "tech",
            "technology",
            "power",
            "energy",
            "industrial",
        ],
        priority: 1,
    },
    PillarTemplate {
        id: "magic",
        label: "Magic progression",
        keywords: &[
            "botania",
            "ars-nouveau",
            "irons-spells-n-spellbooks",
            "magic",
        ],
        aliases: &["magic", "magical", "wizard", "spell", "botania", "магия"],
        priority: 1,
    },
    PillarTemplate {
        id: "space",
        label: "Space / sci-fi",
        keywords: &["ad-astra", "space", "galaxy"],
        aliases: &["space", "sci-fi", "scifi", "astronaut", "космос"],
        priority: 1,
    },
    PillarTemplate {
        id: "adventure_rpg",
        label: "Adventure / RPG",
        keywords: &["origins", "simply-swords", "rpg", "dungeons"],
        aliases: &["adventure", "rpg", "dungeon", "quest"],
        priority: 1,
    },
    PillarTemplate {
        id: "farming_food",
        label: "Farming / food",
        keywords: &["farmers-delight", "farming", "crops"],
        aliases: &["farm", "farming", "food", "delight", "кухня"],
        priority: 2,
    },
    PillarTemplate {
        id: "worldgen",
        label: "Worldgen / biomes",
        keywords: &["biomes", "terrablender", "worldgen", "structures"],
        aliases: &["biome", "worldgen", "exploration", "structure"],
        priority: 2,
    },
    PillarTemplate {
        id: "storage_logistics",
        label: "Storage / logistics",
        keywords: &[
            "ae2",
            "applied-energistics-2",
            "refined-storage",
            "storage-drawers",
        ],
        aliases: &["storage", "ae2", "logistics", "warehouse"],
        priority: 2,
    },
];

/// Never promote these as gameplay pillars.
fn is_utility_noise(token: &str) -> bool {
    matches!(
        token,
        "sodium"
            | "iris"
            | "lithium"
            | "fps"
            | "performance"
            | "optimization"
            | "optifine"
            | "modmenu"
            | "appleskin"
            | "qol"
            | "utility"
            | "library"
            | "api"
            | "fabric-api"
            | "cloth-config"
    )
}

/// Extract 3–7 gameplay pillars from brief + free-text goal. Deterministic, no LLM.
pub fn extract_pillars_from_brief(brief: &CreateModeBrief, user_goal: &str) -> Vec<GameplayPillar> {
    let mut hay = String::new();
    hay.push_str(user_goal);
    hay.push(' ');
    hay.push_str(&brief.title);
    for m in &brief.must_have {
        hay.push(' ');
        hay.push_str(&m.query);
        if let Some(s) = &m.slug_hint {
            hay.push(' ');
            hay.push_str(s);
        }
    }
    for c in &brief.categories {
        hay.push(' ');
        hay.push_str(&c.id);
        hay.push(' ');
        hay.push_str(&c.query);
    }
    let lower = hay.to_ascii_lowercase();

    let mut scored: Vec<(i32, GameplayPillar)> = Vec::new();
    for t in PILLAR_TEMPLATES {
        let mut hits = 0i32;
        for a in t.aliases {
            if is_utility_noise(a) {
                continue;
            }
            if lower.contains(a) {
                hits += 2;
            }
        }
        for k in t.keywords {
            if lower.contains(&k.replace('-', " ")) || lower.contains(k) {
                hits += 3;
            }
        }
        if hits > 0 {
            scored.push((
                hits * 10 - t.priority as i32,
                GameplayPillar {
                    id: t.id.into(),
                    label: t.label.into(),
                    keywords: t.keywords.iter().map(|s| (*s).to_string()).collect(),
                    priority: t.priority,
                },
            ));
        }
    }

    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.id.cmp(&b.1.id)));
    let mut out: Vec<GameplayPillar> = scored.into_iter().map(|(_, p)| p).take(7).collect();

    // Fallback: invent pillars from mustHave / category ids that look like gameplay.
    if out.is_empty() {
        for m in brief.must_have.iter().take(3) {
            let q = m.query.trim();
            if q.is_empty() || is_utility_noise(&q.to_ascii_lowercase()) {
                continue;
            }
            let id = slugify_id(q);
            out.push(GameplayPillar {
                id: id.clone(),
                label: q.to_string(),
                keywords: vec![q.to_ascii_lowercase()],
                priority: 1,
            });
        }
        for c in brief
            .categories
            .iter()
            .filter(|c| GAMEPLAY_CATS.contains(&c.id.to_ascii_lowercase().as_str()))
        {
            if out.len() >= 5 {
                break;
            }
            let id = format!("cat_{}", c.id.to_ascii_lowercase());
            if out.iter().any(|p| p.id == id) {
                continue;
            }
            out.push(GameplayPillar {
                id,
                label: if c.reason.trim().is_empty() {
                    c.id.clone()
                } else {
                    c.reason.clone()
                },
                keywords: vec![c.query.to_ascii_lowercase(), c.id.to_ascii_lowercase()],
                priority: 1,
            });
        }
    }

    // Ensure at least one priority-1 if we only got priority-2.
    if !out.is_empty() && out.iter().all(|p| p.priority != 1) {
        out[0].priority = 1;
    }
    out
}

fn slugify_id(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .chars()
        .take(32)
        .collect()
}

// ─── Coverage ────────────────────────────────────────────────────────────────

pub fn mod_covers_pillar(m: &PackDraftMod, pillar: &GameplayPillar) -> bool {
    let blob = format!(
        "{} {} {} {}",
        m.slug.to_ascii_lowercase(),
        m.name.to_ascii_lowercase(),
        m.reason.to_ascii_lowercase(),
        m.category.to_ascii_lowercase()
    );
    pillar.keywords.iter().any(|k| {
        let k = k.to_ascii_lowercase();
        !k.is_empty() && (blob.contains(&k) || blob.contains(&k.replace('-', " ")))
    })
}

pub fn compute_pillar_status(
    pillars: &[GameplayPillar],
    keep_mods: &[PackDraftMod],
) -> Vec<PillarStatus> {
    pillars
        .iter()
        .map(|p| {
            let evidence: Vec<String> = keep_mods
                .iter()
                .filter(|m| mod_covers_pillar(m, p))
                .map(|m| m.slug.clone())
                .collect();
            PillarStatus {
                id: p.id.clone(),
                label: p.label.clone(),
                priority: p.priority,
                covered: !evidence.is_empty(),
                evidence_slugs: evidence,
            }
        })
        .collect()
}

pub fn unmet_pillars<'a>(
    pillars: &'a [GameplayPillar],
    status: &[PillarStatus],
    priority_only: Option<u8>,
) -> Vec<&'a GameplayPillar> {
    pillars
        .iter()
        .filter(|p| {
            if let Some(pr) = priority_only {
                if p.priority != pr {
                    return false;
                }
            }
            !status
                .iter()
                .find(|s| s.id == p.id)
                .map(|s| s.covered)
                .unwrap_or(false)
        })
        .collect()
}

pub fn priority1_unmet(status: &[PillarStatus]) -> bool {
    status.iter().any(|s| s.priority == 1 && !s.covered)
}

// ─── Compact cards & cooccur ─────────────────────────────────────────────────

pub fn strip_summary(raw: &str, max_chars: usize) -> String {
    let mut s = raw.replace(['\r', '\n'], " ");
    // crude HTML/MD strip
    while let Some(start) = s.find('<') {
        if let Some(end) = s[start..].find('>') {
            s.replace_range(start..start + end + 1, " ");
        } else {
            break;
        }
    }
    for marker in ["**", "__", "`"] {
        s = s.replace(marker, "");
    }
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= max_chars {
        return s;
    }
    let truncated: String = s.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{truncated}…")
}

pub fn compact_candidate_card(
    m: &PackDraftMod,
    pillars: &[GameplayPillar],
    cooccur: Option<&CooccurPartner>,
) -> CompactCandidateCard {
    let cats = if m.category.trim().is_empty() {
        vec![]
    } else {
        vec![m.category.clone()]
    };
    let role = classify_role(&m.slug, &m.name, &cats);
    let covers: Vec<String> = pillars
        .iter()
        .filter(|p| mod_covers_pillar(m, p))
        .map(|p| p.id.clone())
        .collect();
    CompactCandidateCard {
        slug: m.slug.clone(),
        name: m.name.clone(),
        summary: strip_summary(&m.reason, 150),
        categories: cats,
        role: cooccur.map(|c| c.role).unwrap_or(role),
        covers_pillars: covers,
        cooccur_count: cooccur.map(|c| c.pack_count),
        cooccur_graph: cooccur.map(|c| c.graph.clone()),
        deps: vec![],
    }
}

pub fn compact_draft_cards(
    draft: &PackDraft,
    pillars: &[GameplayPillar],
    partners_by_slug: &HashMap<String, CooccurPartner>,
    max_cards: usize,
) -> Vec<CompactCandidateCard> {
    let mut cards: Vec<CompactCandidateCard> = draft
        .mods
        .iter()
        .map(|m| {
            let key = m.slug.to_ascii_lowercase();
            compact_candidate_card(m, pillars, partners_by_slug.get(&key))
        })
        .collect();
    // Gameplay / pillar evidence first, then by cooccur, then downloads.
    cards.sort_by(|a, b| {
        let ag = (
            !a.covers_pillars.is_empty(),
            a.role == CandidateRole::Gameplay,
        );
        let bg = (
            !b.covers_pillars.is_empty(),
            b.role == CandidateRole::Gameplay,
        );
        bg.cmp(&ag)
            .then_with(|| {
                b.cooccur_count
                    .unwrap_or(0)
                    .cmp(&a.cooccur_count.unwrap_or(0))
            })
            .then_with(|| a.slug.cmp(&b.slug))
    });
    cards.truncate(max_cards.max(1));
    cards
}

/// Build co-occurrence partners from PartnerStat batches; tag roles + pillar overlap.
pub fn build_cooccur_partners(
    seed: &str,
    partners: &[PartnerStat],
    graph: &str,
    pillars: &[GameplayPillar],
    limit: usize,
) -> CooccurPrior {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for p in partners {
        let slug = p.partner.trim().to_ascii_lowercase();
        if slug.is_empty() || !seen.insert(slug.clone()) {
            continue;
        }
        let role = classify_role(&slug, &slug, &[]);
        let covers: Vec<String> = pillars
            .iter()
            .filter(|pil| {
                pil.keywords.iter().any(|k| {
                    let k = k.to_ascii_lowercase();
                    slug.contains(&k) || k.contains(&slug)
                })
            })
            .map(|pil| pil.id.clone())
            .collect();
        out.push(CooccurPartner {
            slug,
            pack_count: p.pack_count.max(1),
            graph: graph.into(),
            role,
            covers_pillars: covers,
        });
        if out.len() >= limit {
            break;
        }
    }
    CooccurPrior {
        seed_mod: seed.trim().to_ascii_lowercase(),
        partners: out,
    }
}

/// Prefer gameplay / pillar-covering partners; demote pure QoL when pillars unmet.
pub fn filter_partners_for_pillars(
    priors: &[CooccurPrior],
    pillars_unmet: bool,
    limit: usize,
) -> Vec<CooccurPartner> {
    let mut all: Vec<CooccurPartner> = priors.iter().flat_map(|p| p.partners.clone()).collect();
    let mut seen = HashSet::new();
    all.retain(|p| seen.insert(p.slug.to_ascii_lowercase()));

    all.sort_by(|a, b| {
        let ascore = partner_priority_score(a, pillars_unmet);
        let bscore = partner_priority_score(b, pillars_unmet);
        bscore
            .cmp(&ascore)
            .then_with(|| b.pack_count.cmp(&a.pack_count))
            .then_with(|| a.slug.cmp(&b.slug))
    });
    all.truncate(limit);
    all
}

fn partner_priority_score(p: &CooccurPartner, pillars_unmet: bool) -> i64 {
    let mut s = 0i64;
    if !p.covers_pillars.is_empty() {
        s += 1000;
    }
    match p.role {
        CandidateRole::Gameplay => s += 200,
        CandidateRole::Support => s += if pillars_unmet { -50 } else { 40 },
        CandidateRole::Performance => s += if pillars_unmet { -200 } else { 10 },
        CandidateRole::Library => s += if pillars_unmet { -150 } else { 5 },
    }
    s + (p.pack_count as i64).min(500) / 10
}

pub fn format_cooccur_block(partners: &[CooccurPartner], limit: usize) -> String {
    let mut out = String::from("## Frequent companions (launcher co-occurrence)\n");
    if partners.is_empty() {
        out.push_str("- (none available — rely on pillars + catalog search)\n");
        return out;
    }
    for (i, p) in partners.iter().take(limit).enumerate() {
        out.push_str(&format!(
            "{}. `{}` (count {}, graph {}, role {}){}\n",
            i + 1,
            p.slug,
            p.pack_count,
            p.graph,
            p.role.as_str(),
            if p.covers_pillars.is_empty() {
                String::new()
            } else {
                format!(" covers={}", p.covers_pillars.join(","))
            }
        ));
    }
    out.push_str(
        "Prefer companions that cover unmet pillars. Do not substitute Sodium/Iris for gameplay.\n",
    );
    out
}

pub fn format_pillars_block(status: &[PillarStatus]) -> String {
    let mut out = String::from("## Gameplay pillars (launcher truth)\n");
    for s in status {
        out.push_str(&format!(
            "- [{}] {} (priority {}) {}\n",
            if s.covered { "x" } else { " " },
            s.label,
            s.priority,
            if s.evidence_slugs.is_empty() {
                String::new()
            } else {
                format!("← {}", s.evidence_slugs.join(", "))
            }
        ));
    }
    out
}

// ─── Verdict validation ──────────────────────────────────────────────────────

pub fn normalize_keyword(k: &str) -> Option<String> {
    let t = k.trim().to_ascii_lowercase();
    if t.is_empty() || t.len() > 48 {
        return None;
    }
    if is_utility_noise(&t) {
        return None;
    }
    Some(t)
}

pub fn subtract_searched(keywords: &[String], searched: &[String]) -> Vec<String> {
    let seen: HashSet<String> = searched.iter().map(|s| s.to_ascii_lowercase()).collect();
    let mut out = Vec::new();
    let mut local = HashSet::new();
    for k in keywords {
        if let Some(n) = normalize_keyword(k) {
            if seen.contains(&n) || !local.insert(n.clone()) {
                continue;
            }
            // Near-dup: share prefix ≥4 with searched
            if seen.iter().any(|s| near_dup(s, &n)) {
                continue;
            }
            out.push(n);
        }
    }
    out
}

fn near_dup(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let min = a.len().min(b.len());
    if min < 4 {
        return false;
    }
    a.starts_with(&b[..min.min(b.len())]) || b.starts_with(&a[..min.min(a.len())])
}

/// Intersect keep/reject with known pool; clamp score; sync pillars from launcher.
pub fn validate_and_sync_verdict(
    mut verdict: CurationVerdict,
    known_slugs: &HashSet<String>,
    pillars: &[GameplayPillar],
    keep_mods: &[PackDraftMod],
    searched: &[String],
    min_keep: usize,
) -> CurationVerdict {
    let known_l: HashSet<String> = known_slugs.iter().map(|s| s.to_ascii_lowercase()).collect();
    verdict.keep_mod_ids = verdict
        .keep_mod_ids
        .into_iter()
        .filter_map(|s| {
            let l = s.trim().to_ascii_lowercase();
            if known_l.contains(&l) {
                Some(l)
            } else {
                None
            }
        })
        .collect();
    verdict.rejected_mod_ids = verdict
        .rejected_mod_ids
        .into_iter()
        .filter_map(|s| {
            let l = s.trim().to_ascii_lowercase();
            if known_l.contains(&l) {
                Some(l)
            } else {
                None
            }
        })
        .collect();
    // Keep wins over reject if both listed.
    let keep_set: HashSet<_> = verdict.keep_mod_ids.iter().cloned().collect();
    verdict.rejected_mod_ids.retain(|r| !keep_set.contains(r));

    verdict.coverage_score = verdict.coverage_score.clamp(0.0, 1.0);
    verdict.next_search_keywords = subtract_searched(&verdict.next_search_keywords, searched);

    let status = compute_pillar_status(pillars, keep_mods);
    verdict.pillar_status = status.clone();
    verdict.missing_aspects = status
        .iter()
        .filter(|s| !s.covered)
        .map(|s| s.label.clone())
        .collect();

    let p1_ok = !priority1_unmet(&status);
    if !p1_ok || verdict.keep_mod_ids.len() < min_keep {
        verdict.is_complete = false;
    }
    if p1_ok && verdict.keep_mod_ids.len() >= min_keep && verdict.missing_aspects.is_empty() {
        // Launcher may accept complete only when priority-1 covered.
        // Leave LLM is_complete if already true; else allow true.
        if verdict.coverage_score >= 0.75 {
            verdict.is_complete = true;
        }
    } else {
        verdict.is_complete = false;
    }

    // Force next keywords from unmet pillars when LLM gave none.
    if verdict.next_search_keywords.is_empty() {
        for p in unmet_pillars(pillars, &status, Some(1)) {
            for k in &p.keywords {
                if let Some(n) = normalize_keyword(k) {
                    if !searched.iter().any(|s| s == &n)
                        && !verdict.next_search_keywords.contains(&n)
                    {
                        verdict.next_search_keywords.push(n);
                    }
                }
                if verdict.next_search_keywords.len() >= 6 {
                    break;
                }
            }
        }
    }

    verdict
}

pub fn keywords_for_unmet_pillars(
    pillars: &[GameplayPillar],
    status: &[PillarStatus],
    searched: &[String],
    limit: usize,
) -> Vec<String> {
    let mut out = Vec::new();
    for p in unmet_pillars(pillars, status, Some(1)) {
        for k in &p.keywords {
            if let Some(n) = normalize_keyword(k) {
                if !searched.iter().any(|s| near_dup(s, &n)) && !out.contains(&n) {
                    out.push(n);
                }
            }
            if out.len() >= limit {
                return out;
            }
        }
    }
    for p in unmet_pillars(pillars, status, None) {
        for k in &p.keywords {
            if let Some(n) = normalize_keyword(k) {
                if !searched.iter().any(|s| near_dup(s, &n)) && !out.contains(&n) {
                    out.push(n);
                }
            }
            if out.len() >= limit {
                return out;
            }
        }
    }
    out
}

// ─── Scoring, caps, stuck ────────────────────────────────────────────────────

pub fn role_caps(target_count: u32) -> (usize, usize) {
    let t = target_count.max(1) as usize;
    let perf_lib = ((t as f32) * 0.15).ceil() as usize;
    let support = ((t as f32) * 0.20).ceil() as usize;
    (perf_lib.max(2), support.max(3))
}

pub fn apply_role_caps(mods: &[PackDraftMod], target_count: u32) -> Vec<PackDraftMod> {
    let (perf_lib_cap, support_cap) = role_caps(target_count);
    let target = target_count as usize;
    let mut out = Vec::new();
    let mut perf_lib = 0usize;
    let mut support = 0usize;
    for m in mods {
        if out.len() >= target {
            break;
        }
        let role = classify_role(&m.slug, &m.name, &[m.category.clone()]);
        match role {
            CandidateRole::Performance | CandidateRole::Library => {
                if perf_lib >= perf_lib_cap {
                    continue;
                }
                perf_lib += 1;
            }
            CandidateRole::Support => {
                if support >= support_cap {
                    continue;
                }
                support += 1;
            }
            CandidateRole::Gameplay => {}
        }
        out.push(m.clone());
    }
    out
}

pub fn launcher_score(
    draft: &PackDraft,
    pillars: &[GameplayPillar],
    status: &[PillarStatus],
    partner_slugs: &HashSet<String>,
) -> f32 {
    if pillars.is_empty() {
        return 0.3;
    }
    let mut weight_sum = 0.0f32;
    let mut covered_w = 0.0f32;
    for p in pillars {
        let w = if p.priority == 1 { 2.0 } else { 1.0 };
        weight_sum += w;
        if status.iter().any(|s| s.id == p.id && s.covered) {
            covered_w += w;
        }
    }
    let pillar_frac = if weight_sum > 0.0 {
        covered_w / weight_sum
    } else {
        0.0
    };

    let mut gameplay = 0usize;
    let mut support_perf = 0usize;
    for m in &draft.mods {
        match classify_role(&m.slug, &m.name, &[m.category.clone()]) {
            CandidateRole::Gameplay => gameplay += 1,
            CandidateRole::Support | CandidateRole::Performance => support_perf += 1,
            CandidateRole::Library => support_perf += 1,
        }
    }
    let n = draft.mods.len().max(1);
    let gameplay_share = gameplay as f32 / n as f32;
    let clutter = support_perf as f32 / n as f32;
    let gameplay_term = if clutter > 0.35 {
        (gameplay_share * 0.7).clamp(0.0, 1.0)
    } else {
        gameplay_share.clamp(0.0, 1.0)
    };

    let target = draft.brief.target_count.max(1) as f32;
    let size_term = 1.0 - ((draft.mods.len() as f32 - target).abs() / target).min(1.0);

    let co_hits = draft
        .mods
        .iter()
        .filter(|m| partner_slugs.contains(&m.slug.to_ascii_lowercase()))
        .count();
    let co_term = (co_hits as f32 / n as f32).min(1.0);

    (0.55 * pillar_frac + 0.20 * gameplay_term + 0.10 * co_term + 0.15 * size_term).clamp(0.0, 1.0)
}

pub fn keep_fingerprint(keep: &[String]) -> String {
    let mut v = keep
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .collect::<Vec<_>>();
    v.sort();
    v.join("|")
}

pub fn update_stuck(memory: &mut CurationMemory, keep: &[String], score_delta: f32) -> bool {
    let fp = keep_fingerprint(keep);
    if fp == memory.last_keep_fingerprint && score_delta.abs() < 0.02 {
        memory.stuck_streak += 1;
    } else {
        memory.stuck_streak = 0;
        memory.last_keep_fingerprint = fp;
    }
    memory.stuck_streak >= 2
}

pub fn min_keep_for_complete(target_count: u32) -> usize {
    (((target_count as f32) * 0.25).floor() as usize)
        .min(12)
        .max(4)
}

/// Push keep slugs into mustHave; merge reject into exclude.
pub fn apply_verdict_to_brief(
    brief: &CreateModeBrief,
    verdict: &CurationVerdict,
    partner_boosts: &[CooccurPartner],
) -> CreateModeBrief {
    let mut brief = brief.clone();
    let mut exclude: HashSet<String> = brief
        .exclude
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .collect();
    for r in &verdict.rejected_mod_ids {
        exclude.insert(r.to_ascii_lowercase());
    }
    brief.exclude = exclude.into_iter().collect();
    brief.exclude.sort();

    let mut seen_mh: HashSet<String> = brief
        .must_have
        .iter()
        .filter_map(|m| {
            m.slug_hint
                .as_ref()
                .map(|s| s.to_ascii_lowercase())
                .or_else(|| Some(m.query.to_ascii_lowercase()))
        })
        .collect();

    for k in &verdict.keep_mod_ids {
        let key = k.to_ascii_lowercase();
        if brief.exclude.iter().any(|e| e.eq_ignore_ascii_case(&key)) {
            continue;
        }
        if !seen_mh.insert(key.clone()) {
            continue;
        }
        brief.must_have.push(crate::create_mode::MustHaveSpec {
            query: k.clone(),
            slug_hint: Some(k.clone()),
            reason: "Curation keep".into(),
        });
    }

    for p in partner_boosts
        .iter()
        .filter(|p| p.role == CandidateRole::Gameplay || !p.covers_pillars.is_empty())
    {
        let key = p.slug.to_ascii_lowercase();
        if brief.exclude.iter().any(|e| e.eq_ignore_ascii_case(&key)) {
            continue;
        }
        if !seen_mh.insert(key.clone()) {
            continue;
        }
        brief.must_have.push(crate::create_mode::MustHaveSpec {
            query: p.slug.clone(),
            slug_hint: Some(p.slug.clone()),
            reason: format!("Co-occur prior ({})", p.graph),
        });
        if brief.must_have.len() > 24 {
            break;
        }
    }

    // Inject unmet-pillar keywords into category queries when empty-ish.
    if !verdict.next_search_keywords.is_empty() {
        for (i, kw) in verdict.next_search_keywords.iter().take(3).enumerate() {
            if let Some(cat) = brief.categories.get_mut(i) {
                if !cat.query.to_ascii_lowercase().contains(kw) {
                    cat.query = format!("{} {}", cat.query, kw).trim().to_string();
                }
            }
        }
    }

    brief
}

pub fn known_slugs_from_draft(draft: &PackDraft) -> HashSet<String> {
    draft
        .mods
        .iter()
        .flat_map(|m| {
            [
                m.slug.to_ascii_lowercase(),
                m.project_id.to_ascii_lowercase(),
            ]
        })
        .filter(|s| !s.is_empty())
        .collect()
}

pub fn filter_draft_by_keep_reject(
    draft: &PackDraft,
    keep: &[String],
    reject: &[String],
) -> PackDraft {
    let reject_l: HashSet<_> = reject.iter().map(|s| s.to_ascii_lowercase()).collect();
    let keep_l: HashSet<_> = keep.iter().map(|s| s.to_ascii_lowercase()).collect();
    let mut mods: Vec<PackDraftMod> = draft
        .mods
        .iter()
        .filter(|m| !reject_l.contains(&m.slug.to_ascii_lowercase()))
        .cloned()
        .collect();
    if !keep_l.is_empty() {
        mods.sort_by_key(|m| {
            if keep_l.contains(&m.slug.to_ascii_lowercase()) {
                0
            } else {
                1
            }
        });
    }
    mods = apply_role_caps(&mods, draft.brief.target_count);
    PackDraft {
        brief: draft.brief.clone(),
        mods,
        unresolved: draft.unresolved.clone(),
    }
}

// ─── JSON schemas for Ollama ─────────────────────────────────────────────────

pub fn curation_verdict_json_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "is_complete": { "type": "boolean" },
            "coverage_score": { "type": "number" },
            "missing_aspects": { "type": "array", "items": { "type": "string" } },
            "rejected_mod_ids": { "type": "array", "items": { "type": "string" } },
            "keep_mod_ids": { "type": "array", "items": { "type": "string" } },
            "next_search_keywords": { "type": "array", "items": { "type": "string" } },
            "human_note": { "type": "string" },
            "pillar_status": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "label": { "type": "string" },
                        "priority": { "type": "integer" },
                        "covered": { "type": "boolean" },
                        "evidence_slugs": { "type": "array", "items": { "type": "string" } }
                    },
                    "required": ["id", "label", "priority", "covered"]
                }
            }
        },
        "required": ["is_complete", "coverage_score", "keep_mod_ids", "missing_aspects"]
    })
}

pub fn curation_search_json_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "keywords": { "type": "array", "items": { "type": "string" } },
            "category": { "type": ["string", "null"] },
            "reason": { "type": "string" }
        },
        "required": ["keywords", "reason"]
    })
}

pub fn parse_curation_verdict(raw: &str) -> Result<CurationVerdict, String> {
    let trimmed = raw.trim();
    let trimmed = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed);
    let trimmed = trimmed.strip_suffix("```").unwrap_or(trimmed).trim();
    serde_json::from_str(trimmed).map_err(|e| format!("invalid CurationVerdict: {e}"))
}

pub fn memory_push_verdict(memory: &mut CurationMemory, verdict: CurationVerdict) {
    memory.keep_mod_ids = verdict.keep_mod_ids.clone();
    memory.missing_aspects = verdict.missing_aspects.clone();
    for r in &verdict.rejected_mod_ids {
        let r = r.to_ascii_lowercase();
        if !memory.blacklisted_mod_ids.iter().any(|b| b == &r) {
            memory.blacklisted_mod_ids.push(r);
        }
    }
    for k in &verdict.next_search_keywords {
        let k = k.to_ascii_lowercase();
        if !memory.searched_keywords.iter().any(|s| s == &k) {
            // Searched is updated by caller when keywords are actually used.
        }
    }
    memory.verdicts.push(verdict);
    if memory.verdicts.len() > 5 {
        let skip = memory.verdicts.len() - 5;
        memory.verdicts.drain(0..skip);
    }
}

pub fn maybe_save_best(
    memory: &mut CurationMemory,
    iteration: u32,
    draft: PackDraft,
    llm_score: f32,
    launcher: f32,
    pillar_status: Vec<PillarStatus>,
) {
    let better = memory
        .best
        .as_ref()
        .map(|b| launcher > b.launcher_score + 0.001)
        .unwrap_or(true);
    if better {
        memory.best = Some(CurationSnapshot {
            iteration,
            coverage_score: llm_score,
            launcher_score: launcher,
            draft,
            pillar_status,
        });
    }
}

/// Persisted on CreateChatSession so UI restores pillar checklist after reload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CurationSessionPersist {
    #[serde(default)]
    pub memory: CurationMemory,
    #[serde(default)]
    pub pillar_status: Vec<PillarStatus>,
    #[serde(default)]
    pub partial: bool,
    #[serde(default)]
    pub stop_reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launcher_score: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
}

impl CurationSessionPersist {
    pub fn from_loop_result(
        memory: CurationMemory,
        pillar_status: Vec<PillarStatus>,
        partial: bool,
        stop: CurationStopReason,
        launcher_score: f32,
        tier: CurationTier,
    ) -> Self {
        Self {
            memory,
            pillar_status,
            partial,
            stop_reason: stop.as_str().into(),
            launcher_score: Some(launcher_score),
            tier: Some(
                match tier {
                    CurationTier::Potato => "potato",
                    CurationTier::Normal => "normal",
                    CurationTier::Strong => "strong",
                }
                .into(),
            ),
        }
    }
}

// ─── Graph hints (cheap, launcher-authored) ──────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GraphHint {
    pub code: String,
    pub mod_id: String,
    pub message: String,
}

fn name_tokens(name: &str) -> HashSet<String> {
    name.to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| t.len() >= 4)
        .filter(|t| {
            !matches!(
                *t,
                "minecraft" | "fabric" | "forge" | "neoforge" | "quilt" | "mod" | "addon"
            )
        })
        .map(str::to_string)
        .collect()
}

/// Cheap graph hints: duplicates + category overload + common API deps.
/// No network; no jar install.
pub fn build_graph_hints(
    mods: &[PackDraftMod],
    installed: &HashSet<String>,
    potato: bool,
    max_hints: usize,
) -> Vec<GraphHint> {
    let mut hints = Vec::new();
    let pool: HashSet<String> = mods
        .iter()
        .map(|m| m.slug.to_ascii_lowercase())
        .chain(installed.iter().cloned())
        .collect();

    // Duplicate functionality by shared name tokens.
    for i in 0..mods.len() {
        let a = &mods[i];
        let ta = name_tokens(&a.name);
        if ta.is_empty() {
            continue;
        }
        for b in mods.iter().skip(i + 1) {
            let tb = name_tokens(&b.name);
            let overlap: Vec<_> = ta.intersection(&tb).cloned().collect();
            if overlap.len() >= 2
                || (overlap.len() == 1
                    && a.category.eq_ignore_ascii_case(&b.category)
                    && !a.category.is_empty())
            {
                hints.push(GraphHint {
                    code: "possible_duplicate".into(),
                    mod_id: a.slug.clone(),
                    message: format!(
                        "`{}` may overlap `{}` ({})",
                        a.slug,
                        b.slug,
                        overlap.join("/")
                    ),
                });
            }
        }
    }

    // Category overload.
    let mut by_cat: HashMap<String, usize> = HashMap::new();
    for m in mods {
        let c = m.category.trim().to_ascii_lowercase();
        if c.is_empty() || c == "library" {
            continue;
        }
        *by_cat.entry(c).or_default() += 1;
    }
    for (cat, n) in by_cat {
        if n >= 12 {
            hints.push(GraphHint {
                code: "category_overload".into(),
                mod_id: cat.clone(),
                message: format!("category `{cat}` has {n} mods — consider thinning"),
            });
        }
    }

    // Common API deps (heuristic, no version fetch).
    if !potato {
        let needs_fabric_api = mods.iter().any(|m| {
            let s = m.slug.to_ascii_lowercase();
            !s.is_empty()
                && s != "fabric-api"
                && classify_role(&m.slug, &m.name, &[m.category.clone()]) != CandidateRole::Library
        });
        if needs_fabric_api && !pool.contains("fabric-api") && !pool.iter().any(|s| s == "fabric") {
            hints.push(GraphHint {
                code: "missing_dep".into(),
                mod_id: "fabric-api".into(),
                message: "many Fabric mods need `fabric-api` — not in keep/pool".into(),
            });
        }
        // Create ecosystem often needs JEI/EMI for playability (support, not pillar).
        let has_create = pool
            .iter()
            .any(|s| s == "create" || s.starts_with("create-"));
        let has_jei = pool.iter().any(|s| {
            matches!(
                s.as_str(),
                "jei" | "emi" | "roughly-enough-items" | "just-enough-items"
            )
        });
        if has_create && !has_jei {
            hints.push(GraphHint {
                code: "missing_dep".into(),
                mod_id: "jei".into(),
                message: "Create-heavy packs usually include JEI/EMI".into(),
            });
        }
    }

    hints.truncate(max_hints.max(1).min(20));
    hints
}

pub fn format_graph_hints_block(hints: &[GraphHint]) -> String {
    let mut out = String::from("## Launcher graph hints (not ActionPlan)\n");
    if hints.is_empty() {
        out.push_str("- (none)\n");
        return out;
    }
    for h in hints {
        out.push_str(&format!("- [{}] {}: {}\n", h.code, h.mod_id, h.message));
    }
    out
}

// ─── Search cache + merge keyword hits ───────────────────────────────────────

#[derive(Debug, Default)]
pub struct KeywordSearchCache {
    // key: "loader|mc|keyword" → slugs already fetched (caller stores mods separately)
    keys: HashSet<String>,
}

impl KeywordSearchCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cache_key(loader: &str, mc: &str, keyword: &str) -> String {
        format!(
            "{}|{}|{}",
            loader.trim().to_ascii_lowercase(),
            mc.trim(),
            keyword.trim().to_ascii_lowercase()
        )
    }

    pub fn seen(&self, loader: &str, mc: &str, keyword: &str) -> bool {
        self.keys.contains(&Self::cache_key(loader, mc, keyword))
    }

    pub fn mark(&mut self, loader: &str, mc: &str, keyword: &str) {
        self.keys.insert(Self::cache_key(loader, mc, keyword));
    }
}

pub fn merge_mods_into_draft(
    draft: &PackDraft,
    extra: &[PackDraftMod],
    blacklist: &[String],
) -> PackDraft {
    let bl: HashSet<_> = blacklist.iter().map(|s| s.to_ascii_lowercase()).collect();
    let mut seen: HashSet<String> = draft
        .mods
        .iter()
        .flat_map(|m| {
            [
                m.slug.to_ascii_lowercase(),
                m.project_id.to_ascii_lowercase(),
            ]
        })
        .collect();
    let mut mods = draft.mods.clone();
    for m in extra {
        let slug = m.slug.to_ascii_lowercase();
        if bl.contains(&slug) || !seen.insert(slug) {
            continue;
        }
        if !m.project_id.is_empty() {
            seen.insert(m.project_id.to_ascii_lowercase());
        }
        mods.push(m.clone());
        if mods.len() >= draft.brief.target_count as usize + 16 {
            break;
        }
    }
    mods = apply_role_caps(&mods, draft.brief.target_count);
    PackDraft {
        brief: draft.brief.clone(),
        mods,
        unresolved: draft.unresolved.clone(),
    }
}

pub fn project_info_to_draft_mod(
    id: String,
    slug: String,
    name: String,
    description: String,
    categories: &[String],
    downloads: u64,
    reason: String,
) -> PackDraftMod {
    let provider = if description.starts_with("curseforge:")
        || (!id.is_empty() && id.chars().all(|c| c.is_ascii_digit()))
    {
        "curseforge".into()
    } else {
        "modrinth".into()
    };
    let category = categories
        .iter()
        .find(|c| GAMEPLAY_CATS.contains(&c.to_ascii_lowercase().as_str()))
        .cloned()
        .or_else(|| categories.first().cloned())
        .unwrap_or_default();
    PackDraftMod {
        slug,
        project_id: id,
        name,
        reason: strip_summary(&format!("{reason}: {description}"), 150),
        category,
        downloads,
        provider,
    }
}

// ─── Potato / hardware tiers ─────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurationTier {
    Potato,
    Normal,
    Strong,
}

impl CurationTier {
    pub fn from_potato_flag(potato: bool) -> Self {
        if potato {
            Self::Potato
        } else {
            Self::Normal
        }
    }

    pub fn default_max_iterations(self) -> u32 {
        match self {
            Self::Potato => 3,
            Self::Normal => 5,
            Self::Strong => 8,
        }
    }

    pub fn max_cards(self) -> usize {
        match self {
            Self::Potato => 24,
            Self::Normal | Self::Strong => 40,
        }
    }

    pub fn max_graph_hints(self) -> usize {
        match self {
            Self::Potato => 8,
            Self::Normal => 12,
            Self::Strong => 20,
        }
    }

    pub fn search_role_llm(self) -> bool {
        !matches!(self, Self::Potato)
    }

    pub fn time_budget_secs(self) -> u64 {
        match self {
            Self::Potato => 120,
            Self::Normal => 180,
            Self::Strong => 300,
        }
    }

    pub fn empty_keyword_streak_limit(self) -> u32 {
        match self {
            Self::Potato => 1,
            _ => 2,
        }
    }
}

/// Override QoL-only SearchRole keywords toward unmet pillars.
pub fn sanitize_search_keywords(
    keywords: &[String],
    pillars: &[GameplayPillar],
    status: &[PillarStatus],
    searched: &[String],
) -> Vec<String> {
    let cleaned: Vec<String> = keywords
        .iter()
        .filter_map(|k| normalize_keyword(k))
        .collect();
    let cleaned = subtract_searched(&cleaned, searched);
    if cleaned.is_empty() || cleaned.iter().all(|k| is_utility_noise(k)) || priority1_unmet(status)
    {
        let forced = keywords_for_unmet_pillars(pillars, status, searched, 6);
        if !forced.is_empty() {
            return forced;
        }
    }
    cleaned.into_iter().take(6).collect()
}

pub fn parse_curation_search(raw: &str) -> Result<CurationSearchQuery, String> {
    let trimmed = raw.trim();
    let trimmed = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed);
    let trimmed = trimmed.strip_suffix("```").unwrap_or(trimmed).trim();
    serde_json::from_str(trimmed).map_err(|e| format!("invalid CurationSearchQuery: {e}"))
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::create_mode::{
        CategoryBudget, CreateModeBrief, MustHaveSpec, PackDraft, PackDraftMod,
    };

    fn sample_brief() -> CreateModeBrief {
        CreateModeBrief {
            title: "Industrial flight pack".into(),
            mc_version: "1.20.1".into(),
            loader: "fabric".into(),
            target_count: 60,
            must_have: vec![MustHaveSpec {
                query: "create".into(),
                slug_hint: Some("create".into()),
                reason: "base".into(),
            }],
            categories: vec![CategoryBudget {
                id: "technology".into(),
                query: "create aircraft".into(),
                count: 20,
                reason: "tech".into(),
                facet: Some("technology".into()),
            }],
            exclude: vec![],
        }
    }

    fn mod_(slug: &str, name: &str, cat: &str) -> PackDraftMod {
        PackDraftMod {
            slug: slug.into(),
            project_id: slug.into(),
            name: name.into(),
            reason: name.into(),
            category: cat.into(),
            downloads: 1000,
            provider: "modrinth".into(),
        }
    }

    #[test]
    fn extracts_create_and_flight_pillars() {
        let pillars = extract_pillars_from_brief(&sample_brief(), "нужны самолёты и Create");
        assert!(pillars.iter().any(|p| p.id == "create_automation"));
        assert!(pillars.iter().any(|p| p.id == "flight"));
        assert!(!pillars.iter().any(|p| p.id.contains("sodium")));
    }

    #[test]
    fn classify_sodium_as_performance() {
        assert_eq!(
            classify_role("sodium", "Sodium", &["utility".into()]),
            CandidateRole::Performance
        );
        assert_eq!(
            classify_role("create", "Create", &["technology".into()]),
            CandidateRole::Gameplay
        );
    }

    #[test]
    fn complete_blocked_when_priority1_unmet() {
        let brief = sample_brief();
        let pillars = extract_pillars_from_brief(&brief, "create airplanes");
        let mods = vec![mod_("sodium", "Sodium", "utility")];
        let status = compute_pillar_status(&pillars, &mods);
        assert!(priority1_unmet(&status));
        let known = known_slugs_from_draft(&PackDraft {
            brief: brief.clone(),
            mods: mods.clone(),
            unresolved: vec![],
        });
        let v = validate_and_sync_verdict(
            CurationVerdict {
                is_complete: true,
                coverage_score: 0.99,
                missing_aspects: vec![],
                rejected_mod_ids: vec![],
                keep_mod_ids: vec!["sodium".into()],
                next_search_keywords: vec!["fps".into()],
                human_note: "ok".into(),
                pillar_status: vec![],
            },
            &known,
            &pillars,
            &mods,
            &[],
            4,
        );
        assert!(!v.is_complete);
        assert!(priority1_unmet(&v.pillar_status));
        assert!(!v.next_search_keywords.iter().any(|k| k == "fps"));
    }

    #[test]
    fn strip_summary_truncates_and_strips_html() {
        let s = strip_summary("<b>Hello</b> world ".repeat(20).as_str(), 40);
        assert!(!s.contains('<'));
        assert!(s.chars().count() <= 40);
    }

    #[test]
    fn partner_filter_prefers_gameplay_when_pillars_unmet() {
        let priors = vec![CooccurPrior {
            seed_mod: "create".into(),
            partners: vec![
                CooccurPartner {
                    slug: "sodium".into(),
                    pack_count: 99999,
                    graph: "mpi".into(),
                    role: CandidateRole::Performance,
                    covers_pillars: vec![],
                },
                CooccurPartner {
                    slug: "create-aeronautics".into(),
                    pack_count: 100,
                    graph: "launcher".into(),
                    role: CandidateRole::Gameplay,
                    covers_pillars: vec!["flight".into()],
                },
            ],
        }];
        let filtered = filter_partners_for_pillars(&priors, true, 2);
        assert_eq!(filtered[0].slug, "create-aeronautics");
    }

    #[test]
    fn role_caps_drop_extra_perf() {
        let mods: Vec<_> = (0..10)
            .map(|i| mod_(&format!("sodium-{i}"), "Sodium", "utility"))
            .chain((0..5).map(|i| mod_(&format!("create-{i}"), "Create", "technology")))
            .collect();
        let capped = apply_role_caps(&mods, 20);
        let perf = capped
            .iter()
            .filter(|m| {
                classify_role(&m.slug, &m.name, &[m.category.clone()]) == CandidateRole::Performance
            })
            .count();
        assert!(perf <= role_caps(20).0);
    }

    #[test]
    fn launcher_score_rewards_pillar_coverage() {
        let brief = sample_brief();
        let pillars = extract_pillars_from_brief(&brief, "create airplanes");
        let weak = PackDraft {
            brief: brief.clone(),
            mods: vec![mod_("sodium", "Sodium", "utility")],
            unresolved: vec![],
        };
        let strong = PackDraft {
            brief: brief.clone(),
            mods: vec![
                mod_("create", "Create", "technology"),
                mod_("immersive-aircraft", "Immersive Aircraft", "transportation"),
            ],
            unresolved: vec![],
        };
        let sw = compute_pillar_status(&pillars, &weak.mods);
        let ss = compute_pillar_status(&pillars, &strong.mods);
        let empty = HashSet::new();
        assert!(
            launcher_score(&strong, &pillars, &ss, &empty)
                > launcher_score(&weak, &pillars, &sw, &empty)
        );
    }

    #[test]
    fn stuck_detector_fires() {
        let mut mem = CurationMemory::default();
        assert!(!update_stuck(&mut mem, &["a".into()], 0.0));
        assert!(!update_stuck(&mut mem, &["a".into()], 0.01));
        assert!(update_stuck(&mut mem, &["a".into()], 0.0));
    }

    #[test]
    fn subtract_drops_searched_and_noise() {
        let next = subtract_searched(
            &[
                "Aircraft".into(),
                "sodium".into(),
                "aircraft".into(),
                "create".into(),
            ],
            &["create".into()],
        );
        assert_eq!(next, vec!["aircraft".to_string()]);
    }

    #[test]
    fn graph_hints_flag_duplicates() {
        let mods = vec![
            mod_("xaeros-minimap", "Xaero's Minimap", "utility"),
            mod_("xaeros-world-map", "Xaero's World Map", "utility"),
        ];
        let hints = build_graph_hints(&mods, &HashSet::new(), false, 20);
        assert!(hints.iter().any(|h| h.code == "possible_duplicate"));
    }

    #[test]
    fn sanitize_overrides_fps_when_pillars_unmet() {
        let brief = sample_brief();
        let pillars = extract_pillars_from_brief(&brief, "create airplanes");
        let status = compute_pillar_status(&pillars, &[mod_("sodium", "Sodium", "utility")]);
        let kw = sanitize_search_keywords(&["fps".into(), "sodium".into()], &pillars, &status, &[]);
        assert!(!kw.is_empty());
        assert!(!kw.iter().any(|k| k == "fps" || k == "sodium"));
    }

    #[test]
    fn merge_mods_respects_blacklist() {
        let brief = sample_brief();
        let draft = PackDraft {
            brief: brief.clone(),
            mods: vec![mod_("create", "Create", "technology")],
            unresolved: vec![],
        };
        let extra = vec![
            mod_("sodium", "Sodium", "utility"),
            mod_("immersive-aircraft", "Aircraft", "transportation"),
        ];
        let merged = merge_mods_into_draft(&draft, &extra, &["sodium".into()]);
        assert!(merged.mods.iter().any(|m| m.slug == "immersive-aircraft"));
        assert!(!merged.mods.iter().any(|m| m.slug == "sodium"));
    }

    #[test]
    fn search_cache_marks_keys() {
        let mut c = KeywordSearchCache::new();
        assert!(!c.seen("fabric", "1.20.1", "aircraft"));
        c.mark("fabric", "1.20.1", "Aircraft");
        assert!(c.seen("fabric", "1.20.1", "aircraft"));
    }
}
