//! Allowlisted web research for Tune Config Advisor.
//!
//! Used only to ground unknown config keys — never for choosing mod versions.

use serde::{Deserialize, Serialize};
use std::time::Duration;

const MAX_FETCH_CHARS: usize = 40_000;
const FETCH_TIMEOUT_SECS: u64 = 12;
const SEARCH_TIMEOUT_SECS: u64 = 10;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResearchLogEntry {
    pub step: String,
    pub detail: String,
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ResearchBudget {
    pub tool_calls: usize,
    pub fetches: usize,
    pub max_tool_calls: usize,
    pub max_fetches: usize,
}

impl ResearchBudget {
    pub fn new(max_tool_calls: usize, max_fetches: usize) -> Self {
        Self {
            tool_calls: 0,
            fetches: 0,
            max_tool_calls,
            max_fetches,
        }
    }

    pub fn can_tool(&self) -> bool {
        self.tool_calls < self.max_tool_calls
    }

    pub fn can_fetch(&self) -> bool {
        self.fetches < self.max_fetches && self.can_tool()
    }

    pub fn use_tool(&mut self) -> bool {
        if !self.can_tool() {
            return false;
        }
        self.tool_calls += 1;
        true
    }

    pub fn use_fetch(&mut self) -> bool {
        if !self.can_fetch() {
            return false;
        }
        self.tool_calls += 1;
        self.fetches += 1;
        true
    }
}

/// Hard allowlist for fetch_page (plus URLs returned by lookup that already passed).
pub fn is_url_allowed(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    if parsed.scheme() != "https" && parsed.scheme() != "http" {
        return false;
    }
    let host = match parsed.host_str() {
        Some(h) => h.to_ascii_lowercase(),
        None => return false,
    };
    const ALLOWED_SUFFIXES: &[&str] = &[
        "modrinth.com",
        "curseforge.com",
        "forgecdn.net",
        "github.com",
        "githubusercontent.com",
        "wiki.gg",
        "fandom.com",
        "minecraft.wiki",
        "wikia.com",
        "readthedocs.io",
        "readthedocs.org",
        "gitbook.io",
        "duckduckgo.com",
    ];
    if ALLOWED_SUFFIXES.iter().any(|s| host == *s || host.ends_with(&format!(".{s}"))) {
        return true;
    }
    // docs.<something> — only if parent looks like a known project host pattern
    if host.starts_with("docs.") {
        return true;
    }
    false
}

pub fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 4);
    let mut in_tag = false;
    let mut in_script = false;
    let lower = html.to_ascii_lowercase();
    let bytes = html.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if !in_script && lower[i..].starts_with("<script") {
            in_script = true;
            in_tag = true;
            i += 7;
            continue;
        }
        if in_script {
            if lower[i..].starts_with("</script>") {
                in_script = false;
                i += 9;
                continue;
            }
            i += 1;
            continue;
        }
        if !in_tag && lower[i..].starts_with("<style") {
            // skip style similarly
            if let Some(end) = lower[i..].find("</style>") {
                i += end + 8;
                continue;
            }
        }
        let c = bytes[i] as char;
        if c == '<' {
            in_tag = true;
            i += 1;
            continue;
        }
        if c == '>' {
            in_tag = false;
            i += 1;
            out.push(' ');
            continue;
        }
        if !in_tag {
            out.push(c);
        }
        i += 1;
    }
    collapse_ws(&html_unescape(&out))
}

fn collapse_ws(s: &str) -> String {
    let mut out = String::new();
    let mut prev_space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            prev_space = false;
            out.push(c);
        }
    }
    out.trim().to_string()
}

fn html_unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

