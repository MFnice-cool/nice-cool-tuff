//! Import packwiz packs (`pack.toml` + `index.toml` + `.pw.toml` metafiles).
//!
//! No packwiz binary — parses the on-disk TOML format into [`ProjectManifest`].

use crate::manifest::{
    ContentType, FileHashes, JavaSpec, LoaderKind, LoaderSpec, MinecraftSpec, ModOption, ModSource,
    ModSpec, ProfileSpec, ProjectManifest, ProjectMetadata, Side, SourceKind,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PackwizImportError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("toml error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("missing pack.toml")]
    MissingPackToml,
    #[error("missing index file: {0}")]
    MissingIndex(String),
    #[error("unsupported loader in pack.toml versions")]
    UnknownLoader,
}

#[derive(Debug, Error)]
pub enum PackwizExportError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("output path must be a directory (got a file): {0}")]
    NotADirectory(String),
    #[error("mod {0} is downloadable but has no sha1/sha512 hash")]
    MissingHash(String),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackwizExportResult {
    pub path: PathBuf,
    pub file_count: usize,
    pub override_count: usize,
    /// Non-fatal problems, same contract as `exporter::ExportResult::warnings`.
    /// The packwiz exporter currently hard-fails on unreadable override files
    /// (io error aborts the export), so this is empty today; the field exists
    /// so callers can map the result onto `ExportResult` uniformly.
    pub warnings: Vec<String>,
}

/// True when `path` is a directory containing `pack.toml`.
pub fn is_packwiz_pack(path: impl AsRef<Path>) -> bool {
    path.as_ref().join("pack.toml").is_file()
}

