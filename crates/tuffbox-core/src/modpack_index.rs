//! Modpack Index API client ([modpackindex.com](https://www.modpackindex.com/api)).
//!
//! Free, rate-limited (~3600 req/hr). Always send a descriptive User-Agent.
//! Prefer hub-side analytics for bulk stats so end-user IPs are not sent to MPI.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::sync::OnceLock;

pub const MPI_UA: &str = "TuffBox/0.1 (https://github.com/MFcrychelt/tuffbox)";
/// Hub analytics crawl User-Agent (Modpack Index policy: descriptive UA).
pub const MPI_ANALYTICS_UA: &str =
    "TuffSwarm-Analytics/1.0 (https://github.com/MFcrychelt/tuffbox)";
pub const MPI_BASE: &str = "https://www.modpackindex.com/api/v1";

/// CurseForge modpack class categories mirrored by Modpack Index (root 4471).
pub const PACK_THEMES: &[MpiCategoryStatic] = &[
    MpiCategoryStatic {
        id: 39,
        slug: "sci-fi",
        name: "Sci-Fi",
        kind: "modpack",
    },
    MpiCategoryStatic {
        id: 40,
        slug: "small-light",
        name: "Small / Light",
        kind: "modpack",
    },
    MpiCategoryStatic {
        id: 41,
        slug: "combat-pvp",
        name: "Combat / PvP",
        kind: "modpack",
    },
    MpiCategoryStatic {
        id: 42,
        slug: "mini-game",
        name: "Mini Game",
        kind: "modpack",
    },
    MpiCategoryStatic {
        id: 43,
        slug: "quests",
        name: "Quests",
        kind: "modpack",
    },
    MpiCategoryStatic {
        id: 44,
        slug: "multiplayer",
        name: "Multiplayer",
        kind: "modpack",
    },
    MpiCategoryStatic {
        id: 45,
        slug: "exploration",
        name: "Exploration",
        kind: "modpack",
    },
    MpiCategoryStatic {
        id: 46,
        slug: "skyblock",
        name: "Skyblock",
        kind: "modpack",
    },
    MpiCategoryStatic {
        id: 47,
        slug: "adventure-and-rpg",
        name: "Adventure and RPG",
        kind: "modpack",
    },
    MpiCategoryStatic {
        id: 48,
        slug: "ftb-official-pack",
        name: "FTB Official Pack",
        kind: "modpack",
    },
    MpiCategoryStatic {
        id: 49,
        slug: "map-based",
        name: "Map Based",
        kind: "modpack",
    },
    MpiCategoryStatic {
        id: 50,
        slug: "hardcore",
        name: "Hardcore",
        kind: "modpack",
    },
    MpiCategoryStatic {
        id: 51,
        slug: "tech",
        name: "Tech",
        kind: "modpack",
    },
    MpiCategoryStatic {
        id: 52,
        slug: "extra-large",
        name: "Extra Large",
        kind: "modpack",
    },
    MpiCategoryStatic {
        id: 53,
        slug: "magic",
        name: "Magic",
        kind: "modpack",
    },
];

#[derive(Debug, Clone, Copy)]
pub struct MpiCategoryStatic {
    pub id: u32,
    pub slug: &'static str,
    pub name: &'static str,
    pub kind: &'static str,
}

/// Structured search intent from local AI (Create Mode step 1).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MpiSearchQuery {
    #[serde(default)]
    pub loader: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    /// Theme / pack tag slug or alias (`industrial` → `tech`).
    #[serde(default)]
    pub theme: Option<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MpiCategory {
    pub id: u32,
    pub slug: String,
    pub name: String,
    /// `"mod"` | `"modpack"`
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MpiModHint {
    pub name: String,
    pub slug: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub categories: Vec<String>,
    /// Why this hit was included (`keyword:airplanes`, `theme:tech`, …).
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MpiPaged<T> {
    pub data: Vec<T>,
    pub total: u32,
}

fn client_with_ua(ua: &str) -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .user_agent(ua)
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())
}

fn get_value(path: &str, query: &[(String, String)]) -> Result<Value, String> {
    get_value_ua(path, query, MPI_UA)
}