pub async fn fetch_page(
    url: &str,
    budget: &mut ResearchBudget,
    log: &mut Vec<ResearchLogEntry>,
) -> Result<String, String> {
    if !budget.use_fetch() {
        let msg = "research fetch budget exhausted".to_string();
        log.push(ResearchLogEntry {
            step: "fetch_page".into(),
            detail: msg.clone(),
            ok: false,
            url: Some(url.into()),
        });
        return Err(msg);
    }
    if !is_url_allowed(url) {
        let msg = format!("URL not on allowlist: {url}");
        log.push(ResearchLogEntry {
            step: "fetch_page".into(),
            detail: msg.clone(),
            ok: false,
            url: Some(url.into()),
        });
        return Err(msg);
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(FETCH_TIMEOUT_SECS))
        .user_agent("TuffBox-TuneConfigResearch/1.0")
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client.get(url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        let msg = format!("HTTP {}", resp.status());
        log.push(ResearchLogEntry {
            step: "fetch_page".into(),
            detail: msg.clone(),
            ok: false,
            url: Some(url.into()),
        });
        return Err(msg);
    }
    let body = resp.text().await.map_err(|e| e.to_string())?;
    let text = html_to_text(&body);
    let clipped = if text.len() > MAX_FETCH_CHARS {
        format!("{}…", &text[..MAX_FETCH_CHARS])
    } else {
        text
    };
    log.push(ResearchLogEntry {
        step: "fetch_page".into(),
        detail: format!("fetched {} chars", clipped.len()),
        ok: true,
        url: Some(url.into()),
    });
    Ok(clipped)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// DuckDuckGo HTML search (no API key). Results filtered to allowlisted hosts.
pub async fn web_search(
    query: &str,
    budget: &mut ResearchBudget,
    log: &mut Vec<ResearchLogEntry>,
) -> Result<Vec<SearchHit>, String> {
    if !budget.use_tool() {
        let msg = "research tool budget exhausted".to_string();
        log.push(ResearchLogEntry {
            step: "web_search".into(),
            detail: msg.clone(),
            ok: false,
            url: None,
        });
        return Err(msg);
    }
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let url = format!(
        "https://html.duckduckgo.com/html/?q={}",
        urlencoding_encode(q)
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(SEARCH_TIMEOUT_SECS))
        .user_agent("TuffBox-TuneConfigResearch/1.0")
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .post("https://html.duckduckgo.com/html/")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!("q={}", urlencoding_encode(q)))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        // fallback GET
        let resp2 = client.get(&url).send().await.map_err(|e| e.to_string())?;
        if !resp2.status().is_success() {
            let msg = format!("search HTTP {}", resp2.status());
            log.push(ResearchLogEntry {
                step: "web_search".into(),
                detail: msg.clone(),
                ok: false,
                url: Some(url),
            });
            return Err(msg);
        }
        let body = resp2.text().await.map_err(|e| e.to_string())?;
        let hits = parse_ddg_html(&body);
        log.push(ResearchLogEntry {
            step: "web_search".into(),
            detail: format!("query={q:?} hits={}", hits.len()),
            ok: true,
            url: Some(url),
        });
        return Ok(hits);
    }
    let body = resp.text().await.map_err(|e| e.to_string())?;
    let hits = parse_ddg_html(&body);
    log.push(ResearchLogEntry {
        step: "web_search".into(),
        detail: format!("query={q:?} hits={}", hits.len()),
        ok: true,
        url: None,
    });
    Ok(hits)
}

fn urlencoding_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn parse_ddg_html(html: &str) -> Vec<SearchHit> {
    let mut hits = Vec::new();
    // Very small HTML scrape: look for result__a and result__snippet
    for chunk in html.split("result__a") {
        if !chunk.contains("href=") {
            continue;
        }
        let Some(href_start) = chunk.find("href=\"") else {
            continue;
        };
        let rest = &chunk[href_start + 6..];
        let Some(href_end) = rest.find('"') else {
            continue;
        };
        let mut url = rest[..href_end].to_string();
        // DDG redirect links: //duckduckgo.com/l/?uddg=<encoded>
        if let Some(idx) = url.find("uddg=") {
            let enc = &url[idx + 5..];
            let enc = enc.split('&').next().unwrap_or(enc);
            if let Ok(decoded) = urlencoding_decode(enc) {
                url = decoded;
            }
        }
        if url.starts_with("//") {
            url = format!("https:{url}");
        }
        if !is_url_allowed(&url) {
            continue;
        }
        let title = extract_between(chunk, ">", "</a>").unwrap_or_default();
        let title = html_to_text(&title);
        let snippet = chunk
            .find("result__snippet")
            .and_then(|i| extract_between(&chunk[i..], ">", "</"))
            .map(|s| html_to_text(&s))
            .unwrap_or_default();
        if title.is_empty() && snippet.is_empty() {
            continue;
        }
        hits.push(SearchHit {
            title,
            url,
            snippet,
        });
        if hits.len() >= 6 {
            break;
        }
    }
    hits
}

