//! Create Mode intent pipeline: search JSON → seed mods → co-occur partners → descriptions.

use crate::modpack_index::MpiSearchQuery;
use crate::provider::{ContentProvider, ModrinthProvider, ProjectInfo, ProviderSearchQuery};
use crate::swarm::ModPairStat;
use crate::swarm_supabase::PartnerStat;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateAddon {
    pub slug: String,
    pub name: String,
    #[serde(default)]
    pub summary: String,
    pub score: u64,
    /// `seed` | `partner` | `keyword`
    pub source: String,
}

/// Theme → Modrinth search tokens for seed discovery.
pub fn theme_seed_queries(theme: &str) -> Vec<&'static str> {
    let t = theme.trim().to_ascii_lowercase().replace('_', "-");
    match t.as_str() {
        "industrial" | "industry" | "tech" | "technology" | "create" | "factory" | "automation" => {
            vec!["create", "mekanism"]
        }
        "magic" | "magical" => vec!["botania", "ars nouveau", "iron's spells"],
        "sci-fi" | "scifi" => vec!["ad astra", "create"],
        "adventure" | "adventure-and-rpg" | "rpg" => vec!["origins", "simply swords"],
        "quests" => vec!["ftb quests", "heracles"],
        "skyblock" => vec!["skyblock builder"],
        _ => vec![],
    }
}

fn search_top(
    provider: &ModrinthProvider,
    query: &str,
    mc: &str,
    loader: &str,
) -> Option<ProjectInfo> {
    let q = ProviderSearchQuery {
        query: Some(query.to_string()),
        minecraft_version: if mc.is_empty() {
            None
        } else {
            Some(mc.to_string())
        },
        loader: if loader.is_empty() {
            None
        } else {
            Some(loader.to_string())
        },
        sort: Some("downloads".into()),
        limit: Some(5),
        project_type: Some("mod".into()),
        ..Default::default()
    };
    let page = provider.search(&q).ok()?;
    page.results.into_iter().next()
}

fn project_to_candidate(p: &ProjectInfo, score: u64, source: &str) -> CandidateAddon {
    CandidateAddon {
        slug: if p.slug.is_empty() {
            p.id.clone()
        } else {
            p.slug.clone()
        },
        name: p.name.clone(),
        summary: p.description.clone(),
        score,
        source: source.into(),
    }
}

/// Resolve seed mods from theme + keywords via Modrinth.
pub fn resolve_seed_mods(
    search: &MpiSearchQuery,
    mc_fallback: &str,
    loader_fallback: &str,
) -> Vec<CandidateAddon> {
    let provider = ModrinthProvider::new();
    let mc = search
        .version
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(mc_fallback);
    let loader = search
        .loader
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(loader_fallback);
    let mut out = Vec::new();
    let mut seen = HashSet::new();

    let mut queries: Vec<String> = Vec::new();
    if let Some(theme) = search.theme.as_deref().filter(|s| !s.trim().is_empty()) {
        for q in theme_seed_queries(theme) {
            queries.push(q.to_string());
        }
        if queries.is_empty() {
            queries.push(theme.to_string());
        }
    }
    for kw in &search.keywords {
        let kw = kw.trim();
        if !kw.is_empty() {
            queries.push(kw.to_string());
        }
    }

    for (i, q) in queries.into_iter().take(8).enumerate() {
        if let Some(p) = search_top(&provider, &q, mc, loader) {
            let slug = if p.slug.is_empty() {
                p.id.clone()
            } else {
                p.slug.clone()
            };
            let key = slug.to_ascii_lowercase();
            if seen.insert(key) {
                let source = if i == 0 && search.theme.is_some() {
                    "seed"
                } else {
                    "keyword"
                };
                out.push(project_to_candidate(&p, 1000 - (i as u64) * 10, source));
            }
        }
    }
    out
}

