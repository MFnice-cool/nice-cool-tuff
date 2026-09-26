use super::{
    provider_dependency_to_spec, ContentProvider, ModDependencySpec, ProjectInfo,
    ProviderDependency, ProviderError, ProviderFileHashes, ProviderFileInfo, ProviderSearchQuery,
    SearchPage, VersionInfo,
};
use serde::{Deserialize, Deserializer, Serialize};

const BASE_URL: &str = "https://api.modrinth.com/v2";

/// Deserializes a field that can be either a string or an object.
/// Modrinth returns `license` as either a string ID or an object
/// `{"id": "MIT", "name": "MIT License", "url": "..."}`, and
/// `client_side`/`server_side` as either a string or an object
/// `{"client": "optional", "server": "required"}`.
fn string_or_object<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde_json::Value;
    let value = Value::deserialize(deserializer)?;
    match value {
        Value::Null => Ok(None),
        Value::String(s) => Ok(Some(s)),
        Value::Object(map) => {
            if let Some(id) = map.get("id").and_then(|v| v.as_str()) {
                Ok(Some(id.to_string()))
            } else if let Some(name) = map.get("name").and_then(|v| v.as_str()) {
                Ok(Some(name.to_string()))
            } else if let Some(client) = map.get("client").and_then(|v| v.as_str()) {
                Ok(Some(client.to_string()))
            } else {
                Ok(None)
            }
        }
        _ => Ok(None),
    }
}

pub struct ModrinthProvider;

impl ModrinthProvider {
    pub fn new() -> Self {
        Self
    }

    fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, ProviderError> {
        let url = format!("{BASE_URL}{path}");
        crate::http::get_json_with_context(&url).map_err(ProviderError::NetworkContext)
    }

    /// Live category tags from Modrinth (`GET /v2/tag/category`).
    ///
    /// Pass `project_type` (e.g. `"modpack"`) to filter; `None` returns all.
    pub fn list_categories(
        &self,
        project_type: Option<&str>,
    ) -> Result<Vec<ModrinthCategory>, ProviderError> {
        let all: Vec<ModrinthCategory> = self.get_json("/tag/category")?;
        let Some(wanted) = project_type.map(|s| s.trim().to_ascii_lowercase()) else {
            return Ok(all);
        };
        if wanted.is_empty() {
            return Ok(all);
        }
        Ok(all
            .into_iter()
            .filter(|c| c.project_type.eq_ignore_ascii_case(&wanted))
            .collect())
    }

    /// Looks up the Modrinth version that produced a given file, by SHA1 hash.
    ///
    /// This lets TuffBox recognize `.jar` files that were dropped into the
    /// `mods/` folder manually (outside the IDE) and turn them into proper
    /// tracked Modrinth-sourced entries instead of leaving them as opaque
    /// "local" mods forever.
    pub fn get_version_by_hash(&self, sha1: &str) -> Result<Option<VersionInfo>, ProviderError> {
        let key = crate::api_cache::hash_key("modrinth", sha1);
        if let Some(cached) = crate::api_cache::get::<Option<VersionInfo>>(&key) {
            return Ok(cached);
        }
        let url = format!("{BASE_URL}/version_file/{sha1}?algorithm=sha1");
        let version: Option<ModrinthVersion> = crate::http::get_json_optional(&url)?;
        let result = version.map(Into::into);
        crate::api_cache::put(key, result.clone());
        Ok(result)
    }

    /// Resolves the parent project for a version obtained through
    /// [`Self::get_version_by_hash`].
    pub fn identify_local_jar(
        &self,
        sha1: &str,
    ) -> Result<Option<(ProjectInfo, VersionInfo)>, ProviderError> {
        let Some(version) = self.get_version_by_hash(sha1)? else {
            return Ok(None);
        };
        let project = self.get_project(&version.project_id)?;
        Ok(Some((project, version)))
    }