fn extract_between<'a>(s: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let i = s.find(start)? + start.len();
    let rest = &s[i..];
    let j = rest.find(end)?;
    Some(&rest[..j])
}

fn urlencoding_decode(s: &str) -> Result<String, ()> {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let h = from_hex(bytes[i + 1], bytes[i + 2]).ok_or(())?;
                out.push(h);
                i += 3;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| ())
}

fn from_hex(a: u8, b: u8) -> Option<u8> {
    let nibble = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    };
    Some(nibble(a)? << 4 | nibble(b)?)
}

/// Lookup mod project description via Modrinth API by slug / id.
pub async fn lookup_modrinth_mod(
    slug_or_id: &str,
    budget: &mut ResearchBudget,
    log: &mut Vec<ResearchLogEntry>,
) -> Result<String, String> {
    if !budget.use_tool() {
        return Err("research tool budget exhausted".into());
    }
    let slug = slug_or_id.trim();
    if slug.is_empty() {
        return Err("empty mod id".into());
    }
    let url = format!("https://api.modrinth.com/v2/project/{}", urlencoding_encode(slug));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(FETCH_TIMEOUT_SECS))
        .user_agent("TuffBox-TuneConfigResearch/1.0")
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        let msg = format!("modrinth lookup HTTP {}", resp.status());
        log.push(ResearchLogEntry {
            step: "lookup_mod".into(),
            detail: msg.clone(),
            ok: false,
            url: Some(url),
        });
        return Err(msg);
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let title = v.get("title").and_then(|x| x.as_str()).unwrap_or(slug);
    let desc = v
        .get("description")
        .and_then(|x| x.as_str())
        .unwrap_or("");
    let body = v.get("body").and_then(|x| x.as_str()).unwrap_or("");
    let source = v
        .pointer("/source_url")
        .and_then(|x| x.as_str())
        .or_else(|| v.get("source_url").and_then(|x| x.as_str()))
        .unwrap_or("");
    let wiki = v
        .get("wiki_url")
        .and_then(|x| x.as_str())
        .unwrap_or("");
    let mut snip = format!(
        "Modrinth project: {title} ({slug})\nDescription: {desc}\n"
    );
    if !wiki.is_empty() {
        snip.push_str(&format!("Wiki: {wiki}\n"));
    }
    if !source.is_empty() {
        snip.push_str(&format!("Source: {source}\n"));
    }
    if !body.is_empty() {
        let clipped = if body.len() > 8000 {
            format!("{}…", &body[..8000])
        } else {
            body.to_string()
        };
        snip.push_str("Body:\n");
        snip.push_str(&clipped);
    }
    log.push(ResearchLogEntry {
        step: "lookup_mod".into(),
        detail: format!("modrinth {slug}"),
        ok: true,
        url: Some(url),
    });
    Ok(snip)
}

const MINECRAFT_WIKI_API: &str = "https://minecraft.wiki/api.php";

/// Maximum chars of plaintext extract returned per page.
const MINECRAFT_WIKI_EXTRACT_CHARS: u32 = 6_000;

/// TextExtracts response fields (subset).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MinecraftWikiExtract {
    pub title: String,
    pub page_id: u64,
    pub url: String,
    pub extract: String,
}

/// MediaWiki API base for building the query URL (unit-testable).
fn minecraft_wiki_search_url(query: &str, limit: usize) -> String {
    let limit = limit.clamp(1, 10);
    format!(
        "{MINECRAFT_WIKI_API}?action=query&format=json&formatversion=2&redirects=1&generator=search&gsrnamespace=0&gsrlimit={limit}&gsrsearch={}&prop=extracts&exintro=0&explaintext=1&exlimit=max&exchars={MINECRAFT_WIKI_EXTRACT_CHARS}&exsectionformat=plain",
        urlencoding_encode(query),
    )
}