/// Merge partner stats into candidates and fill Modrinth descriptions.
pub fn enrich_partners_with_descriptions(
    seeds: &[CandidateAddon],
    partners: &[PartnerStat],
    limit: usize,
) -> Vec<CandidateAddon> {
    let provider = ModrinthProvider::new();
    let mut out = seeds.to_vec();
    let mut seen: HashSet<String> = seeds.iter().map(|c| c.slug.to_ascii_lowercase()).collect();

    for p in partners.iter().take(limit.max(1)) {
        let key = p.partner.to_ascii_lowercase();
        if !seen.insert(key.clone()) {
            continue;
        }
        match provider.get_project(&p.partner) {
            Ok(proj) => {
                out.push(project_to_candidate(&proj, p.pack_count, "partner"));
            }
            Err(_) => {
                // Fallback: search by slug/name.
                if let Some(proj) = search_top(&provider, &p.partner, "", "") {
                    out.push(project_to_candidate(&proj, p.pack_count, "partner"));
                } else {
                    out.push(CandidateAddon {
                        slug: p.partner.clone(),
                        name: p.partner.clone(),
                        summary: String::new(),
                        score: p.pack_count,
                        source: "partner".into(),
                    });
                }
            }
        }
    }
    out
}

/// Partners from local/network ModPairStat list for a seed slug.
pub fn partners_from_pairs(seed: &str, pairs: &[ModPairStat], limit: usize) -> Vec<PartnerStat> {
    let seed = seed.trim().to_ascii_lowercase();
    let mut out: Vec<PartnerStat> = pairs
        .iter()
        .filter_map(|p| {
            let a = p.mod_a.to_ascii_lowercase();
            let b = p.mod_b.to_ascii_lowercase();
            let partner = if a == seed {
                Some(b)
            } else if b == seed {
                Some(a)
            } else {
                None
            }?;
            Some(PartnerStat {
                partner,
                pack_count: p.count,
            })
        })
        .collect();
    out.sort_by(|a, b| b.pack_count.cmp(&a.pack_count));
    out.truncate(limit);
    out
}

pub fn format_candidates_for_prompt(candidates: &[CandidateAddon], limit: usize) -> String {
    if candidates.is_empty() {
        return String::new();
    }
    let mut lines = vec![
        "Catalog candidates (from Modrinth + co-occurrence). Prefer these slugs in PackBrief.mustHave (slugHint) when they match user intent:"
            .to_string(),
    ];
    for (i, c) in candidates.iter().take(limit).enumerate() {
        let summary = if c.summary.trim().is_empty() {
            "(no summary)".to_string()
        } else {
            c.summary.chars().take(200).collect::<String>()
        };
        lines.push(format!(
            "{}. {} ({}) — {} [score={}, source={}]",
            i + 1,
            c.name,
            c.slug,
            summary,
            c.score,
            c.source
        ));
    }
    lines.join("\n")
}

/// Merge partner lists from multiple seeds (dedupe, keep highest pack_count).
pub fn merge_partner_stats(batches: &[Vec<PartnerStat>], limit: usize) -> Vec<PartnerStat> {
    let mut best: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    for batch in batches {
        for p in batch {
            let key = p.partner.to_ascii_lowercase();
            let entry = best.entry(key).or_insert(0);
            if p.pack_count > *entry {
                *entry = p.pack_count;
            }
        }
    }
    let mut out: Vec<PartnerStat> = best
        .into_iter()
        .map(|(partner, pack_count)| PartnerStat {
            partner,
            pack_count,
        })
        .collect();
    out.sort_by(|a, b| b.pack_count.cmp(&a.pack_count));
    out.truncate(limit.max(1));
    out
}