fn get_value_ua(path: &str, query: &[(String, String)], ua: &str) -> Result<Value, String> {
    let url = format!("{MPI_BASE}{path}");
    let c = client_with_ua(ua)?;
    let mut req = c.get(&url);
    if !query.is_empty() {
        req = req.query(query);
    }
    let resp = req
        .send()
        .map_err(|e| format!("Modpack Index request failed: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().unwrap_or_default();
        return Err(format!("Modpack Index {status}: {body}"));
    }
    resp.json().map_err(|e| format!("Modpack Index JSON: {e}"))
}

fn page_total(body: &Value) -> u32 {
    body.get("meta")
        .and_then(|m| m.get("total"))
        .and_then(|t| t.as_u64())
        .unwrap_or(0) as u32
}

fn page_data(body: &Value) -> Vec<Value> {
    body.get("data")
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default()
}

fn map_modpack_hit(item: &Value) -> Value {
    let id = item
        .get("id")
        .map(|v| v.to_string().trim_matches('"').to_string())
        .unwrap_or_default();
    let slug = item
        .get("slug")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let name = item
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let description = item
        .get("summary")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let icon_url = item.get("thumbnail_url").cloned().unwrap_or(Value::Null);
    let downloads = item.get("download_count").and_then(|v| v.as_u64());
    let page_url = item
        .get("page_url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let url = item
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let links = item.get("links").cloned().unwrap_or(serde_json::json!({}));
    let categories = item
        .get("categories")
        .cloned()
        .unwrap_or(Value::Array(vec![]));
    serde_json::json!({
        "id": id,
        "slug": slug,
        "name": name,
        "description": description,
        "iconUrl": icon_url,
        "downloads": downloads,
        "follows": null,
        "projectType": "modpack",
        "pageUrl": page_url,
        "url": url,
        "links": links,
        "categories": categories,
        "provider": "modpackindex",
    })
}

fn map_mod_hit(item: &Value) -> Value {
    let id = item
        .get("id")
        .map(|v| v.to_string().trim_matches('"').to_string())
        .unwrap_or_default();
    let slug = item
        .get("slug")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let name = item
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let description = item
        .get("summary")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let icon_url = item.get("thumbnail_url").cloned().unwrap_or(Value::Null);
    let downloads = item.get("download_count").and_then(|v| v.as_u64());
    let categories = item
        .get("categories")
        .and_then(|c| c.as_array())
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|c| {
            c.get("slug")
                .and_then(|s| s.as_str())
                .map(|s| s.to_string())
        })
        .collect::<Vec<_>>();
    let page_url = item
        .get("page_url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    serde_json::json!({
        "id": id,
        "slug": slug,
        "name": name,
        "description": description,
        "iconUrl": icon_url,
        "downloads": downloads,
        "follows": null,
        "projectType": "mod",
        "pageUrl": page_url,
        "categories": categories,
        "provider": "modpackindex",
    })
}

/// Resolve a free-text theme/alias to a pack-theme category id.
pub fn resolve_theme_category_id(theme: &str) -> Option<u32> {
    let t = theme.trim().to_ascii_lowercase().replace('_', "-");
    if t.is_empty() {
        return None;
    }
    if let Ok(id) = t.parse::<u32>() {
        return Some(id);
    }
    let alias = match t.as_str() {
        "industrial" | "industry" | "tech" | "technology" | "create" | "mekanism" | "factory"
        | "automation" => "tech",
        "magic" | "magical" | "wizard" | "witchery" => "magic",
        "scifi" | "sci-fi" | "science-fiction" => "sci-fi",
        "rpg" | "adventure" | "adventure-and-rpg" => "adventure-and-rpg",
        "pvp" | "combat" | "combat-pvp" => "combat-pvp",
        "light" | "lightweight" | "small" | "small-light" => "small-light",
        "kitchen-sink" | "kitchen" | "extra-large" | "kitchen_sink" => "extra-large",
        other => other,
    };
    PACK_THEMES.iter().find(|c| c.slug == alias).map(|c| c.id)
}

/// Mod-category slug useful as a secondary filter for theme (API `/categories`).
pub fn resolve_mod_category_id(theme: &str) -> Option<u32> {
    let t = theme.trim().to_ascii_lowercase().replace('_', "-");
    match t.as_str() {
        "industrial" | "industry" | "tech" | "technology" | "create" => Some(9), // technology
        "magic" | "magical" => Some(8),
        "adventure" | "rpg" | "adventure-and-rpg" => Some(20),
        "transport" | "transportation" | "flight" | "airplane" | "airplanes" | "vehicles" => {
            Some(12)
        } // player-transport
        "storage" => Some(6),
        "redstone" => Some(10),
        _ => None,
    }
}

static VERSION_CACHE: OnceLock<Vec<(u32, String)>> = OnceLock::new();