/// Search minecraft.wiki (MediaWiki Action API) and return plaintext extracts.
///
/// Single round-trip via `generator=search` + `prop=extracts`. Use the result
/// as grounding context for a (local) LLM so it can answer about Minecraft.
pub async fn search_minecraft_wiki(
    query: &str,
    limit: usize,
    budget: &mut ResearchBudget,
    log: &mut Vec<ResearchLogEntry>,
) -> Result<Vec<MinecraftWikiExtract>, String> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    if !budget.use_tool() {
        let msg = "research tool budget exhausted".to_string();
        log.push(ResearchLogEntry {
            step: "minecraft_wiki".into(),
            detail: msg.clone(),
            ok: false,
            url: None,
        });
        return Err(msg);
    }
    let url = minecraft_wiki_search_url(q, limit);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(SEARCH_TIMEOUT_SECS))
        .user_agent("TuffBox-TuneConfigResearch/1.0 (contact: local)")
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        let msg = format!("minecraft.wiki HTTP {}", resp.status());
        log.push(ResearchLogEntry {
            step: "minecraft_wiki".into(),
            detail: msg.clone(),
            ok: false,
            url: Some(url.clone()),
        });
        return Err(msg);
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let Some(pages) = v
        .pointer("/query/pages")
        .and_then(|p| p.as_array())
    else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for p in pages {
        let title = p.get("title").and_then(|x| x.as_str()).unwrap_or("");
        if title.is_empty() {
            continue;
        }
        let page_id = p.get("pageid").and_then(|x| x.as_u64()).unwrap_or(0);
        let extract = p
            .get("extract")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if extract.is_empty() {
            continue;
        }
        let url = format!(
            "https://minecraft.wiki/?curid={page_id}"
        );
        out.push(MinecraftWikiExtract {
            title: title.to_string(),
            page_id,
            url,
            extract,
        });
    }
    out.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()));
    log.push(ResearchLogEntry {
        step: "minecraft_wiki".into(),
        detail: format!("query={q:?} hits={}", out.len()),
        ok: true,
        url: Some(url),
    });
    Ok(out)
}

/// RAG-style lookup: search minecraft.wiki and return a rendered context block
/// suitable for injection into a local model prompt.
pub fn render_minecraft_wiki_context(query: &str, hits: &[MinecraftWikiExtract]) -> String {
    if hits.is_empty() {
        return String::new();
    }
    let mut out = format!("Minecraft Wiki results for {query:?}:\n");
    for h in hits {
        out.push_str(&format!("\n=== {} ===\n{} ({})\n", h.title, h.extract, h.url));
    }
    out
}

/// Tauri command: grounds a local model prompt with minecraft.wiki content.
#[tauri::command]
pub async fn minecraft_wiki_rag_search(
    query: String,
    limit: Option<usize>,
) -> Result<String, String> {
    let mut budget = ResearchBudget::new(1, 0);
    let mut log: Vec<ResearchLogEntry> = Vec::new();
    let hits = search_minecraft_wiki(&query, limit.unwrap_or(3), &mut budget, &mut log).await?;
    Ok(render_minecraft_wiki_context(&query, &hits))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minecraft_wiki_url_encodes_query() {
        let u = minecraft_wiki_search_url("nether portal", 3);
        assert!(u.starts_with("https://minecraft.wiki/api.php?"));
        assert!(u.contains("generator=search"));
        assert!(u.contains("gsrsearch=nether%2Bportal") || u.contains("gsrsearch=nether+portal"));
        assert!(u.contains("prop=extracts"));
        assert!(u.contains("explaintext=1"));
    }

    #[test]
    fn allowlist_accepts_known_hosts() {
        assert!(is_url_allowed("https://modrinth.com/mod/sodium"));
        assert!(is_url_allowed("https://github.com/CaffeineMC/sodium-fabric"));
        assert!(is_url_allowed("https://raw.githubusercontent.com/a/b/main/README.md"));
        assert!(is_url_allowed("https://create.fandom.com/wiki/Config"));
        assert!(!is_url_allowed("https://evil.example.com/x"));
        assert!(!is_url_allowed("javascript:alert(1)"));
    }

    #[test]
    fn budget_stops() {
        let mut b = ResearchBudget::new(2, 1);
        assert!(b.use_tool());
        assert!(b.use_fetch());
        assert!(!b.use_fetch());
        assert!(!b.use_tool());
    }

    #[test]
    fn html_strip_basic() {
        let t = html_to_text("<html><script>evil()</script><p>Hello&nbsp;<b>world</b></p></html>");
        assert!(t.contains("Hello"));
        assert!(t.contains("world"));
        assert!(!t.contains("evil"));
    }
}