/// Supabase graph is primary; local pairs only soft-boost matches or fill when SB empty.
pub fn soft_boost_partners(
    primary: &[PartnerStat],
    local: &[PartnerStat],
    limit: usize,
) -> Vec<PartnerStat> {
    let limit = limit.max(1);
    if primary.is_empty() {
        let mut out = local.to_vec();
        out.sort_by(|a, b| b.pack_count.cmp(&a.pack_count));
        out.truncate(limit);
        return out;
    }
    let local_map: std::collections::HashMap<String, u64> = local
        .iter()
        .map(|p| (p.partner.to_ascii_lowercase(), p.pack_count))
        .collect();
    let mut out: Vec<PartnerStat> = primary
        .iter()
        .map(|p| {
            let key = p.partner.to_ascii_lowercase();
            let boost = local_map.get(&key).copied().unwrap_or(0) / 4;
            PartnerStat {
                partner: p.partner.clone(),
                pack_count: p.pack_count.saturating_add(boost),
            }
        })
        .collect();
    out.sort_by(|a, b| b.pack_count.cmp(&a.pack_count));
    out.truncate(limit);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action_plan::parse_action_plan;

    #[test]
    fn industrial_theme_seeds_create() {
        let q = theme_seed_queries("industrial");
        assert!(q.contains(&"create"));
    }

    #[test]
    fn partners_from_pairs_picks_other_side() {
        let pairs = vec![
            ModPairStat {
                mod_a: "create".into(),
                mod_b: "create-aeronautics".into(),
                count: 40,
            },
            ModPairStat {
                mod_a: "jei".into(),
                mod_b: "create".into(),
                count: 90,
            },
        ];
        let p = partners_from_pairs("create", &pairs, 10);
        assert_eq!(p[0].partner, "jei");
        assert_eq!(p[1].partner, "create-aeronautics");
    }

    #[test]
    fn soft_boost_prefers_supabase_primary() {
        let sb = vec![
            PartnerStat {
                partner: "jei".into(),
                pack_count: 100,
            },
            PartnerStat {
                partner: "sodium".into(),
                pack_count: 50,
            },
        ];
        let local = vec![
            PartnerStat {
                partner: "sodium".into(),
                pack_count: 40,
            },
            PartnerStat {
                partner: "local-only".into(),
                pack_count: 999,
            },
        ];
        let out = soft_boost_partners(&sb, &local, 10);
        assert_eq!(out.len(), 2, "local-only must not replace SB graph");
        assert_eq!(out[0].partner, "jei");
        assert!(out
            .iter()
            .any(|p| p.partner == "sodium" && p.pack_count >= 50));
        let empty_sb = soft_boost_partners(&[], &local, 5);
        assert_eq!(empty_sb[0].partner, "local-only");
    }

    #[test]
    fn merge_partners_keeps_highest_score() {
        let a = vec![PartnerStat {
            partner: "create-aeronautics".into(),
            pack_count: 10,
        }];
        let b = vec![PartnerStat {
            partner: "create-aeronautics".into(),
            pack_count: 40,
        }];
        let m = merge_partner_stats(&[a, b], 5);
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].pack_count, 40);
    }

    #[test]
    fn format_candidates_lists_slugs() {
        let c = vec![CandidateAddon {
            slug: "create-aeronautics".into(),
            name: "Create: Aeronautics".into(),
            summary: "Airplanes for Create".into(),
            score: 42,
            source: "partner".into(),
        }];
        let s = format_candidates_for_prompt(&c, 10);
        assert!(s.contains("create-aeronautics"));
        assert!(s.contains("mustHave") || s.contains("slugHint") || s.contains("PackBrief"));
    }

    #[test]
    fn refine_json_parses_action_plan() {
        // Crash ActionPlan stays in action_plan module — Create Mode does not emit it.
        let json = r#"{
          "schemaVersion": 1,
          "humanExplanation": "Install Create and Aeronautics for airplanes.",
          "confidence": 0.8,
          "needsUserReview": true,
          "actions": [
            {"op":"install_mod","modId":"create","provider":"modrinth","reason":"base","risk":"low"},
            {"op":"install_mod","modId":"create-aeronautics","provider":"modrinth","reason":"planes","risk":"low"}
          ]
        }"#;
        let plan = parse_action_plan(json).unwrap();
        assert_eq!(plan.actions.len(), 2);
        assert_eq!(
            plan.actions[1].mod_id.as_deref(),
            Some("create-aeronautics")
        );
    }
}
