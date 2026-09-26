//! Collect FTB Quests `.snbt` files from popular CurseForge modpacks.
//!
//! Modpack `.zip` archives are downloaded into RAM only — never written to disk.
//! Only matching `.snbt` under `config/ftbquests/` (incl. `overrides/`) are saved.

use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde::Deserialize;
use sha1::{Digest, Sha1};
use std::collections::HashSet;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::Semaphore;
use zip::ZipArchive;

use tuffbox_core::provider::curseforge::{
    curseforge_cdn_urls, url_needs_curseforge_key, CurseForgeProvider,
};

const CF_API: &str = "https://api.curseforge.com/v1";
const USER_AGENT: &str =
    "tuffbox-cli/0.1.0 (FTB Quests dataset collector; github.com/MFcrychelt/tuffbox)";
const MINECRAFT_GAME_ID: u32 = 432;
const CLASS_MODPACK: u32 = 4471;
/// CurseForge modpack category "Quests".
const CATEGORY_QUESTS: u32 = 4478;
/// CurseForge search page size hard limit.
const SEARCH_PAGE: u32 = 50;
/// CurseForge `releaseType`: 1 = Release, 2 = Beta, 3 = Alpha.
const RELEASE_TYPE_RELEASE: u32 = 1;

#[derive(Debug, Clone)]
pub struct CollectOptions {
    /// Max modpacks to process (most-downloaded first).
    pub limit: u32,
    /// Output directory for extracted `.snbt` files.
    pub out_dir: PathBuf,
    /// Max concurrent zip downloads (keeps RAM bounded).
    pub concurrency: usize,
    /// Optional free-text search query.
    pub query: Option<String>,
    /// CurseForge category id, or `None` to search all modpacks.
    /// Default callers pass Quests (`4478`).
    pub category_id: Option<u32>,
}