    /// Batch-resolves the latest compatible version for a set of file hashes
    /// using Modrinth's `POST /v2/version_files/update` endpoint.
    ///
    /// Returns a map of `sha1 -> latest VersionInfo` for every hash that has
    /// an update available.
    pub fn get_latest_versions(
        &self,
        hashes: &[String],
        loaders: &[String],
        game_versions: &[String],
    ) -> Result<std::collections::HashMap<String, VersionInfo>, ProviderError> {
        if hashes.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        // Content tab checks updates on every open; a short TTL keyed by the
        // exact request keeps repeated opens instant while staying fresh
        // enough for advisory update dots.
        let cache_key = format!(
            "modrinth:latest:{}|{}|{}",
            hashes.join(","),
            loaders.join(","),
            game_versions.join(",")
        );
        if let Some(cached) =
            crate::api_cache::get::<std::collections::HashMap<String, VersionInfo>>(&cache_key)
        {
            return Ok(cached);
        }
        // Chunk large packs — Modrinth App / Prism keep batch bodies bounded.
        const CHUNK: usize = 256;
        let url = format!("{BASE_URL}/version_files/update");
        let mut merged = std::collections::HashMap::new();
        for chunk in hashes.chunks(CHUNK) {
            let body = serde_json::json!({
                "hashes": chunk,
                "algorithm": "sha1",
                "loaders": loaders,
                "game_versions": game_versions,
            });
            let raw: std::collections::HashMap<String, ModrinthVersion> =
                crate::http::post_json(&url, &body)?;
            merged.extend(raw.into_iter().map(|(k, v)| (k, v.into())));
        }
        crate::api_cache::put_with_ttl(
            cache_key,
            merged.clone(),
            std::time::Duration::from_secs(10 * 60),
        );
        Ok(merged)
    }

    /// Project metadata plus long-form Markdown body (for in-launcher pages).
    /// Always hits the network for the body; still refreshes the ProjectInfo cache.
    pub fn get_project_with_body(
        &self,
        id: &str,
    ) -> Result<(ProjectInfo, Option<String>), ProviderError> {
        let project: ModrinthProject = self.get_json(&format!("/project/{id}"))?;
        let body = project
            .body
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let info: ProjectInfo = project.into();
        let key = crate::api_cache::project_key("modrinth", id);
        crate::api_cache::put(key, info.clone());
        Ok((info, body))
    }

    /// Batch project metadata (`GET /v2/projects?ids=["a","b"]`), chunked so
    /// large packs (an FO set is ~100+ mods) resolve in a few requests.
    /// Accepts ids or slugs; unknown ones are silently absent from the result.
    pub fn get_projects_batch(&self, ids: &[String]) -> Result<Vec<ProjectInfo>, ProviderError> {
        const CHUNK: usize = 100;
        let mut out = Vec::new();
        for chunk in ids.chunks(CHUNK) {
            if chunk.is_empty() {
                continue;
            }
            let arr = serde_json::to_string(chunk).map_err(ProviderError::Parse)?;
            let path = format!("/projects?ids={}", urlencode(&arr));
            let projects: Vec<ModrinthProject> = self.get_json(&path)?;
            out.extend(projects.into_iter().map(Into::into));
        }
        Ok(out)
    }

    /// Full project detail for the in-launcher catalog page: everything
    /// [`Self::get_project_with_body`] returns plus gallery images, loaders,
    /// game-version lines, external links (discord / wiki / donate) and the
    /// team member list. Two requests total (project + team members), both
    /// team/gallery/links failures degrade gracefully instead of failing the
    /// whole page.
    pub fn get_project_detail(&self, id: &str) -> Result<ProjectDetail, ProviderError> {
        let project: ModrinthProjectFull = self.get_json(&format!("/project/{id}"))?;
        let body = project
            .project
            .body
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let info: ProjectInfo = project.project.clone().into();
        let key = crate::api_cache::project_key("modrinth", id);
        crate::api_cache::put(key, info);

        // Team members: a project id is also a valid team id on Modrinth
        // (`GET /project/{id}/members` proxies to the team endpoint).
        let creators: Vec<ProjectCreator> = self
            .get_json::<Vec<ModrinthTeamMember>>(&format!("/project/{}", project.project.id))
            .map(|members| {
                // Modrinth ordering: Owner first, then by ordering/join date.
                // The API usually returns members pre-sorted; sorting again
                // keeps the owner-first contract even if the response isn't.
                let mut members = members;
                members.sort_by_key(|m| m.ordering);
                members
                    .into_iter()
                    .filter(|m| m.user.username.is_some())
                    .map(Into::into)
                    .collect()
            })
            .unwrap_or_default();

        Ok(ProjectDetail {
            project: project.project.into(),
            body,
            gallery: project.gallery.unwrap_or_default(),
            loaders: project.loaders.unwrap_or_default(),
            game_versions: project.game_versions.unwrap_or_default(),
            discord_url: project.discord_url.filter(|s| !s.trim().is_empty()),
            wiki_url: project.wiki_url.filter(|s| !s.trim().is_empty()),
            donate_url: project
                .donation_urls
                .unwrap_or_default()
                .into_iter()
                .find_map(|d| d.url),
            creators,
        })
    }

