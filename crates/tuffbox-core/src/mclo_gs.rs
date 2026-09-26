//! Client for [mclo.gs](https://mclo.gs) crash/log paste API.
//!
//! Used to share Minecraft crash logs / latest.log without hosting files ourselves.
//! Spec: <https://api.mclo.gs/>

use serde::{Deserialize, Serialize};

use crate::http;

const UPLOAD_URL: &str = "https://api.mclo.gs/1/log";
/// API docs: 10 MiB / 25_000 lines — truncate on the client side first.
const MAX_BYTES: usize = 10 * 1024 * 1024;
const MAX_LINES: usize = 25_000;

#[derive(Debug, Clone, Serialize)]
struct UploadBody<'a> {
    content: &'a str,
    source: &'a str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    metadata: Vec<MetadataEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetadataEntry {
    pub key: String,
    pub value: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UploadResponse {
    pub success: bool,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub raw: Option<String>,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub lines: Option<u64>,
    #[serde(default)]
    pub size: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct SharedLog {
    pub id: String,
    pub url: String,
    pub raw_url: Option<String>,
    pub token: Option<String>,
    pub lines: Option<u64>,
    pub size: Option<u64>,
}

#[derive(Debug, thiserror::Error)]
pub enum McloError {
    #[error("{0}")]
    Api(String),
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("empty log content")]
    Empty,
}

/// Keep the **tail** of large logs (crash signatures are usually near the end).
pub fn truncate_log_content(content: &str) -> String {
    let mut lines: Vec<&str> = content.lines().collect();
    if lines.len() > MAX_LINES {
        lines = lines[lines.len() - MAX_LINES..].to_vec();
    }
    let mut joined = lines.join("\n");
    if joined.len() > MAX_BYTES {
        // Cut from the start so the crash end remains.
        let start = joined.len() - MAX_BYTES;
        let start = joined[start..]
            .find('\n')
            .map(|i| start + i + 1)
            .unwrap_or(start);
        joined = joined[start..].to_string();
    }
    joined
}

/// Upload raw log text to mclo.gs. Returns the public URL on success.
pub fn upload_log(
    content: &str,
    source: &str,
    metadata: Vec<MetadataEntry>,
) -> Result<SharedLog, McloError> {
    let truncated = truncate_log_content(content);
    if truncated.trim().is_empty() {
        return Err(McloError::Empty);
    }
    let body = UploadBody {
        content: &truncated,
        source,
        metadata,
    };
    let resp: UploadResponse = http::post_json(UPLOAD_URL, &body)?;
    if !resp.success {
        return Err(McloError::Api(
            resp.error.unwrap_or_else(|| "mclo.gs upload failed".into()),
        ));
    }
    let id = resp
        .id
        .filter(|s| !s.is_empty())
        .ok_or_else(|| McloError::Api("mclo.gs returned no id".into()))?;
    let url = resp
        .url
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("https://mclo.gs/{id}"));
    Ok(SharedLog {
        id,
        url,
        raw_url: resp.raw,
        token: resp.token,
        lines: resp.lines,
        size: resp.size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_keeps_tail_by_line_count() {
        let mut lines = Vec::new();
        for i in 0..(MAX_LINES + 50) {
            lines.push(format!("line-{i}"));
        }
        let raw = lines.join("\n");
        let out = truncate_log_content(&raw);
        assert!(out.lines().count() <= MAX_LINES);
        assert!(out.contains(&format!("line-{}", MAX_LINES + 49)));
        assert!(!out.contains("line-0"));
    }

    #[test]
    fn truncate_empty_stays_empty() {
        assert!(
            truncate_log_content("   \n  ").trim().is_empty()
                || truncate_log_content("").is_empty()
        );
    }
}
