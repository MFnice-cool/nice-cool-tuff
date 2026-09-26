use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum GitHubSourceError {
    #[error("not a GitHub repository reference: {0}")]
    Invalid(String),
    #[error("unsafe GitHub ref or tag: {0}")]
    UnsafeRef(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubSource {
    pub owner: String,
    pub repo: String,
    pub git_ref: Option<String>,
}

/// Allowlist branch/tag/commit-ish values used in GitHub API path segments.
/// Rejects path traversal (`..`), absolute paths, and characters outside a safe set.
pub fn validate_github_ref(value: &str) -> Result<(), GitHubSourceError> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.starts_with('/')
        || trimmed.ends_with('/')
        || trimmed.contains('\\')
        || trimmed.contains('\0')
        || trimmed.contains("..")
    {
        return Err(GitHubSourceError::UnsafeRef(value.to_string()));
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'))
    {
        return Err(GitHubSourceError::UnsafeRef(value.to_string()));
    }
    if trimmed
        .split('/')
        .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(GitHubSourceError::UnsafeRef(value.to_string()));
    }
    Ok(())
}

/// Parse `owner/repo`, `gh:owner/repo[:ref]`, or `https://github.com/owner/repo[/...]`.
pub fn parse_github_source(input: &str) -> Result<GitHubSource, GitHubSourceError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(GitHubSourceError::Invalid(input.to_string()));
    }

    let rest = trimmed
        .strip_prefix("gh:")
        .or_else(|| trimmed.strip_prefix("https://github.com/"))
        .or_else(|| trimmed.strip_prefix("http://github.com/"))
        .or_else(|| trimmed.strip_prefix("github.com/"))
        .unwrap_or(trimmed)
        .trim_end_matches('/');

    let mut parts = rest.split('/').filter(|p| !p.is_empty());
    let owner = parts
        .next()
        .ok_or_else(|| GitHubSourceError::Invalid(input.to_string()))?;
    let repo_raw = parts
        .next()
        .ok_or_else(|| GitHubSourceError::Invalid(input.to_string()))?;
    let extra: Vec<&str> = parts.collect();

    let (repo, colon_ref) = match repo_raw.split_once(':') {
        Some((repo, tag)) => (repo.trim_end_matches(".git"), Some(tag.to_string())),
        None => (repo_raw.trim_end_matches(".git"), None),
    };

    if owner.is_empty()
        || repo.is_empty()
        || !owner
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        || !repo
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(GitHubSourceError::Invalid(input.to_string()));
    }

    let git_ref = colon_ref.or_else(|| match extra.as_slice() {
        ["tree", r] | ["commit", r] | ["releases", "tag", r] => Some((*r).to_string()),
        _ => None,
    });
    if let Some(git_ref) = git_ref.as_deref() {
        validate_github_ref(git_ref)?;
    }

    Ok(GitHubSource {
        owner: owner.to_string(),
        repo: repo.to_string(),
        git_ref,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_owner_repo() {
        let src = parse_github_source("acme/cool-pack").unwrap();
        assert_eq!(src.owner, "acme");
        assert_eq!(src.repo, "cool-pack");
        assert_eq!(src.git_ref, None);
    }

    #[test]
    fn parses_https_and_tag() {
        let src =
            parse_github_source("https://github.com/acme/cool-pack/releases/tag/v1.2.3").unwrap();
        assert_eq!(src.owner, "acme");
        assert_eq!(src.repo, "cool-pack");
        assert_eq!(src.git_ref.as_deref(), Some("v1.2.3"));
    }

    #[test]
    fn parses_gh_prefix_with_tag() {
        let src = parse_github_source("gh:acme/cool-pack:v1.0.0").unwrap();
        assert_eq!(src.owner, "acme");
        assert_eq!(src.repo, "cool-pack");
        assert_eq!(src.git_ref.as_deref(), Some("v1.0.0"));
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_github_source("not a repo").is_err());
        assert!(parse_github_source("").is_err());
    }

    #[test]
    fn rejects_path_traversal_ref() {
        assert!(matches!(
            parse_github_source("gh:acme/cool-pack:../../../victim/private"),
            Err(GitHubSourceError::UnsafeRef(_))
        ));
        assert!(validate_github_ref("../../other/releases/tags/v1").is_err());
        assert!(validate_github_ref("heads/main").is_ok());
        assert!(validate_github_ref("v1.2.3").is_ok());
    }
}