#[derive(Debug, Default)]
pub struct CollectStats {
    pub modpacks_found: usize,
    pub modpacks_ok: usize,
    pub modpacks_skipped: usize,
    pub modpacks_failed: usize,
    pub snbt_written: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfData<T> {
    data: T,
    #[serde(default)]
    pagination: CfPagination,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfPagination {
    #[serde(default)]
    total_count: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfModHit {
    id: u64,
    #[serde(default)]
    slug: String,
    #[serde(default)]
    #[allow(dead_code)]
    name: String,
    #[serde(default)]
    download_count: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfFile {
    id: u64,
    #[serde(default)]
    file_name: String,
    #[serde(default)]
    download_url: Option<String>,
    #[serde(default)]
    release_type: u32,
    #[serde(default)]
    file_length: u64,
}

#[derive(Debug, Clone)]
struct PackRef {
    id: u64,
    slug: String,
    downloads: u64,
}

/// Parse `--category`: `quests` → 4478, numeric id, or `none`/`-` → disabled.
pub fn parse_category_arg(raw: &str) -> Result<Option<u32>> {
    let t = raw.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("none") || t == "-" {
        return Ok(None);
    }
    if t.eq_ignore_ascii_case("quests") {
        return Ok(Some(CATEGORY_QUESTS));
    }
    let id: u32 = t.parse().with_context(|| {
        format!("invalid --category '{raw}' (use quests, none, or a numeric id)")
    })?;
    Ok(Some(id))
}

/// Run the collector (async). Call from a tokio runtime.
pub async fn collect_ftb_quests(opts: CollectOptions) -> Result<CollectStats> {
    if opts.limit == 0 {
        return Err(anyhow!("--limit must be >= 1"));
    }
    let concurrency = opts.concurrency.max(1);
    std::fs::create_dir_all(&opts.out_dir)
        .with_context(|| format!("create output dir {}", opts.out_dir.display()))?;

    let api_key = Arc::new(CurseForgeProvider::new().api_key().to_string());
    let client = Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(std::time::Duration::from_secs(30))
        .timeout(std::time::Duration::from_secs(600))
        .redirect(reqwest::redirect::Policy::limited(8))
        .build()
        .context("build HTTP client")?;

    eprintln!(
        "Searching CurseForge modpacks (limit={}, category_id={:?}, query={:?})…",
        opts.limit, opts.category_id, opts.query
    );
    let packs = search_modpacks(&client, &api_key, &opts).await?;
    let mut stats = CollectStats {
        modpacks_found: packs.len(),
        ..Default::default()
    };
    eprintln!(
        "Found {} modpack(s). Downloading with concurrency={}…",
        packs.len(),
        concurrency
    );

    let semaphore = Arc::new(Semaphore::new(concurrency));
    let out_dir = Arc::new(opts.out_dir.clone());
    let written = Arc::new(AtomicUsize::new(0));
    let ok = Arc::new(AtomicUsize::new(0));
    let skipped = Arc::new(AtomicUsize::new(0));
    let failed = Arc::new(AtomicUsize::new(0));

    let mut joins = Vec::with_capacity(packs.len());
    for pack in packs {
        let client = client.clone();
        let api_key = Arc::clone(&api_key);
        let semaphore = Arc::clone(&semaphore);
        let out_dir = Arc::clone(&out_dir);
        let written = Arc::clone(&written);
        let ok = Arc::clone(&ok);
        let skipped = Arc::clone(&skipped);
        let failed = Arc::clone(&failed);

        joins.push(tokio::spawn(async move {
            let _permit = match semaphore.acquire().await {
                Ok(p) => p,
                Err(_) => {
                    failed.fetch_add(1, Ordering::Relaxed);
                    return;
                }
            };
            match process_modpack(&client, &api_key, &pack, out_dir.as_path()).await {
                Ok(0) => {
                    eprintln!(
                        "  skip {}: no config/ftbquests/*.snbt in latest release",
                        pack.slug
                    );
                    skipped.fetch_add(1, Ordering::Relaxed);
                }
                Ok(n) => {
                    eprintln!(
                        "  ok {}: extracted {n} .snbt file(s) ({} downloads)",
                        pack.slug, pack.downloads
                    );
                    written.fetch_add(n, Ordering::Relaxed);
                    ok.fetch_add(1, Ordering::Relaxed);
                }
                Err(e) => {
                    eprintln!("  fail {}: {e:#}", pack.slug);
                    failed.fetch_add(1, Ordering::Relaxed);
                }
            }
        }));
    }

    for join in joins {
        if let Err(e) = join.await {
            eprintln!("  warn: worker task ended unexpectedly: {e}");
            failed.fetch_add(1, Ordering::Relaxed);
        }
    }

    stats.modpacks_ok = ok.load(Ordering::Relaxed);
    stats.modpacks_skipped = skipped.load(Ordering::Relaxed);
    stats.modpacks_failed = failed.load(Ordering::Relaxed);
    stats.snbt_written = written.load(Ordering::Relaxed);
    Ok(stats)
}

async fn search_modpacks(
    client: &Client,
    api_key: &str,
    opts: &CollectOptions,
) -> Result<Vec<PackRef>> {
    let mut collected = Vec::new();
    let mut seen = HashSet::new();
    let mut offset: u32 = 0;
    let target = opts.limit as usize;

    while collected.len() < target {
        let page = (target - collected.len()).min(SEARCH_PAGE as usize) as u32;
        // sortField=2 → TotalDownloads (CurseForge ModsSearchSortField).
        let mut url = format!(
            "{CF_API}/mods/search?gameId={MINECRAFT_GAME_ID}&classId={CLASS_MODPACK}\
             &index={offset}&pageSize={page}&sortField=2&sortOrder=desc"
        );
        if let Some(cat) = opts.category_id {
            url.push_str(&format!("&categoryId={cat}"));
        }
        if let Some(q) = opts.query.as_deref() {
            let q = q.trim();
            if !q.is_empty() {
                url.push_str(&format!("&searchFilter={}", urlencoding_minimal(q)));
            }
        }

        let resp: CfData<Vec<CfModHit>> = cf_get_json(client, api_key, &url).await?;
        if resp.data.is_empty() {
            break;
        }

        let batch_len = resp.data.len() as u32;
        for hit in resp.data {
            if seen.insert(hit.id) {
                let slug = if hit.slug.trim().is_empty() {
                    format!("cf-{}", hit.id)
                } else {
                    hit.slug
                };
                collected.push(PackRef {
                    id: hit.id,
                    slug,
                    downloads: hit.download_count,
                });
                if collected.len() >= target {
                    break;
                }
            }
        }

        offset += batch_len;
        if offset >= resp.pagination.total_count {
            break;
        }
    }

    Ok(collected)
}

async fn process_modpack(
    client: &Client,
    api_key: &str,
    pack: &PackRef,
    out_dir: &Path,
) -> Result<usize> {
    let urls = resolve_latest_release_urls(client, api_key, pack.id)
        .await
        .with_context(|| format!("resolve zip for {}", pack.slug))?;

    eprintln!("  download {} …", pack.slug);
    let bytes = download_bytes(client, api_key, &urls)
        .await
        .with_context(|| format!("download {}", pack.slug))?;

    eprintln!(
        "  {} in RAM ({:.1} MiB) — extracting ftbquests .snbt…",
        pack.slug,
        bytes.len() as f64 / (1024.0 * 1024.0)
    );

    let slug = pack.slug.clone();
    let out_dir = out_dir.to_path_buf();
    tokio::task::spawn_blocking(move || extract_ftb_snbt(&slug, &bytes, &out_dir))
        .await
        .map_err(|e| anyhow!("zip worker join: {e}"))?
}

async fn resolve_latest_release_urls(
    client: &Client,
    api_key: &str,
    mod_id: u64,
) -> Result<Vec<String>> {
    let url = format!("{CF_API}/mods/{mod_id}/files?pageSize=50");
    let resp: CfData<Vec<CfFile>> = cf_get_json(client, api_key, &url).await?;
    if resp.data.is_empty() {
        return Err(anyhow!("no files listed"));
    }

    let release = resp
        .data
        .iter()
        .find(|f| f.release_type == RELEASE_TYPE_RELEASE)
        .or_else(|| resp.data.first())
        .ok_or_else(|| anyhow!("no files listed"))?;

    let mut urls = Vec::new();
    if let Some(u) = release
        .download_url
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        urls.push(u.to_string());
    }
    if !release.file_name.trim().is_empty() {
        for candidate in curseforge_cdn_urls(release.id, &release.file_name) {
            if !urls.iter().any(|u| u == &candidate) {
                urls.push(candidate);
            }
        }
    }
    if urls.is_empty() {
        return Err(anyhow!(
            "no download URL for file {} ({})",
            release.id,
            release.file_name
        ));
    }
    let _ = release.file_length; // available for future progress logging
    Ok(urls)
}

async fn download_bytes(client: &Client, api_key: &str, urls: &[String]) -> Result<Vec<u8>> {
    let mut last_err = None;
    for url in urls {
        let mut req = client.get(url).timeout(std::time::Duration::from_secs(900));
        if url_needs_curseforge_key(url) {
            req = req.header("x-api-key", api_key);
        }
        match req.send().await {
            Ok(resp) => {
                if !resp.status().is_success() {
                    last_err = Some(anyhow!("HTTP {} for {url}", resp.status()));
                    continue;
                }
                match resp.bytes().await {
                    Ok(b) if !b.is_empty() => return Ok(b.to_vec()),
                    Ok(_) => {
                        last_err = Some(anyhow!("empty body from {url}"));
                    }
                    Err(e) => {
                        last_err = Some(anyhow!("read body from {url}: {e}"));
                    }
                }
            }
            Err(e) => {
                last_err = Some(anyhow!("GET {url}: {e}"));
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow!("all CurseForge CDN candidates failed")))
}

async fn cf_get_json<T: serde::de::DeserializeOwned>(
    client: &Client,
    api_key: &str,
    url: &str,
) -> Result<T> {
    let resp = client
        .get(url)
        .header("x-api-key", api_key)
        .header("Accept", "application/json")
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    let status = resp.status();
    let text = resp.text().await.context("read CF response body")?;
    if !status.is_success() {
        return Err(anyhow!(
            "CurseForge API {status}: {}",
            text.chars().take(200).collect::<String>()
        ));
    }
    serde_json::from_str(&text).with_context(|| format!("decode CF JSON from {url}"))
}

fn extract_ftb_snbt(slug: &str, zip_bytes: &[u8], out_dir: &Path) -> Result<usize> {
    let cursor = Cursor::new(zip_bytes);
    let mut archive = ZipArchive::new(cursor).context("open modpack ZIP (corrupt?)")?;

    let mut written = 0usize;
    let mut used_names: HashSet<String> = HashSet::new();

    for i in 0..archive.len() {
        let mut entry = match archive.by_index(i) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("    warn: skip zip entry {i}: {e}");
                continue;
            }
        };
        if entry.is_dir() {
            continue;
        }

        let name = entry.name().replace('\\', "/");
        if !is_ftb_quests_snbt(&name) {
            continue;
        }

        let mut data = Vec::new();
        if let Err(e) = entry.read_to_end(&mut data) {
            eprintln!("    warn: cannot read {name}: {e}");
            continue;
        }

        let out_name = unique_output_name(slug, &name, &data, &mut used_names);
        let dest = out_dir.join(&out_name);
        if let Err(e) = std::fs::write(&dest, &data) {
            eprintln!("    warn: write {}: {e}", dest.display());
            continue;
        }
        written += 1;
    }

    Ok(written)
}

fn is_ftb_quests_snbt(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    if !lower.ends_with(".snbt") {
        return false;
    }
    lower.contains("config/ftbquests/")
}

fn unique_output_name(
    slug: &str,
    archive_path: &str,
    data: &[u8],
    used: &mut HashSet<String>,
) -> String {
    let safe_slug = sanitize_component(slug);
    let file_stem = Path::new(archive_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("quest");
    let safe_stem = sanitize_component(file_stem);

    let candidate = format!("{safe_slug}_{safe_stem}.snbt");
    if used.insert(candidate.clone()) {
        return candidate;
    }

    let mut hasher = Sha1::new();
    hasher.update(archive_path.as_bytes());
    hasher.update(data);
    let digest = hex::encode(hasher.finalize());
    let short = &digest[..8];
    let hashed = format!("{safe_slug}_{safe_stem}_{short}.snbt");
    used.insert(hashed.clone());
    hashed
}

fn sanitize_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "file".into()
    } else {
        out
    }
}

fn urlencoding_minimal(value: &str) -> String {
    let mut out = String::with_capacity(value.len() * 2);
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_ftb_paths() {
        assert!(is_ftb_quests_snbt(
            "overrides/config/ftbquests/chapter.snbt"
        ));
        assert!(is_ftb_quests_snbt("config/ftbquests/quests/foo.snbt"));
        assert!(!is_ftb_quests_snbt("overrides/config/ftbquests/readme.txt"));
        assert!(!is_ftb_quests_snbt("overrides/config/other/foo.snbt"));
    }

    #[test]
    fn output_names_dedupe() {
        let mut used = HashSet::new();
        let a = unique_output_name("pack", "overrides/config/ftbquests/a.snbt", b"1", &mut used);
        let b = unique_output_name("pack", "config/ftbquests/nested/a.snbt", b"2", &mut used);
        assert_eq!(a, "pack_a.snbt");
        assert!(b.starts_with("pack_a_"));
        assert!(b.ends_with(".snbt"));
        assert_ne!(a, b);
    }

    #[test]
    fn category_arg_parsing() {
        assert_eq!(parse_category_arg("quests").unwrap(), Some(CATEGORY_QUESTS));
        assert_eq!(parse_category_arg("4478").unwrap(), Some(4478));
        assert_eq!(parse_category_arg("none").unwrap(), None);
        assert_eq!(parse_category_arg("-").unwrap(), None);
    }
}