fn load_versions() -> Result<&'static [(u32, String)], String> {
    if let Some(v) = VERSION_CACHE.get() {
        return Ok(v.as_slice());
    }
    let body = get_value(
        "/minecraft/versions",
        &[("limit".into(), "200".into()), ("page".into(), "1".into())],
    )?;
    let mut out = Vec::new();
    for item in page_data(&body) {
        let id = item.get("id").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let name = item
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if id > 0 && !name.is_empty() {
            out.push((id, name));
        }
    }
    let _ = VERSION_CACHE.set(out);
    Ok(VERSION_CACHE.get().map(|v| v.as_slice()).unwrap_or(&[]))
}

pub fn resolve_mc_version_id(version: &str) -> Option<u32> {
    let needle = version.trim();
    if needle.is_empty() {
        return None;
    }
    if let Ok(id) = needle.parse::<u32>() {
        return Some(id);
    }
    let versions = load_versions().ok()?;
    versions
        .iter()
        .find(|(_, name)| name.eq_ignore_ascii_case(needle))
        .map(|(id, _)| *id)
}

pub fn list_categories() -> Result<Vec<MpiCategory>, String> {
    list_categories_with_ua(MPI_UA)
}

/// Pack-theme chips only (no MPI network). Enough for Create Mode / Creation Trends filters.
pub fn list_pack_theme_categories() -> Vec<MpiCategory> {
    PACK_THEMES
        .iter()
        .map(|c| MpiCategory {
            id: c.id,
            slug: c.slug.to_string(),
            name: c.name.to_string(),
            kind: c.kind.to_string(),
        })
        .collect()
}

/// Hub-side category list (analytics UA when merging MPI mod categories).
pub fn list_categories_hub() -> Result<Vec<MpiCategory>, String> {
    list_categories_with_ua(MPI_ANALYTICS_UA)
}

fn list_categories_with_ua(ua: &str) -> Result<Vec<MpiCategory>, String> {
    let body = get_value_ua("/categories", &[], ua)?;
    let mut out = list_pack_theme_categories();
    let mut seen: HashSet<u32> = out.iter().map(|c| c.id).collect();
    for item in page_data(&body) {
        let id = item.get("id").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        if id == 0 || !seen.insert(id) {
            continue;
        }
        let slug = item
            .get("slug")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let name = item
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        out.push(MpiCategory {
            id,
            slug,
            name,
            kind: "mod".into(),
        });
    }
    out.sort_by(|a, b| a.kind.cmp(&b.kind).then(a.name.cmp(&b.name)));
    Ok(out)
}

pub fn search_modpacks(
    name: Option<&str>,
    page: u32,
    limit: u32,
    category_id: Option<u32>,
    version_id: Option<u32>,
    launcher_id: Option<u32>,
) -> Result<(Vec<Value>, u32), String> {
    search_modpacks_with_ua(
        name,
        page,
        limit,
        category_id,
        version_id,
        launcher_id,
        MPI_UA,
    )
}

/// Hub-side pack search (analytics UA). Clients should call the hub, not MPI directly.
pub fn search_modpacks_hub(
    name: Option<&str>,
    page: u32,
    limit: u32,
    category_id: Option<u32>,
    version_id: Option<u32>,
) -> Result<(Vec<Value>, u32), String> {
    search_modpacks_with_ua(
        name,
        page,
        limit,
        category_id,
        version_id,
        None,
        MPI_ANALYTICS_UA,
    )
}

fn search_modpacks_with_ua(
    name: Option<&str>,
    page: u32,
    limit: u32,
    category_id: Option<u32>,
    version_id: Option<u32>,
    launcher_id: Option<u32>,
    ua: &str,
) -> Result<(Vec<Value>, u32), String> {
    let limit = limit.clamp(1, 100);
    let page = page.max(1);
    let path = if let Some(id) = category_id {
        format!("/category/{id}/modpacks")
    } else if let Some(id) = version_id {
        format!("/minecraft/version/{id}/modpacks")
    } else if let Some(id) = launcher_id {
        format!("/launcher/{id}/modpacks")
    } else {
        "/modpacks".to_string()
    };

    let mut q = vec![
        ("page".into(), page.to_string()),
        ("limit".into(), limit.to_string()),
    ];
    if category_id.is_none() && version_id.is_none() && launcher_id.is_none() {
        let token = name
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .unwrap_or("the");
        q.push(("name".into(), token.to_string()));
    }

    let body = get_value_ua(&path, &q, ua)?;
    let mut results: Vec<Value> = page_data(&body).iter().map(map_modpack_hit).collect();
    let total = page_total(&body);

    if let Some(raw) = name.map(|s| s.trim()).filter(|s| !s.is_empty()) {
        if category_id.is_some() || version_id.is_some() || launcher_id.is_some() {
            let needle = raw.to_ascii_lowercase();
            results.retain(|h| {
                h.get("name")
                    .and_then(|v| v.as_str())
                    .map(|n| n.to_ascii_lowercase().contains(&needle))
                    .unwrap_or(false)
                    || h.get("slug")
                        .and_then(|v| v.as_str())
                        .map(|n| n.to_ascii_lowercase().contains(&needle))
                        .unwrap_or(false)
            });
        }
    }

    Ok((results, total))
}