/// Export a TuffBox project as a packwiz directory (`pack.toml` + `index.toml` + metafiles).
///
/// Does not copy Modrinth/CurseForge jar binaries — those are written as `.pw.toml`
/// metafiles. Local/custom files are copied when present.
pub fn export_packwiz_pack(
    manifest: &ProjectManifest,
    manifest_path: impl AsRef<Path>,
    output_dir: impl AsRef<Path>,
) -> Result<PackwizExportResult, PackwizExportError> {
    let output_dir = output_dir.as_ref();
    let project_dir = manifest_path
        .as_ref()
        .parent()
        .ok_or_else(|| PackwizExportError::NotADirectory("manifest has no parent".into()))?;
    if output_dir.is_file() {
        return Err(PackwizExportError::NotADirectory(
            output_dir.display().to_string(),
        ));
    }
    if same_dir(output_dir, project_dir) {
        return Err(PackwizExportError::NotADirectory(
            "packwiz output must not be the project root".into(),
        ));
    }
    fs::create_dir_all(output_dir)?;

    let mut versions: BTreeMap<String, String> = BTreeMap::new();
    versions.insert("minecraft".into(), manifest.minecraft.version.clone());
    let loader_key = match manifest.loader.kind {
        LoaderKind::Fabric => "fabric",
        LoaderKind::Quilt => "quilt",
        LoaderKind::Forge => "forge",
        LoaderKind::Neoforge => "neoforge",
        LoaderKind::Vanilla => "vanilla",
    };
    if !matches!(manifest.loader.kind, LoaderKind::Vanilla) {
        versions.insert(loader_key.into(), manifest.loader.version.clone());
    }

    let mut index_entries: Vec<(String, String, bool)> = Vec::new();
    let mut override_count = 0usize;

    for m in &manifest.mods {
        let folder = m.content_type.folder_name();
        if mod_has_packwiz_remote(m) {
            if !mod_has_usable_hash(m) {
                return Err(PackwizExportError::MissingHash(m.id.clone()));
            }
            let meta_rel = format!("{folder}/{}.pw.toml", sanitize_filename(&m.id));
            let meta_path = output_dir.join(&meta_rel);
            if let Some(parent) = meta_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let body = render_pw_toml(m);
            fs::write(&meta_path, &body)?;
            let hash = hex::encode(Sha256::digest(body.as_bytes()));
            index_entries.push((meta_rel.replace('\\', "/"), hash, true));
        } else if let Some(file_name) = m.file_name.as_ref() {
            let src = project_dir.join(folder).join(file_name);
            if src.is_file() {
                let rel = format!("{folder}/{}", sanitize_filename(file_name));
                let dest = output_dir.join(&rel);
                if let Some(parent) = dest.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(&src, &dest)?;
                let bytes = fs::read(&dest)?;
                let hash = hex::encode(Sha256::digest(&bytes));
                index_entries.push((rel.replace('\\', "/"), hash, false));
            }
        }
    }

    for root in PACKWIZ_OVERRIDE_ROOTS {
        let src_root = project_dir.join(root);
        if !src_root.is_dir() {
            continue;
        }
        override_count += copy_tree_hashed(project_dir, &src_root, output_dir, &mut index_entries)?;
    }

    index_entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut index_toml = String::from("hash-format = \"sha256\"\n\n");
    for (file, hash, metafile) in &index_entries {
        index_toml.push_str("[[files]]\n");
        index_toml.push_str(&format!("file = {}\n", toml_string(file)));
        index_toml.push_str(&format!("hash = {}\n", toml_string(hash)));
        if *metafile {
            index_toml.push_str("metafile = true\n");
        }
        index_toml.push('\n');
    }
    fs::write(output_dir.join("index.toml"), &index_toml)?;
    let index_hash = hex::encode(Sha256::digest(index_toml.as_bytes()));

    let author = manifest.project.authors.first().cloned();
    let mut pack_toml = String::new();
    pack_toml.push_str(&format!("name = {}\n", toml_string(&manifest.project.name)));
    if let Some(a) = &author {
        pack_toml.push_str(&format!("author = {}\n", toml_string(a)));
    }
    pack_toml.push_str(&format!(
        "version = {}\n",
        toml_string(&manifest.project.version)
    ));
    if let Some(desc) = &manifest.project.description {
        if !desc.trim().is_empty() {
            pack_toml.push_str(&format!("description = {}\n", toml_string(desc)));
        }
    }
    pack_toml.push_str("pack-format = \"packwiz:1.1.0\"\n\n");
    pack_toml.push_str("[index]\n");
    pack_toml.push_str("file = \"index.toml\"\n");
    pack_toml.push_str("hash-format = \"sha256\"\n");
    pack_toml.push_str(&format!("hash = {}\n\n", toml_string(&index_hash)));
    pack_toml.push_str("[versions]\n");
    for (k, v) in &versions {
        pack_toml.push_str(&format!("{k} = {}\n", toml_string(v)));
    }
    fs::write(output_dir.join("pack.toml"), pack_toml)?;

    Ok(PackwizExportResult {
        path: output_dir.to_path_buf(),
        file_count: index_entries.len() + 2,
        override_count,
        // Hard-fail exporter: unreadable files abort with an error, so there
        // are never soft skips to report.
        warnings: Vec::new(),
    })
}

const PACKWIZ_OVERRIDE_ROOTS: &[&str] = &[
    "config",
    "defaultconfigs",
    "kubejs",
    "scripts",
    "resourcepacks",
    "shaderpacks",
    "datapacks",
];

fn same_dir(a: &Path, b: &Path) -> bool {
    let a = fs::canonicalize(a).unwrap_or_else(|_| a.to_path_buf());
    let b = fs::canonicalize(b).unwrap_or_else(|_| b.to_path_buf());
    a == b
}

fn mod_has_usable_hash(m: &ModSpec) -> bool {
    m.hashes.as_ref().is_some_and(|h| {
        h.sha1.as_deref().is_some_and(|s| !s.is_empty())
            || h.sha512.as_deref().is_some_and(|s| !s.is_empty())
    })
}

fn mod_has_packwiz_remote(m: &ModSpec) -> bool {
    match m.source.kind {
        SourceKind::Curseforge => {
            m.source
                .project_id
                .as_deref()
                .is_some_and(|s| !s.is_empty())
                && m.source.file_id.as_deref().is_some_and(|s| !s.is_empty())
        }
        _ => m
            .source
            .url
            .as_deref()
            .is_some_and(|u| !u.trim().is_empty()),
    }
}