    /// Best-effort reverse deps via search facet `required_dependencies:{id}*`.
    /// Returns empty if the facet is unsupported / request fails (not blocking).
    pub fn search_dependents(&self, project_id: &str, limit: u32) -> Vec<ProjectInfo> {
        let id = project_id.trim();
        if id.is_empty() {
            return Vec::new();
        }
        let limit = limit.clamp(1, 20);
        let facets = serde_json::to_string(&vec![vec![format!("required_dependencies:{id}*")]])
            .unwrap_or_default();
        let path = format!(
            "/search?index=downloads&limit={limit}&facets={}",
            urlencode(&facets)
        );
        match self.get_json::<ModrinthSearchResponse>(&path) {
            Ok(resp) => resp.hits.into_iter().map(Into::into).collect(),
            Err(_) => Vec::new(),
        }
    }
}

/// Render Modrinth Markdown body to HTML for the catalog project page.
pub fn markdown_to_html(md: &str) -> String {
    use pulldown_cmark::{html, Options, Parser};
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_HEADING_ATTRIBUTES);
    let parser = Parser::new_ext(md, options);
    let mut out = String::new();
    html::push_html(&mut out, parser);
    out
}

impl ContentProvider for ModrinthProvider {
    fn search(&self, query: &ProviderSearchQuery) -> Result<SearchPage, ProviderError> {
        let index = query.sort.as_deref().unwrap_or("relevance");
        let limit = query.limit.unwrap_or(24).clamp(1, 100);
        let offset = query.offset.unwrap_or(0);
        let mut path = format!(
            "/search?index={}&limit={}&offset={}",
            urlencode(index),
            limit,
            offset
        );
        if let Some(q) = &query.query {
            if !q.trim().is_empty() {
                path.push_str(&format!("&query={}", urlencode(q.trim())));
            }
        }
        let facets = build_facets(query);
        if !facets.is_empty() {
            path.push_str(&format!("&facets={}", urlencode(&facets)));
        }

        let response: ModrinthSearchResponse = self.get_json(&path)?;
        Ok(SearchPage {
            results: response.hits.into_iter().map(Into::into).collect(),
            total: response.total_hits,
        })
    }

    fn get_project(&self, id: &str) -> Result<ProjectInfo, ProviderError> {
        let key = crate::api_cache::project_key("modrinth", id);
        if let Some(cached) = crate::api_cache::get::<ProjectInfo>(&key) {
            return Ok(cached);
        }
        let (info, _) = self.get_project_with_body(id)?;
        Ok(info)
    }

    fn get_version(&self, version_id: &str) -> Result<VersionInfo, ProviderError> {
        let key = crate::api_cache::version_key("modrinth", version_id);
        if let Some(cached) = crate::api_cache::get::<VersionInfo>(&key) {
            return Ok(cached);
        }
        let version: ModrinthVersion = self.get_json(&format!("/version/{version_id}"))?;
        let info: VersionInfo = version.into();
        crate::api_cache::put(key, info.clone());
        Ok(info)
    }

    fn get_versions(
        &self,
        id: &str,
        query: &ProviderSearchQuery,
    ) -> Result<Vec<VersionInfo>, ProviderError> {
        let mut path = format!("/project/{id}/version");
        let mut params = Vec::new();
        if let Some(loader) = &query.loader {
            params.push(format!(
                "loaders={}",
                urlencode(&serde_json::to_string(&[loader]).unwrap())
            ));
        }
        if let Some(mc) = &query.minecraft_version {
            params.push(format!(
                "game_versions={}",
                urlencode(&serde_json::to_string(&[mc]).unwrap())
            ));
        }
        if !params.is_empty() {
            path.push('?');
            path.push_str(&params.join("&"));
        }

        let versions: Vec<ModrinthVersion> = self.get_json(&path)?;
        Ok(versions.into_iter().map(Into::into).collect())
    }