pub fn search_mods(
    name: Option<&str>,
    page: u32,
    limit: u32,
    category_id: Option<u32>,
) -> Result<(Vec<Value>, u32), String> {
    let limit = limit.clamp(1, 100);
    let page = page.max(1);
    let path = if let Some(id) = category_id {
        format!("/category/{id}/mods")
    } else {
        "/mods".to_string()
    };
    let mut q = vec![
        ("page".into(), page.to_string()),
        ("limit".into(), limit.to_string()),
    ];
    if category_id.is_none() {
        let token = name
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .unwrap_or("the");
        q.push(("name".into(), token.to_string()));
    }
    let body = get_value(&path, &q)?;
    let mut results: Vec<Value> = page_data(&body).iter().map(map_mod_hit).collect();
    let total = page_total(&body);
    if let Some(raw) = name.map(|s| s.trim()).filter(|s| !s.is_empty()) {
        if category_id.is_some() {
            let needle = raw.to_ascii_lowercase();
            results.retain(|h| {
                h.get("name")
                    .and_then(|v| v.as_str())
                    .map(|n| n.to_ascii_lowercase().contains(&needle))
                    .unwrap_or(false)
                    || h.get("slug")
                        .and_then(|v| v.as_str())
                        .map(|n| n.to_ascii_lowercase().contains(&needle))
                        .unwrap_or(false)
                    || h.get("description")
                        .and_then(|v| v.as_str())
                        .map(|n| n.to_ascii_lowercase().contains(&needle))
                        .unwrap_or(false)
            });
        }
    }
    Ok((results, total))
}