fn copy_tree_hashed(
    project_dir: &Path,
    dir: &Path,
    output_dir: &Path,
    index_entries: &mut Vec<(String, String, bool)>,
) -> Result<usize, PackwizExportError> {
    let mut count = 0usize;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_symlink() {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            count += copy_tree_hashed(project_dir, &path, output_dir, index_entries)?;
        } else if path.is_file() {
            let rel = path
                .strip_prefix(project_dir)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let dest = output_dir.join(&rel);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&path, &dest)?;
            let bytes = fs::read(&dest)?;
            let hash = hex::encode(Sha256::digest(&bytes));
            index_entries.push((rel, hash, false));
            count += 1;
        }
    }
    Ok(count)
}

fn toml_string(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn sanitize_filename(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn render_pw_toml(m: &ModSpec) -> String {
    let filename = m
        .file_name
        .clone()
        .unwrap_or_else(|| format!("{}.jar", m.id));
    let side = match m.side {
        Side::Client => "client",
        Side::Server => "server",
        Side::Both | Side::Optional | Side::Unknown => "both",
    };
    let mut out = String::new();
    out.push_str(&format!("name = {}\n", toml_string(&m.name)));
    out.push_str(&format!("filename = {}\n", toml_string(&filename)));
    out.push_str(&format!("side = {}\n", toml_string(side)));
    if m.pinned() {
        out.push_str("pin = true\n");
    }
    out.push('\n');

    out.push_str("[download]\n");
    let (hash_format, hash) = match &m.hashes {
        Some(h) if h.sha512.as_ref().is_some_and(|s| !s.is_empty()) => {
            ("sha512", h.sha512.clone().unwrap())
        }
        Some(h) if h.sha1.as_ref().is_some_and(|s| !s.is_empty()) => {
            ("sha1", h.sha1.clone().unwrap())
        }
        _ => ("sha256", String::new()),
    };
    if !hash.is_empty() {
        out.push_str(&format!("hash-format = {}\n", toml_string(hash_format)));
        out.push_str(&format!("hash = {}\n", toml_string(&hash)));
    }

    match m.source.kind {
        SourceKind::Curseforge => {
            out.push_str("mode = \"metadata:curseforge\"\n");
        }
        _ => {
            if let Some(url) = &m.source.url {
                if !url.is_empty() {
                    out.push_str(&format!("url = {}\n", toml_string(url)));
                }
            }
        }
    }
    out.push('\n');

    match m.source.kind {
        SourceKind::Modrinth => {
            if let Some(pid) = &m.source.project_id {
                out.push_str("[update.modrinth]\n");
                out.push_str(&format!("mod-id = {}\n", toml_string(pid)));
                if !m.version.is_empty() && m.version != "unknown" {
                    out.push_str(&format!("version = {}\n", toml_string(&m.version)));
                }
                out.push('\n');
            }
        }
        SourceKind::Curseforge => {
            if let Some(pid) = &m.source.project_id {
                out.push_str("[update.curseforge]\n");
                out.push_str(&format!("project-id = {}\n", pid));
                if let Some(fid) = &m.source.file_id {
                    out.push_str(&format!("file-id = {}\n", fid));
                }
                out.push('\n');
            }
        }
        SourceKind::Github => {
            if let Some(slug) = &m.source.project_id {
                out.push_str("[update.github]\n");
                out.push_str(&format!("slug = {}\n", toml_string(slug)));
                if let Some((owner, repo)) = slug.split_once('/') {
                    out.push_str(&format!("owner = {}\n", toml_string(owner)));
                    out.push_str(&format!("repo = {}\n", toml_string(repo)));
                }
                if let Some(tag) = &m.source.file_id {
                    if !tag.is_empty() {
                        out.push_str(&format!("tag = {}\n", toml_string(tag)));
                    }
                }
                out.push('\n');
            }
        }
        _ => {}
    }

    if m.side == Side::Optional || m.option.is_some() {
        out.push_str("[option]\n");
        out.push_str("optional = true\n");
        if let Some(opt) = &m.option {
            if let Some(desc) = &opt.description {
                out.push_str(&format!("description = {}\n", toml_string(desc)));
            }
            out.push_str(&format!(
                "default = {}\n",
                if opt.default { "true" } else { "false" }
            ));
        } else {
            out.push_str("default = true\n");
        }
        out.push('\n');
    }

    out
}

#[derive(Debug, Deserialize)]
struct PackToml {
    name: String,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default, rename = "pack-format")]
    pack_format: Option<String>,
    index: PackIndexRef,
    #[serde(default)]
    versions: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct PackIndexRef {
    file: String,
}

#[derive(Debug, Deserialize)]
struct IndexToml {
    #[serde(default)]
    files: Vec<IndexFile>,
}

#[derive(Debug, Deserialize)]
struct IndexFile {
    file: String,
    #[serde(default)]
    metafile: bool,
}

#[derive(Debug, Deserialize)]
struct PwToml {
    name: String,
    filename: String,
    #[serde(default)]
    side: Option<String>,
    #[serde(default)]
    pin: bool,
    #[serde(default)]
    download: Option<PwDownload>,
    #[serde(default)]
    update: Option<PwUpdate>,
    #[serde(default)]
    option: Option<PwOption>,
}

#[derive(Debug, Deserialize)]
struct PwDownload {
    #[serde(default)]
    url: Option<String>,
    #[serde(default, rename = "hash-format")]
    hash_format: Option<String>,
    #[serde(default)]
    hash: Option<String>,
    #[serde(default)]
    mode: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct PwUpdate {
    #[serde(default)]
    modrinth: Option<PwModrinthUpdate>,
    #[serde(default)]
    curseforge: Option<PwCurseforgeUpdate>,
    #[serde(default)]
    github: Option<PwGithubUpdate>,
}

#[derive(Debug, Deserialize)]
struct PwModrinthUpdate {
    #[serde(rename = "mod-id")]
    mod_id: String,
    #[serde(default)]
    version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PwCurseforgeUpdate {
    #[serde(rename = "project-id")]
    project_id: u64,
    #[serde(default, rename = "file-id")]
    file_id: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct PwGithubUpdate {
    #[serde(default)]
    slug: Option<String>, // "owner/repo"
    #[serde(default)]
    owner: Option<String>,
    #[serde(default)]
    repo: Option<String>,
    #[serde(default)]
    tag: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PwOption {
    #[serde(default)]
    optional: bool,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    default: Option<bool>,
}

/// Import a packwiz pack directory into a [`ProjectManifest`].
pub fn import_packwiz_pack(path: impl AsRef<Path>) -> Result<ProjectManifest, PackwizImportError> {
    let root = path.as_ref();
    let pack_path = root.join("pack.toml");
    if !pack_path.is_file() {
        return Err(PackwizImportError::MissingPackToml);
    }
    let pack: PackToml = toml::from_str(&fs::read_to_string(&pack_path)?)?;
    let _ = pack.pack_format; // accepted for forward-compat; unused

    let index_path = root.join(&pack.index.file);
    if !index_path.is_file() {
        return Err(PackwizImportError::MissingIndex(pack.index.file.clone()));
    }
    let index: IndexToml = toml::from_str(&fs::read_to_string(&index_path)?)?;

    let (loader_kind, loader_version) = detect_loader(&pack.versions)?;
    let mc_version = pack.versions.get("minecraft").cloned().unwrap_or_default();

    let mut mods = Vec::new();
    for entry in &index.files {
        if !entry.metafile {
            continue;
        }
        let meta_path = index_path.parent().unwrap_or(root).join(&entry.file);
        if !meta_path.is_file() {
            continue;
        }
        let pw: PwToml = match toml::from_str(&fs::read_to_string(&meta_path)?) {
            Ok(v) => v,
            Err(_) => continue,
        };
        mods.push(pw_to_modspec(&pw));
    }

    let project_id = slugify(&pack.name);
    Ok(ProjectManifest {
        schema_version: crate::manifest::CURRENT_PROJECT_SCHEMA_VERSION.into(),
        project: ProjectMetadata {
            id: project_id,
            name: pack.name,
            version: pack.version.unwrap_or_else(|| "1.0.0".into()),
            description: pack.description,
            authors: pack.author.map(|a| vec![a]).unwrap_or_default(),
        },
        minecraft: MinecraftSpec {
            version: mc_version,
        },
        loader: LoaderSpec {
            kind: loader_kind,
            version: loader_version,
        },
        brief: None,
        listing: None,
        java: Some(JavaSpec {
            major: Some(17),
            distribution: None,
            path: None,
        }),
        profiles: vec![
            ProfileSpec {
                id: "client".into(),
                name: "Client".into(),
                side: Side::Client,
                include_optional_mods: true,
                include_shaders: true,
                memory_mb: Some(4096),
                jvm_args: vec!["-XX:+UseG1GC".into()],
                include_mods: Vec::new(),
                player_name: Some("Player".into()),
            },
            ProfileSpec {
                id: "server".into(),
                name: "Server".into(),
                side: Side::Server,
                include_optional_mods: false,
                include_shaders: false,
                memory_mb: Some(4096),
                jvm_args: vec!["-XX:+UseG1GC".into()],
                include_mods: Vec::new(),
                player_name: None,
            },
        ],
        mods,
        overrides: None,
    })
}

fn detect_loader(
    versions: &HashMap<String, String>,
) -> Result<(LoaderKind, String), PackwizImportError> {
    for (key, kind) in [
        ("fabric", LoaderKind::Fabric),
        ("quilt", LoaderKind::Quilt),
        ("neoforge", LoaderKind::Neoforge),
        ("forge", LoaderKind::Forge),
    ] {
        if let Some(ver) = versions.get(key) {
            return Ok((kind, ver.clone()));
        }
    }
    if versions.contains_key("minecraft") {
        return Ok((LoaderKind::Vanilla, "none".into()));
    }
    Err(PackwizImportError::UnknownLoader)
}

fn pw_to_modspec(pw: &PwToml) -> ModSpec {
    let optional = pw.option.as_ref().map(|o| o.optional).unwrap_or(false);
    let side = if optional {
        Side::Optional
    } else {
        match pw.side.as_deref().unwrap_or("both") {
            "client" => Side::Client,
            "server" => Side::Server,
            _ => Side::Both,
        }
    };

    let (kind, project_id, file_id) = resolve_source(pw);
    let url = pw.download.as_ref().and_then(|d| d.url.clone());
    let hashes = pw.download.as_ref().and_then(|d| {
        let hash = d.hash.clone()?;
        match d.hash_format.as_deref() {
            Some("sha1") => Some(FileHashes {
                sha1: Some(hash),
                sha512: None,
            }),
            Some("sha512") => Some(FileHashes {
                sha1: None,
                sha512: Some(hash),
            }),
            _ => Some(FileHashes {
                sha1: None,
                sha512: Some(hash),
            }),
        }
    });

    let mut status = vec!["imported-packwiz".into()];
    if pw.pin {
        status.push("pinned".into());
    }

    let option = pw.option.as_ref().map(|o| ModOption {
        description: o.description.clone(),
        default: o.default.unwrap_or(true),
    });

    let id = slugify(&pw.name);
    ModSpec {
        id,
        name: pw.name.clone(),
        source: ModSource {
            kind,
            project_id,
            file_id,
            url,
            path: None,
            icon_url: None,
            categories: Vec::new(),
        },
        version: pw
            .update
            .as_ref()
            .and_then(|u| u.modrinth.as_ref())
            .and_then(|m| m.version.clone())
            .unwrap_or_else(|| "unknown".into()),
        file_name: Some(pw.filename.clone()),
        hashes,
        side,
        dependencies: vec![],
        status,
        content_type: ContentType::from_filename(&pw.filename),
        authors: Vec::new(),
        option,
    }
}

fn resolve_source(pw: &PwToml) -> (SourceKind, Option<String>, Option<String>) {
    if let Some(update) = &pw.update {
        if let Some(mr) = &update.modrinth {
            return (
                SourceKind::Modrinth,
                Some(mr.mod_id.clone()),
                mr.version.clone(),
            );
        }
        if let Some(cf) = &update.curseforge {
            return (
                SourceKind::Curseforge,
                Some(cf.project_id.to_string()),
                cf.file_id.map(|id| id.to_string()),
            );
        }
        if let Some(gh) = &update.github {
            let slug = gh.slug.clone().or_else(|| match (&gh.owner, &gh.repo) {
                (Some(o), Some(r)) => Some(format!("{o}/{r}")),
                _ => None,
            });
            return (SourceKind::Github, slug, gh.tag.clone());
        }
    }
    if pw
        .download
        .as_ref()
        .and_then(|d| d.mode.as_deref())
        .is_some_and(|m| m.contains("curseforge"))
    {
        return (SourceKind::Curseforge, None, None);
    }
    if pw.download.as_ref().and_then(|d| d.url.as_ref()).is_some() {
        return (SourceKind::Direct, None, None);
    }
    (SourceKind::Local, None, None)
}

fn slugify(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn import_minimal_packwiz_pack() {
        let dir = std::env::temp_dir().join("tuffbox_packwiz_import_test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("mods")).unwrap();

        fs::write(
            dir.join("pack.toml"),
            r#"
name = "Demo Pack"
author = "Tester"
version = "0.1.0"
pack-format = "packwiz:1.1.0"

[index]
file = "index.toml"
hash-format = "sha256"
hash = "00"

[versions]
minecraft = "1.20.1"
fabric = "0.15.0"
"#,
        )
        .unwrap();

        fs::write(
            dir.join("index.toml"),
            r#"
hash-format = "sha256"

[[files]]
file = "mods/sodium.pw.toml"
hash = "00"
metafile = true
"#,
        )
        .unwrap();

        fs::write(
            dir.join("mods/sodium.pw.toml"),
            r#"
name = "Sodium"
filename = "sodium.jar"
side = "client"

[download]
url = "https://example.com/sodium.jar"
hash-format = "sha1"
hash = "deadbeef"

[update.modrinth]
mod-id = "AANobbMI"
version = "mc1.20.1-0.5.8"

[option]
optional = true
description = "Pretty rendering"
default = true
"#,
        )
        .unwrap();

        let manifest = import_packwiz_pack(&dir).unwrap();
        assert_eq!(manifest.project.name, "Demo Pack");
        assert_eq!(manifest.minecraft.version, "1.20.1");
        assert_eq!(manifest.loader.kind, LoaderKind::Fabric);
        assert_eq!(manifest.mods.len(), 1);
        let m = &manifest.mods[0];
        assert_eq!(m.source.kind, SourceKind::Modrinth);
        assert_eq!(m.source.project_id.as_deref(), Some("AANobbMI"));
        assert_eq!(m.side, Side::Optional);
        assert_eq!(
            m.option.as_ref().and_then(|o| o.description.as_deref()),
            Some("Pretty rendering")
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn is_packwiz_detects_pack_toml() {
        let dir = std::env::temp_dir().join("tuffbox_packwiz_detect");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        assert!(!is_packwiz_pack(&dir));
        let mut f = fs::File::create(dir.join("pack.toml")).unwrap();
        writeln!(f, "name = \"x\"").unwrap();
        assert!(is_packwiz_pack(&dir));
        let _ = fs::remove_dir_all(&dir);
    }

    fn github_mod(hashes: Option<FileHashes>) -> ModSpec {
        ModSpec {
            id: "custom-lib".into(),
            name: "Custom Lib".into(),
            source: ModSource {
                kind: SourceKind::Github,
                project_id: Some("owner/custom-lib".into()),
                file_id: Some("v1.0.0".into()),
                url: Some(
                    "https://github.com/owner/custom-lib/releases/download/v1.0.0/custom-lib.jar"
                        .into(),
                ),
                path: None,
                icon_url: None,
                categories: vec![],
            },
            version: "1.0.0".into(),
            file_name: Some("custom-lib.jar".into()),
            hashes,
            side: Side::Both,
            dependencies: vec![],
            status: vec![],
            content_type: ContentType::Mod,
            authors: vec![],
            option: None,
        }
    }

    fn demo_manifest(mods: Vec<ModSpec>) -> ProjectManifest {
        ProjectManifest {
            schema_version: crate::manifest::CURRENT_PROJECT_SCHEMA_VERSION.into(),
            project: ProjectMetadata {
                id: "demo".into(),
                name: "Demo Pack".into(),
                version: "1.2.3".into(),
                description: Some("hi".into()),
                authors: vec!["Tester".into()],
            },
            minecraft: MinecraftSpec {
                version: "1.21.1".into(),
            },
            loader: LoaderSpec {
                kind: LoaderKind::Fabric,
                version: "0.16.0".into(),
            },
            brief: None,
            listing: None,
            java: None,
            profiles: vec![],
            mods,
            overrides: None,
        }
    }

    #[test]
    fn export_emits_github_update_table() {
        let out = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let manifest_path = project.path().join("demo.tuffbox.json");
        fs::write(&manifest_path, "{}").unwrap();
        let manifest = demo_manifest(vec![github_mod(Some(FileHashes {
            sha1: Some("deadbeef".into()),
            sha512: None,
        }))]);

        export_packwiz_pack(&manifest, &manifest_path, out.path()).unwrap();
        let pw = fs::read_to_string(out.path().join("mods/custom-lib.pw.toml")).unwrap();
        assert!(
            pw.contains("[update.github]"),
            "github mods must emit [update.github], got:\n{pw}"
        );
        assert!(
            pw.contains("owner/custom-lib") || (pw.contains("owner") && pw.contains("custom-lib"))
        );
        assert!(pw.contains("v1.0.0") || pw.contains("1.0.0"));
    }

    #[test]
    fn export_rejects_remote_mod_without_hash() {
        let out = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let manifest_path = project.path().join("demo.tuffbox.json");
        fs::write(&manifest_path, "{}").unwrap();
        let manifest = demo_manifest(vec![github_mod(None)]);

        let err = export_packwiz_pack(&manifest, &manifest_path, out.path()).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.to_lowercase().contains("hash"),
            "expected hash rejection, got: {msg}"
        );
    }

    #[test]
    fn github_update_round_trips_through_import() {
        let out = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let manifest_path = project.path().join("demo.tuffbox.json");
        fs::write(&manifest_path, "{}").unwrap();
        let manifest = demo_manifest(vec![github_mod(Some(FileHashes {
            sha1: Some("deadbeef".into()),
            sha512: None,
        }))]);
        export_packwiz_pack(&manifest, &manifest_path, out.path()).unwrap();
        let imported = import_packwiz_pack(out.path()).unwrap();
        let custom = imported
            .mods
            .iter()
            .find(|m| m.id == "custom-lib")
            .expect("custom-lib should round-trip");
        assert_eq!(custom.source.kind, SourceKind::Github);
        assert_eq!(
            custom.source.project_id.as_deref(),
            Some("owner/custom-lib")
        );
        assert_eq!(custom.source.file_id.as_deref(), Some("v1.0.0"));
    }

    #[test]
    fn export_packwiz_roundtrip_smoke() {
        let out = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let manifest_path = project.path().join("demo.tuffbox.json");
        fs::write(&manifest_path, "{}").unwrap();
        fs::create_dir_all(project.path().join("config")).unwrap();
        fs::write(project.path().join("config/demo.toml"), "ok = true\n").unwrap();

        let mut remote_mod = github_mod(Some(FileHashes {
            sha1: None,
            sha512: Some("abc".into()),
        }));
        remote_mod.id = "sodium".into();
        remote_mod.name = "Sodium".into();
        remote_mod.source.kind = SourceKind::Modrinth;
        remote_mod.source.project_id = Some("AANobbMI".into());
        remote_mod.source.file_id = Some("ver1".into());
        remote_mod.source.url = Some("https://example.com/sodium.jar".into());
        remote_mod.version = "0.6.0".into();
        remote_mod.file_name = Some("sodium.jar".into());
        remote_mod.side = Side::Client;

        let manifest = demo_manifest(vec![remote_mod]);
        let result = export_packwiz_pack(&manifest, &manifest_path, out.path()).unwrap();
        assert!(result.path.join("pack.toml").is_file());
        assert!(result.path.join("index.toml").is_file());
        assert!(result.path.join("mods/sodium.pw.toml").is_file());
        assert!(result.path.join("config/demo.toml").is_file());
        assert!(result.override_count >= 1);

        let roundtrip = import_packwiz_pack(out.path()).unwrap();
        assert_eq!(roundtrip.mods.len(), 1);
        assert_eq!(roundtrip.mods[0].source.kind, SourceKind::Modrinth);
    }
}