    fn get_file(
        &self,
        version_id: &str,
        filename: &str,
    ) -> Result<ProviderFileInfo, ProviderError> {
        let version: ModrinthVersion = self.get_json(&format!("/version/{version_id}"))?;
        version
            .files
            .into_iter()
            .find(|f| f.filename == filename)
            .map(Into::into)
            .ok_or_else(|| ProviderError::VersionNotFound(filename.to_string()))
    }

    fn resolve_dependencies(
        &self,
        version_id: &str,
    ) -> Result<Vec<ModDependencySpec>, ProviderError> {
        let version: ModrinthVersion = self.get_json(&format!("/version/{version_id}"))?;
        let mut dependencies: Vec<ModDependencySpec> = version
            .dependencies
            .into_iter()
            .filter_map(|dep| {
                let raw = ProviderDependency::from(dep);
                if raw.project_id.is_none() {
                    if let Some(version_id) = &raw.version_id {
                        if let Ok(v) = self.get_version(version_id) {
                            return provider_dependency_to_spec(ProviderDependency {
                                project_id: Some(v.project_id),
                                version_id: Some(version_id.clone()),
                                dependency_type: raw.dependency_type,
                            });
                        }
                    }
                    return None;
                }
                provider_dependency_to_spec(raw)
            })
            .collect();

        // Modrinth dependency payloads use immutable project IDs, while TuffBox
        // mod nodes use stable human-readable slugs (`mod:sodium`, `mod:fabric-api`).
        // Normalizing here keeps missing-dependency diagnostics consistent across
        // CLI, desktop UI and imported manifests.
        //
        // If the network is flaky and get_project fails, we keep the raw
        // project_id as the target. The graph builder handles both: slugs
        // match installed mods directly, raw project_ids are resolved via
        // project_id_to_slug. Either way the edge still works.
        for dependency in &mut dependencies {
            if let Ok(project) = self.get_project(&dependency.target) {
                dependency.target = project.slug;
            }
        }

        Ok(dependencies)
    }
}

fn build_facets(query: &ProviderSearchQuery) -> String {
    let mut facets: Vec<Vec<String>> = Vec::new();
    let project_type = query.project_type.as_deref().unwrap_or("mod");
    facets.push(vec![format!("project_type:{project_type}")]);

    if let Some(mc) = &query.minecraft_version {
        facets.push(vec![format!("versions:{mc}")]);
    }
    // The loader facet only makes sense for loader-bound content (mods,
    // modpacks, plugins). Resourcepacks/datapacks/shaders aren't tied to a
    // mod loader on Modrinth, so applying it there would silently zero out
    // every result.
    if matches!(project_type, "mod" | "modpack" | "plugin") {
        if let Some(loader) = &query.loader {
            if !loader.trim().is_empty() {
                facets.push(vec![format!("categories:{}", loader.trim().to_lowercase())]);
            }
        }
    }
    if let Some(category) = &query.category {
        if !category.trim().is_empty() {
            facets.push(vec![format!(
                "categories:{}",
                category.trim().to_lowercase().replace(' ', "-")
            )]);
        }
    }
    if let Some(environment) = &query.environment {
        if !environment.trim().is_empty() {
            facets.push(vec![format!(
                "{}_side:required",
                environment.trim().to_lowercase()
            )]);
        }
    }
    if query.license.as_deref() == Some("open-source") {
        facets.push(vec!["open_source:true".to_string()]);
    }
    if facets.is_empty() {
        return String::new();
    }
    serde_json::to_string(&facets).unwrap_or_default()
}

pub fn urlencode(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace(' ', "%20")
        .replace('[', "%5B")
        .replace(']', "%5D")
        .replace('"', "%22")
        .replace(':', "%3A")
        .replace(',', "%2C")
        .replace('&', "%26")
        .replace('+', "%2B")
        .replace('#', "%23")
        .replace('=', "%3D")
        .replace('?', "%3F")
        .replace('/', "%2F")
        .replace('@', "%40")
}