fn value_to_hint(item: &Value, source: &str) -> Option<MpiModHint> {
    let name = item.get("name").and_then(|v| v.as_str())?.to_string();
    let slug = item
        .get("slug")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if name.is_empty() {
        return None;
    }
    let summary = item
        .get("description")
        .or_else(|| item.get("summary"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let categories = item
        .get("categories")
        .and_then(|c| c.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|c| {
                    if let Some(s) = c.as_str() {
                        Some(s.to_string())
                    } else {
                        c.get("slug")
                            .and_then(|s| s.as_str())
                            .map(|s| s.to_string())
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    Some(MpiModHint {
        name,
        slug,
        summary,
        categories,
        source: source.to_string(),
    })
}

/// Gather mod name/tag candidates for Create Mode (step 3 context).
pub fn gather_search_hints(
    query: &MpiSearchQuery,
    per_source: usize,
) -> Result<Vec<MpiModHint>, String> {
    let per_source = per_source.clamp(1, 15);
    let mut out = Vec::new();
    let mut seen = HashSet::new();

    let push_page =
        |out: &mut Vec<MpiModHint>, seen: &mut HashSet<String>, hits: Vec<Value>, source: &str| {
            for h in hits {
                if let Some(hint) = value_to_hint(&h, source) {
                    let key = if hint.slug.is_empty() {
                        hint.name.to_ascii_lowercase()
                    } else {
                        hint.slug.to_ascii_lowercase()
                    };
                    if seen.insert(key) {
                        out.push(hint);
                    }
                }
            }
        };

    if let Some(theme) = query.theme.as_deref().filter(|s| !s.trim().is_empty()) {
        if let Some(pack_id) = resolve_theme_category_id(theme) {
            // Theme packs → sample mods from a top pack is heavy; use mod category instead when available.
            if let Some(mod_cat) = resolve_mod_category_id(theme) {
                let (hits, _) = search_mods(None, 1, per_source as u32, Some(mod_cat))?;
                push_page(&mut out, &mut seen, hits, &format!("theme:{theme}"));
            } else {
                let (packs, _) = search_modpacks(None, 1, 3, Some(pack_id), None, None)?;
                let _ = packs; // pack list is for UI; mod hints come from keywords / mod cats
            }
        } else if let Some(mod_cat) = resolve_mod_category_id(theme) {
            let (hits, _) = search_mods(None, 1, per_source as u32, Some(mod_cat))?;
            push_page(&mut out, &mut seen, hits, &format!("theme:{theme}"));
        }
    }

    for kw in &query.keywords {
        let kw = kw.trim();
        if kw.is_empty() {
            continue;
        }
        let (hits, _) = search_mods(Some(kw), 1, per_source as u32, None)?;
        push_page(&mut out, &mut seen, hits, &format!("keyword:{kw}"));
    }

    // Theme-as-keyword fallback (e.g. "industrial" → Create / IC2 hits).
    if let Some(theme) = query.theme.as_deref().filter(|s| !s.trim().is_empty()) {
        if out.len() < per_source {
            let (hits, _) = search_mods(Some(theme), 1, per_source as u32, None)?;
            push_page(&mut out, &mut seen, hits, &format!("theme-name:{theme}"));
        }
    }

    Ok(out)
}

pub fn format_hints_for_prompt(hints: &[MpiModHint], limit: usize) -> String {
    if hints.is_empty() {
        return String::new();
    }
    let mut lines = Vec::new();
    lines.push(
        "Modpack Index candidates (name + summary). Prefer mustHave queries that match user intent:"
            .to_string(),
    );
    for (i, h) in hints.iter().take(limit).enumerate() {
        let summary = if h.summary.trim().is_empty() {
            "(no summary)".to_string()
        } else {
            h.summary.chars().take(180).collect::<String>()
        };
        lines.push(format!(
            "{}. {} ({}) — {} [{}]",
            i + 1,
            h.name,
            if h.slug.is_empty() { "?" } else { &h.slug },
            summary,
            h.source
        ));
    }
    lines.join("\n")
}

/// Compact tag list for the Create Mode system prompt.
pub fn format_tags_for_prompt() -> String {
    let pack = PACK_THEMES
        .iter()
        .map(|c| format!("{} ({})", c.slug, c.name))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "Modpack Index pack themes (use as search.theme): {pack}. \
         Aliases: industrial/tech/create→tech, magical→magic, scifi→sci-fi, rpg→adventure-and-rpg. \
         Keywords are free-text mod name searches (airplanes, create, jei, …)."
    )
}

/// Prefer Modrinth slug from an MPI mod record; fall back to MPI slug.
pub fn prefer_modrinth_slug(item: &Value) -> Option<String> {
    if let Some(arr) = item.get("modrinth_info").and_then(|v| v.as_array()) {
        if let Some(slug) = arr
            .first()
            .and_then(|m| m.get("slug"))
            .and_then(|s| s.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return Some(slug.to_ascii_lowercase());
        }
    }
    if let Some(url) = item
        .get("links")
        .and_then(|l| l.get("modrinth"))
        .and_then(|v| v.as_str())
    {
        if let Some(slug) = url
            .rsplit('/')
            .next()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return Some(slug.to_ascii_lowercase());
        }
    }
    item.get("slug")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase())
}

/// List mods in a Modpack Index pack (paginated). Uses analytics UA when `analytics` is true.
pub fn modpack_mods(
    pack_id: u64,
    page: u32,
    limit: u32,
    analytics: bool,
) -> Result<(Vec<Value>, u32), String> {
    let limit = limit.clamp(1, 100);
    let page = page.max(1);
    let path = format!("/modpack/{pack_id}/mods");
    let q = vec![
        ("page".into(), page.to_string()),
        ("limit".into(), limit.to_string()),
    ];
    let ua = if analytics { MPI_ANALYTICS_UA } else { MPI_UA };
    let body = get_value_ua(&path, &q, ua)?;
    Ok((page_data(&body), page_total(&body)))
}

/// Collect unique Modrinth-oriented slugs from a pack (up to `max_mods`).
pub fn modpack_mod_slugs(
    pack_id: u64,
    max_mods: usize,
    analytics: bool,
) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut page = 1u32;
    while out.len() < max_mods && page <= 10 {
        let (batch, total) = modpack_mods(pack_id, page, 100, analytics)?;
        if batch.is_empty() {
            break;
        }
        for item in batch {
            if let Some(slug) = prefer_modrinth_slug(&item) {
                if seen.insert(slug.clone()) {
                    out.push(slug);
                    if out.len() >= max_mods {
                        break;
                    }
                }
            }
        }
        if (page as u64) * 100 >= total as u64 && total > 0 {
            break;
        }
        page += 1;
    }
    Ok(out)
}

/// Search modpacks for a MC version using analytics UA (hub crawl).
pub fn search_modpacks_for_version_analytics(
    version: &str,
    page: u32,
    limit: u32,
) -> Result<(Vec<Value>, u32), String> {
    let version_id = resolve_mc_version_id(version)
        .ok_or_else(|| format!("unknown Minecraft version for MPI: {version}"))?;
    let limit = limit.clamp(1, 100);
    let page = page.max(1);
    let path = format!("/minecraft/version/{version_id}/modpacks");
    let q = vec![
        ("page".into(), page.to_string()),
        ("limit".into(), limit.to_string()),
    ];
    let body = get_value_ua(&path, &q, MPI_ANALYTICS_UA)?;
    Ok((
        page_data(&body).iter().map(map_modpack_hit).collect(),
        page_total(&body),
    ))
}

/// Expand a mod id list into ordered pair JSON rows for MPI / launcher seed RPCs.
pub fn expand_pair_rows(
    mod_ids: &[String],
    mc_version: &str,
    loader: &str,
    weight: u64,
    source: &str,
) -> Vec<Value> {
    expand_pair_rows_with_category(mod_ids, mc_version, loader, weight, source, "")
}

/// Same as [`expand_pair_rows`] with optional pack-theme `category_slug` (MPI graph).
pub fn expand_pair_rows_with_category(
    mod_ids: &[String],
    mc_version: &str,
    loader: &str,
    weight: u64,
    source: &str,
    category_slug: &str,
) -> Vec<Value> {
    let mut ids: Vec<String> = mod_ids
        .iter()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    ids.sort();
    ids.dedup();
    let weight = weight.max(1);
    let cat = category_slug.trim().to_ascii_lowercase();
    let mut out = Vec::new();
    for i in 0..ids.len() {
        for j in (i + 1)..ids.len() {
            out.push(serde_json::json!({
                "mod_a": ids[i],
                "mod_b": ids[j],
                "mc_version": mc_version,
                "loader": loader.to_ascii_lowercase(),
                "weight": weight,
                "last_source": source,
                "category_slug": cat,
            }));
        }
    }
    out
}

/// Hub-side: list packs for a pack-theme category (analytics UA).
pub fn search_modpacks_by_category_analytics(
    category_id: u32,
    page: u32,
    limit: u32,
) -> Result<(Vec<Value>, u32), String> {
    search_modpacks_with_ua(
        None,
        page,
        limit,
        Some(category_id),
        None,
        None,
        MPI_ANALYTICS_UA,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_aliases_resolve() {
        assert_eq!(resolve_theme_category_id("industrial"), Some(51));
        assert_eq!(resolve_theme_category_id("TECH"), Some(51));
        assert_eq!(resolve_theme_category_id("magic"), Some(53));
        assert_eq!(resolve_theme_category_id("sci-fi"), Some(39));
        assert_eq!(resolve_mod_category_id("airplanes"), Some(12));
    }

    #[test]
    fn search_query_parses() {
        let raw = r#"{"loader":"neoforge","version":"1.21.1","theme":"industrial","keywords":["airplanes","flight"]}"#;
        let q: MpiSearchQuery = serde_json::from_str(raw).unwrap();
        assert_eq!(q.loader.as_deref(), Some("neoforge"));
        assert_eq!(q.keywords.len(), 2);
    }

    #[test]
    fn expand_pairs_orders_and_weights() {
        let rows = expand_pair_rows(
            &["create".into(), "jei".into(), "create".into()],
            "1.21.1",
            "NeoForge",
            3,
            "mpi",
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["mod_a"], "create");
        assert_eq!(rows[0]["mod_b"], "jei");
        assert_eq!(rows[0]["weight"], 3);
        assert_eq!(rows[0]["loader"], "neoforge");
        assert_eq!(rows[0]["category_slug"], "");
    }

    #[test]
    fn expand_pairs_with_category_slug() {
        let rows = expand_pair_rows_with_category(
            &["jei".into(), "create".into()],
            "",
            "",
            2,
            "mpi",
            "Tech",
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["category_slug"], "tech");
        assert_eq!(rows[0]["mod_a"], "create");
    }
}