/// Category tag from Modrinth `GET /v2/tag/category`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModrinthCategory {
    pub name: String,
    /// Modrinth API uses `project_type`; IPC to UI uses `projectType`.
    #[serde(alias = "project_type")]
    pub project_type: String,
    #[serde(default)]
    pub header: String,
    #[serde(default)]
    pub icon: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ModrinthSearchResponse {
    hits: Vec<ModrinthSearchHit>,
    #[serde(default)]
    total_hits: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
struct ModrinthSearchHit {
    project_id: String,
    slug: String,
    title: String,
    description: String,
    project_type: String,
    icon_url: Option<String>,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    downloads: Option<u64>,
    #[serde(default)]
    follows: Option<u64>,
    #[serde(default)]
    date_modified: Option<String>,
    #[serde(default)]
    categories: Vec<String>,
    #[serde(default, deserialize_with = "string_or_object")]
    license: Option<String>,
    #[serde(default, deserialize_with = "string_or_object")]
    client_side: Option<String>,
    #[serde(default, deserialize_with = "string_or_object")]
    server_side: Option<String>,
}

impl From<ModrinthSearchHit> for ProjectInfo {
    fn from(hit: ModrinthSearchHit) -> Self {
        Self {
            id: hit.project_id,
            slug: hit.slug,
            name: hit.title,
            description: hit.description,
            project_type: hit.project_type,
            icon_url: hit.icon_url,
            author: hit.author,
            downloads: hit.downloads,
            follows: hit.follows,
            date_modified: hit.date_modified,
            categories: hit.categories,
            license: hit.license,
            client_side: hit.client_side,
            server_side: hit.server_side,
            issues_url: None,
            source_url: None,
            date_created: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
struct ModrinthProject {
    id: String,
    slug: String,
    title: String,
    description: String,
    project_type: String,
    icon_url: Option<String>,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    downloads: Option<u64>,
    /// Search hits use `follows`; GET /project uses `followers`.
    #[serde(default, alias = "followers")]
    follows: Option<u64>,
    /// Search hits use `date_modified`; GET /project uses `updated`.
    #[serde(default, alias = "updated")]
    date_modified: Option<String>,
    /// Project creation timestamp (`published` on GET /project).
    #[serde(default)]
    published: Option<String>,
    #[serde(default)]
    categories: Vec<String>,
    /// Secondary Modrinth tags (merged into categories for graph clustering).
    #[serde(default)]
    additional_categories: Vec<String>,
    #[serde(default, deserialize_with = "string_or_object")]
    license: Option<String>,
    #[serde(default, deserialize_with = "string_or_object")]
    client_side: Option<String>,
    #[serde(default, deserialize_with = "string_or_object")]
    server_side: Option<String>,
    /// Long-form project page (Markdown). Search hits omit this; GET /project includes it.
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    issues_url: Option<String>,
    #[serde(default)]
    source_url: Option<String>,
}

impl From<ModrinthProject> for ProjectInfo {
    fn from(project: ModrinthProject) -> Self {
        let mut categories = project.categories;
        for extra in project.additional_categories {
            if !categories.iter().any(|c| c.eq_ignore_ascii_case(&extra)) {
                categories.push(extra);
            }
        }
        Self {
            id: project.id,
            slug: project.slug,
            name: project.title,
            description: project.description,
            project_type: project.project_type,
            icon_url: project.icon_url,
            author: project.author,
            downloads: project.downloads,
            follows: project.follows,
            date_modified: project.date_modified,
            categories,
            license: project.license,
            client_side: project.client_side,
            server_side: project.server_side,
            issues_url: project.issues_url,
            source_url: project.source_url,
            date_created: project.published,
        }
    }
}

/// Full Modrinth project payload (`GET /v2/project/{id}`) — everything the
/// detail page needs in one request. Reuses the base project via `flatten`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
struct ModrinthProjectFull {
    #[serde(flatten)]
    project: ModrinthProject,
    #[serde(default)]
    loaders: Option<Vec<String>>,
    #[serde(default)]
    game_versions: Option<Vec<String>>,
    #[serde(default)]
    gallery: Option<Vec<ProjectGalleryImage>>,
    #[serde(default)]
    discord_url: Option<String>,
    #[serde(default)]
    wiki_url: Option<String>,
    #[serde(default)]
    donation_urls: Option<Vec<ModrinthDonationUrl>>,
}

/// One entry of a project gallery (`gallery[]` on the project payload).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectGalleryImage {
    pub url: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModrinthDonationUrl {
    #[serde(default)]
    url: Option<String>,
}

/// Team member of a Modrinth project (`GET /v2/project/{id}/members`).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
struct ModrinthTeamMember {
    #[serde(default)]
    user: ModrinthTeamUser,
    #[serde(default)]
    role: Option<String>,
    /// Modrinth ordering: Owner first, then by `ordering`/join date.
    #[serde(default)]
    ordering: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
struct ModrinthTeamUser {
    username: Option<String>,
    #[serde(default)]
    avatar_url: Option<String>,
}

/// Creator shown on the catalog sidebar (username + role + avatar).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectCreator {
    pub username: String,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub avatar_url: Option<String>,
}

impl From<ModrinthTeamMember> for ProjectCreator {
    fn from(m: ModrinthTeamMember) -> Self {
        Self {
            username: m.user.username.unwrap_or_default(),
            role: m.role,
            avatar_url: m.user.avatar_url,
        }
    }
}

/// Detail payload for the in-launcher catalog page (Modrinth side).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDetail {
    pub project: ProjectInfo,
    /// Long-form Markdown body, pre-trimmed.
    pub body: Option<String>,
    pub gallery: Vec<ProjectGalleryImage>,
    pub loaders: Vec<String>,
    pub game_versions: Vec<String>,
    pub discord_url: Option<String>,
    pub wiki_url: Option<String>,
    pub donate_url: Option<String>,
    pub creators: Vec<ProjectCreator>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
struct ModrinthVersion {
    id: String,
    project_id: String,
    version_number: String,
    game_versions: Vec<String>,
    loaders: Vec<String>,
    files: Vec<ModrinthFile>,
    dependencies: Vec<ModrinthDependency>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    changelog: Option<String>,
    #[serde(default)]
    date_published: Option<String>,
    #[serde(default)]
    version_type: Option<String>,
}

impl From<ModrinthVersion> for VersionInfo {
    fn from(version: ModrinthVersion) -> Self {
        Self {
            id: version.id,
            project_id: version.project_id,
            version_number: version.version_number,
            game_versions: version.game_versions,
            loaders: version.loaders,
            files: version.files.into_iter().map(Into::into).collect(),
            dependencies: version
                .dependencies
                .into_iter()
                .map(ProviderDependency::from)
                .collect(),
            name: version.name,
            changelog: version.changelog,
            date_published: version.date_published,
            version_type: version.version_type,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
struct ModrinthFile {
    url: String,
    filename: String,
    primary: bool,
    hashes: ModrinthFileHashes,
}

impl From<ModrinthFile> for ProviderFileInfo {
    fn from(file: ModrinthFile) -> Self {
        Self {
            url: file.url,
            filename: file.filename,
            primary: file.primary,
            hashes: ProviderFileHashes {
                sha1: file.hashes.sha1,
                sha512: file.hashes.sha512,
            },
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
struct ModrinthFileHashes {
    sha1: Option<String>,
    sha512: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
struct ModrinthDependency {
    project_id: Option<String>,
    version_id: Option<String>,
    dependency_type: String,
}

impl From<ModrinthDependency> for ProviderDependency {
    fn from(dep: ModrinthDependency) -> Self {
        Self {
            project_id: dep.project_id,
            version_id: dep.version_id,
            dependency_type: dep.dependency_type,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires network access to Modrinth"]
    fn searches_sodium_for_fabric_1_20_1() {
        let provider = ModrinthProvider::new();
        let results = provider
            .search(&ProviderSearchQuery {
                query: Some("sodium".to_string()),
                minecraft_version: Some("1.20.1".to_string()),
                loader: Some("fabric".to_string()),
                ..Default::default()
            })
            .unwrap();
        assert!(!results.results.is_empty());
        assert!(results.results.iter().any(|p| p.slug == "sodium"));
    }
}
