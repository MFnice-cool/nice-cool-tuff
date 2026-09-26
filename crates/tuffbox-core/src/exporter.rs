use crate::manifest::{LoaderKind, ProjectManifest, Side, SourceKind};
use crate::provider::CurseForgeProvider;
use serde::Serialize;
use std::{
    collections::HashMap,
    fs,
    io::{Seek, Write},
    path::{Path, PathBuf},
};
use thiserror::Error;
use zip::{write::SimpleFileOptions, ZipWriter};

#[derive(Debug, Error)]
pub enum ExportError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("archive error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("manifest has no parent directory")]
    NoProjectDir,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub path: PathBuf,
    pub file_count: usize,
    pub override_count: usize,
    /// Non-fatal problems: files that could not be read (locked by the
    /// running game / antivirus / permissions) and were SKIPPED. The caller
    /// must surface these — a silently missing file is a broken pack.
    pub warnings: Vec<String>,
}

/// Read a file for archiving with a small retry loop. While the game is
/// running, Windows briefly holds config/log files with a sharing violation
/// (and antivirus scanners do the same to jars); retrying a few times turns
/// a hard export failure into a millisecond delay.
fn read_export_file(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut last_err = None;
    for attempt in 0..3 {
        match fs::read(path) {
            Ok(bytes) => return Ok(bytes),
            Err(e) => {
                // ERROR_SHARING_VIOLATION (32) and PermissionDenied are the
                // transient kinds worth waiting for; anything else fails fast.
                let transient = e.raw_os_error() == Some(32)
                    || matches!(e.kind(), std::io::ErrorKind::PermissionDenied);
                last_err = Some(e);
                if !transient || attempt == 2 {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(150));
            }
        }
    }
    Err(last_err.unwrap_or_else(|| std::io::Error::other("unreadable file")))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportIssue {
    pub severity: ExportIssueSeverity,
    pub code: String,
    pub message: String,
    pub target: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportIssueSeverity {
    Error,
    Warning,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ServerPackManifest {
    name: String,
    version: String,
    minecraft_version: String,
    loader: ServerPackLoader,
    included_mods: Vec<ServerPackMod>,
    remote_mods: Vec<ServerPackRemoteMod>,
    skipped_client_mods: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ServerPackLoader {
    kind: String,
    version: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ServerPackMod {
    id: String,
    name: String,
    version: String,
    file_name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ServerPackRemoteMod {
    id: String,
    name: String,
    version: String,
    file_name: Option<String>,
    url: String,
    sha1: Option<String>,
    sha512: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CurseForgeManifest {
    minecraft: CurseForgeMinecraft,
    manifest_type: String,
    manifest_version: u8,
    name: String,
    version: String,
    author: String,
    files: Vec<CurseForgeFile>,
    overrides: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CurseForgeMinecraft {
    version: String,
    mod_loaders: Vec<CurseForgeLoader>,
}

#[derive(Debug, Serialize)]
struct CurseForgeLoader {
    id: String,
    primary: bool,
}

#[derive(Debug, Serialize)]
struct CurseForgeFile {
    #[serde(rename = "projectID")]
    project_id: u64,
    #[serde(rename = "fileID")]
    file_id: u64,
    required: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PrismPack {
    components: Vec<PrismComponent>,
    format_version: u8,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PrismComponent {
    cached_name: String,
    cached_version: String,
    uid: String,
    version: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModrinthIndex {
    format_version: u8,
    game: String,
    version_id: String,
    name: String,
    summary: Option<String>,
    files: Vec<ModrinthFile>,
    dependencies: HashMap<String, String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModrinthFile {
    path: String,
    hashes: ModrinthHashes,
    downloads: Vec<String>,
    env: HashMap<String, String>,
}

#[derive(Debug, Serialize)]
struct ModrinthHashes {
    #[serde(skip_serializing_if = "Option::is_none")]
    sha1: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sha512: Option<String>,
}

pub fn validate_curseforge_export(manifest: &ProjectManifest) -> Vec<ExportIssue> {
    let mut issues = Vec::new();
    if manifest.minecraft.version.trim().is_empty() {
        issues.push(issue(
            ExportIssueSeverity::Error,
            "MISSING_MINECRAFT_VERSION",
            "Minecraft version is required for CurseForge export.",
            None,
        ));
    }
    if !matches!(manifest.loader.kind, LoaderKind::Vanilla)
        && manifest.loader.version.trim().is_empty()
    {
        issues.push(issue(
            ExportIssueSeverity::Error,
            "MISSING_LOADER_VERSION",
            "Loader version is required for CurseForge export.",
            None,
        ));
    }
    if manifest.mods.is_empty() {
        issues.push(issue(
            ExportIssueSeverity::Warning,
            "NO_MODS",
            "The project has no mods; CurseForge zip will contain only overrides.",
            None,
        ));
    }
    let mut cf_count = 0usize;
    for module in &manifest.mods {
        let has_cf = matches!(module.source.kind, SourceKind::Curseforge)
            && module
                .source
                .project_id
                .as_deref()
                .map(|s| !s.is_empty())
                .unwrap_or(false)
            && module
                .source
                .file_id
                .as_deref()
                .map(|s| !s.is_empty())
                .unwrap_or(false);
        if has_cf {
            cf_count += 1;
            continue;
        }
        if module.file_name.is_some() {
            issues.push(issue(
                ExportIssueSeverity::Warning,
                "CF_MOD_AS_OVERRIDE",
                "No CurseForge project/file IDs; jar will be embedded in overrides/mods if present locally.",
                Some(module.id.clone()),
            ));
        } else if module
            .source
            .url
            .as_deref()
            .map(|u| !u.is_empty())
            .unwrap_or(false)
        {
            issues.push(issue(
                ExportIssueSeverity::Warning,
                "CF_REMOTE_SIDE_CHANNEL",
                "Non-CurseForge remote kept in tuffbox.remote-mods.json (CF App will not auto-install it).",
                Some(module.id.clone()),
            ));
        } else {
            issues.push(issue(
                ExportIssueSeverity::Warning,
                "CF_MOD_UNRESOLVED",
                "Mod has neither CurseForge IDs nor a local jar/url; it may be missing from the zip.",
                Some(module.id.clone()),
            ));
        }
    }
    if !manifest.mods.is_empty() && cf_count == 0 {
        issues.push(issue(
            ExportIssueSeverity::Warning,
            "CF_NO_MANIFEST_FILES",
            "No mods have CurseForge IDs; manifest.json files[] will be empty (overrides-only pack).",
            None,
        ));
    }
    issues
}

pub fn validate_modrinth_export(manifest: &ProjectManifest) -> Vec<ExportIssue> {
    let mut issues = Vec::new();
    if manifest.project.id.trim().is_empty() {
        issues.push(issue(
            ExportIssueSeverity::Error,
            "MISSING_PROJECT_ID",
            "Project id is required for export filenames and indexes.",
            None,
        ));
    }
    if manifest.project.version.trim().is_empty() {
        issues.push(issue(
            ExportIssueSeverity::Error,
            "MISSING_PROJECT_VERSION",
            "Project version is required for .mrpack / pack indexes.",
            None,
        ));
    }
    if manifest.minecraft.version.trim().is_empty() {
        issues.push(issue(
            ExportIssueSeverity::Error,
            "MISSING_MINECRAFT_VERSION",
            "Minecraft version is required for .mrpack export.",
            None,
        ));
    }
    if !matches!(manifest.loader.kind, LoaderKind::Vanilla)
        && manifest.loader.version.trim().is_empty()
    {
        issues.push(issue(
            ExportIssueSeverity::Error,
            "MISSING_LOADER_VERSION",
            "Loader version is required for .mrpack export.",
            None,
        ));
    }
    if manifest.mods.is_empty() {
        issues.push(issue(
            ExportIssueSeverity::Warning,
            "NO_MODS",
            "The project has no mods; export will contain only overrides and dependencies.",
            None,
        ));
    }
    let mut remote_count = 0usize;
    let mut local_only = 0usize;
    for module in &manifest.mods {
        let has_url = !module.source.url.as_deref().unwrap_or_default().is_empty();
        if has_url {
            remote_count += 1;
        } else {
            local_only += 1;
            issues.push(issue(
                ExportIssueSeverity::Warning,
                "MOD_WITHOUT_DOWNLOAD_URL",
                "No download URL — this content will be embedded under overrides/ (not a Modrinth remote file).",
                Some(module.id.clone()),
            ));
        }
        let hashes = module.hashes.as_ref();
        if has_url
            && hashes.and_then(|h| h.sha1.as_ref()).is_none()
            && hashes.and_then(|h| h.sha512.as_ref()).is_none()
        {
            issues.push(issue(
                ExportIssueSeverity::Warning,
                "MOD_WITHOUT_HASH",
                "This mod has no hash metadata; Modrinth clients may not verify it correctly.",
                Some(module.id.clone()),
            ));
        }
        if module.side == Side::Unknown {
            issues.push(issue(
                ExportIssueSeverity::Warning,
                "UNKNOWN_MOD_SIDE",
                "Mod side is unknown; verify client/server compatibility before release.",
                Some(module.id.clone()),
            ));
        }
        if matches!(module.source.kind, SourceKind::Curseforge)
            && (module.source.project_id.as_deref().unwrap_or("").is_empty()
                || module.source.file_id.as_deref().unwrap_or("").is_empty())
        {
            issues.push(issue(
                ExportIssueSeverity::Warning,
                "CURSEFORGE_MISSING_IDS",
                "CurseForge entry is missing projectID/fileID — it will be packed as an override jar if present on disk.",
                Some(module.id.clone()),
            ));
        }
    }
    if remote_count == 0 && local_only > 0 {
        issues.push(issue(
            ExportIssueSeverity::Warning,
            "NO_REMOTE_DOWNLOADS",
            "No mods have download URLs. The .mrpack index will only list dependencies; all content is in overrides/.",
            None,
        ));
    }
    issues
}

fn issue(
    severity: ExportIssueSeverity,
    code: &str,
    message: &str,
    target: Option<String>,
) -> ExportIssue {
    ExportIssue {
        severity,
        code: code.to_string(),
        message: message.to_string(),
        target,
    }
}

pub fn export_modrinth_pack(
    manifest: &ProjectManifest,
    manifest_path: impl AsRef<Path>,
    output_path: impl AsRef<Path>,
) -> Result<ExportResult, ExportError> {
    let manifest_path = manifest_path.as_ref();
    let project_dir = manifest_path.parent().ok_or(ExportError::NoProjectDir)?;
    let output_path = output_path.as_ref();
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut dependencies = HashMap::new();
    dependencies.insert("minecraft".to_string(), manifest.minecraft.version.clone());
    match manifest.loader.kind {
        LoaderKind::Vanilla => {}
        LoaderKind::Fabric => {
            dependencies.insert("fabric-loader".to_string(), manifest.loader.version.clone());
        }
        LoaderKind::Forge => {
            dependencies.insert("forge".to_string(), manifest.loader.version.clone());
        }
        LoaderKind::Neoforge => {
            dependencies.insert("neoforge".to_string(), manifest.loader.version.clone());
        }
        LoaderKind::Quilt => {
            dependencies.insert("quilt-loader".to_string(), manifest.loader.version.clone());
        }
    }

    let (files, override_content, skip_override_paths) =
        modrinth_files_and_overrides(manifest, project_dir);

    let index = ModrinthIndex {
        format_version: 1,
        game: "minecraft".to_string(),
        version_id: manifest.project.version.clone(),
        name: manifest.project.name.clone(),
        summary: crate::listing::pack_summary(manifest),
        files,
        dependencies,
    };

    let output = fs::File::create(output_path)?;
    let mut zip = ZipWriter::new(output);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    zip.start_file("modrinth.index.json", options)?;
    zip.write_all(serde_json::to_string_pretty(&index)?.as_bytes())?;

    let mut warnings: Vec<String> = Vec::new();
    let mut override_count = add_modrinth_overrides(
        &mut zip,
        project_dir,
        options,
        &skip_override_paths,
        &mut warnings,
    )?;
    for (src, dest) in &override_content {
        zip.start_file(dest, options)?;
        zip.write_all(&read_export_file(src)?)?;
        override_count += 1;
    }
    override_count += add_listing_pack_icon(
        &mut zip,
        manifest,
        project_dir,
        options,
        ListingIconMode::Modrinth,
    )?;
    zip.finish()?;

    Ok(ExportResult {
        path: output_path.to_path_buf(),
        file_count: index.files.len(),
        override_count,
        warnings,
    })
}

pub fn export_server_pack(
    manifest: &ProjectManifest,
    manifest_path: impl AsRef<Path>,
    output_path: impl AsRef<Path>,
) -> Result<ExportResult, ExportError> {
    let manifest_path = manifest_path.as_ref();
    let project_dir = manifest_path.parent().ok_or(ExportError::NoProjectDir)?;
    let output_path = output_path.as_ref();
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let output = fs::File::create(output_path)?;
    let mut zip = ZipWriter::new(output);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let mut included_mods = Vec::new();
    let mut remote_mods = Vec::new();
    let mut skipped_client_mods = Vec::new();
    let mut file_count = 0;

    for module in &manifest.mods {
        if module.side == Side::Client {
            skipped_client_mods.push(module.id.clone());
            continue;
        }

        let Some(file_name) = module.file_name.clone() else {
            if let Some(url) = &module.source.url {
                let hashes = module.hashes.as_ref();
                remote_mods.push(ServerPackRemoteMod {
                    id: module.id.clone(),
                    name: module.name.clone(),
                    version: module.version.clone(),
                    file_name: None,
                    url: url.clone(),
                    sha1: hashes.and_then(|h| h.sha1.clone()),
                    sha512: hashes.and_then(|h| h.sha512.clone()),
                });
            }
            continue;
        };

        let local_path = project_dir.join("mods").join(&file_name);
        if local_path.is_file() {
            zip.start_file(format!("mods/{file_name}"), options)?;
            zip.write_all(&read_export_file(&local_path)?)?;
            file_count += 1;
            included_mods.push(ServerPackMod {
                id: module.id.clone(),
                name: module.name.clone(),
                version: module.version.clone(),
                file_name,
            });
        } else if let Some(url) = &module.source.url {
            let hashes = module.hashes.as_ref();
            remote_mods.push(ServerPackRemoteMod {
                id: module.id.clone(),
                name: module.name.clone(),
                version: module.version.clone(),
                file_name: Some(file_name),
                url: url.clone(),
                sha1: hashes.and_then(|h| h.sha1.clone()),
                sha512: hashes.and_then(|h| h.sha512.clone()),
            });
        }
    }

    let mut warnings: Vec<String> = Vec::new();
    let override_count = add_server_overrides(&mut zip, project_dir, options, &mut warnings)?;
    let server_manifest = ServerPackManifest {
        name: manifest.project.name.clone(),
        version: manifest.project.version.clone(),
        minecraft_version: manifest.minecraft.version.clone(),
        loader: ServerPackLoader {
            kind: format!("{:?}", manifest.loader.kind).to_lowercase(),
            version: manifest.loader.version.clone(),
        },
        included_mods,
        remote_mods,
        skipped_client_mods,
    };

    zip.start_file("tuffbox.server-pack.json", options)?;
    zip.write_all(serde_json::to_string_pretty(&server_manifest)?.as_bytes())?;

    zip.start_file("README_INSTALL.txt", options)?;
    zip.write_all(server_readme(manifest, &server_manifest).as_bytes())?;
    zip.start_file("start.bat", options)?;
    zip.write_all(start_bat().as_bytes())?;
    zip.start_file("start.sh", options)?;
    zip.write_all(start_sh().as_bytes())?;
    file_count += 4;

    zip.finish()?;

    Ok(ExportResult {
        path: output_path.to_path_buf(),
        file_count,
        override_count,
        warnings,
    })
}

pub fn export_prism_instance(
    manifest: &ProjectManifest,
    manifest_path: impl AsRef<Path>,
    output_path: impl AsRef<Path>,
) -> Result<ExportResult, ExportError> {
    let manifest_path = manifest_path.as_ref();
    let project_dir = manifest_path.parent().ok_or(ExportError::NoProjectDir)?;
    let output_path = output_path.as_ref();
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let output = fs::File::create(output_path)?;
    let mut zip = ZipWriter::new(output);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    zip.start_file("instance.cfg", options)?;
    zip.write_all(prism_instance_cfg(manifest).as_bytes())?;

    zip.start_file("mmc-pack.json", options)?;
    zip.write_all(serde_json::to_string_pretty(&prism_pack(manifest))?.as_bytes())?;

    zip.start_file("tuffbox.remote-mods.json", options)?;
    zip.write_all(serde_json::to_string_pretty(&remote_mod_manifest(manifest))?.as_bytes())?;

    let mut warnings: Vec<String> = Vec::new();
    let override_count = add_prism_files(&mut zip, project_dir, options, &mut warnings)?;
    zip.finish()?;

    Ok(ExportResult {
        path: output_path.to_path_buf(),
        file_count: 3,
        override_count,
        warnings,
    })
}

pub fn export_curseforge_pack(
    manifest: &ProjectManifest,
    manifest_path: impl AsRef<Path>,
    output_path: impl AsRef<Path>,
) -> Result<ExportResult, ExportError> {
    let manifest_path = manifest_path.as_ref();
    let project_dir = manifest_path.parent().ok_or(ExportError::NoProjectDir)?;
    let output_path = output_path.as_ref();
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let (files, override_jars) = curseforge_files_and_jars(manifest, project_dir);

    let output = fs::File::create(output_path)?;
    let mut zip = ZipWriter::new(output);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let cf_manifest = CurseForgeManifest {
        minecraft: CurseForgeMinecraft {
            version: manifest.minecraft.version.clone(),
            mod_loaders: curseforge_loaders(manifest),
        },
        manifest_type: "minecraftModpack".to_string(),
        manifest_version: 1,
        name: manifest.project.name.clone(),
        version: manifest.project.version.clone(),
        author: manifest
            .project
            .authors
            .first()
            .cloned()
            .unwrap_or_else(|| "TuffBox".to_string()),
        files,
        overrides: "overrides".to_string(),
    };

    zip.start_file("manifest.json", options)?;
    zip.write_all(serde_json::to_string_pretty(&cf_manifest)?.as_bytes())?;

    // Remotes that are not CurseForge entries (Modrinth/direct URLs) stay as a
    // side channel so nothing is silently dropped.
    let remotes: Vec<ServerPackRemoteMod> = remote_mod_manifest(manifest)
        .into_iter()
        .filter(|r| {
            !manifest
                .mods
                .iter()
                .any(|m| m.id == r.id && matches!(m.source.kind, SourceKind::Curseforge))
        })
        .collect();
    if !remotes.is_empty() {
        zip.start_file("tuffbox.remote-mods.json", options)?;
        zip.write_all(serde_json::to_string_pretty(&remotes)?.as_bytes())?;
    }

    let mut warnings: Vec<String> = Vec::new();
    let mut override_count = add_overrides(&mut zip, project_dir, options, &mut warnings)?;
    override_count += add_listing_pack_icon(
        &mut zip,
        manifest,
        project_dir,
        options,
        ListingIconMode::CurseForge,
    )?;
    for jar in &override_jars {
        let dest = format!(
            "overrides/mods/{}",
            jar.file_name()
                .map(|s| s.to_string_lossy())
                .unwrap_or_else(|| "mod.jar".into())
        );
        zip.start_file(&dest, options)?;
        zip.write_all(&read_export_file(jar)?)?;
        override_count += 1;
    }
    zip.finish()?;

    Ok(ExportResult {
        path: output_path.to_path_buf(),
        file_count: cf_manifest.files.len() + if remotes.is_empty() { 1 } else { 2 },
        override_count,
        warnings,
    })
}

/// Build CurseForge `files[]` from mods that already have project/file IDs,
/// fingerprint-resolve local jars when possible, and collect leftover jars for
/// `overrides/mods/`.
fn curseforge_files_and_jars(
    manifest: &ProjectManifest,
    project_dir: &Path,
) -> (Vec<CurseForgeFile>, Vec<PathBuf>) {
    let mut files = Vec::new();
    let mut override_jars = Vec::new();
    let mut seen = std::collections::HashSet::new();

    let cf = CurseForgeProvider::new();
    // Batch fingerprint lookups for jars that lack CF IDs.
    let mut pending_fps: Vec<(usize, u32, PathBuf)> = Vec::new();
    let mut pending_mods: Vec<&crate::manifest::ModSpec> = Vec::new();

    for module in &manifest.mods {
        let required = !matches!(module.side, Side::Optional);
        if matches!(module.source.kind, SourceKind::Curseforge) {
            if let (Some(pid), Some(fid)) = (
                module
                    .source
                    .project_id
                    .as_deref()
                    .and_then(|s| s.parse().ok()),
                module
                    .source
                    .file_id
                    .as_deref()
                    .and_then(|s| s.parse().ok()),
            ) {
                let key = (pid, fid);
                if seen.insert(key) {
                    files.push(CurseForgeFile {
                        project_id: pid,
                        file_id: fid,
                        required,
                    });
                }
                continue;
            }
        }

        let jar = resolve_mod_jar_path(project_dir, module);
        if let Some(path) = jar {
            if matches!(module.source.kind, SourceKind::Curseforge)
                || module.source.project_id.is_none()
            {
                if let Ok(fp) = crate::murmur2::murmur2_file(&path) {
                    pending_fps.push((pending_mods.len(), fp, path.clone()));
                    pending_mods.push(module);
                    continue;
                }
            }
            override_jars.push(path);
        }
    }

    if !pending_fps.is_empty() {
        let fps: Vec<u32> = pending_fps.iter().map(|(_, fp, _)| *fp).collect();
        let resolved = cf.get_fingerprints(&fps).unwrap_or_default();
        for (idx, fp, path) in pending_fps {
            if let Some(info) = resolved.get(&fp) {
                let key = (info.mod_id, info.id);
                if seen.insert(key) {
                    let required = !matches!(pending_mods[idx].side, Side::Optional);
                    files.push(CurseForgeFile {
                        project_id: info.mod_id,
                        file_id: info.id,
                        required,
                    });
                }
            } else {
                override_jars.push(path);
            }
        }
    }

    files.sort_by_key(|f| (f.project_id, f.file_id));
    (files, override_jars)
}

fn resolve_mod_jar_path(project_dir: &Path, module: &crate::manifest::ModSpec) -> Option<PathBuf> {
    resolve_content_path(project_dir, module)
}

fn resolve_content_path(project_dir: &Path, module: &crate::manifest::ModSpec) -> Option<PathBuf> {
    if let Some(rel) = module.source.path.as_ref() {
        let p = project_dir.join(rel);
        if p.is_file() {
            return Some(p);
        }
    }
    if let Some(name) = module.file_name.as_ref() {
        let p = project_dir
            .join(module.content_type.folder_name())
            .join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// Build Modrinth index entries and collect local-only content for
/// `overrides/{content_type.folder}/…`, mirroring the CurseForge
/// `override_jars` pattern.
fn modrinth_files_and_overrides(
    manifest: &ProjectManifest,
    project_dir: &Path,
) -> (
    Vec<ModrinthFile>,
    Vec<(PathBuf, String)>,
    std::collections::HashSet<String>,
) {
    let mut files = Vec::new();
    let mut override_content = Vec::new();
    let mut skip_paths = std::collections::HashSet::new();

    for module in &manifest.mods {
        let file_name = module
            .file_name
            .clone()
            .unwrap_or_else(|| format!("{}.jar", module.id));
        let folder = module.content_type.folder_name();
        let index_path = format!("{folder}/{file_name}");
        let downloads = module
            .source
            .url
            .clone()
            .map(|url| vec![url])
            .unwrap_or_default();
        let hashes = module.hashes.as_ref();

        // Modrinth index: local-only content also gets an index row
        // (with empty downloads) so the pack installer knows the path
        // and hashes.  The actual bytes go into overrides/.
        if downloads.is_empty() {
            let mut has_override = false;
            if let Some(local) = resolve_content_path(project_dir, module) {
                override_content.push((local, format!("overrides/{index_path}")));
                skip_paths.insert(index_path.clone());
                has_override = true;
            }
            if has_override {
                files.push(ModrinthFile {
                    path: index_path,
                    hashes: ModrinthHashes {
                        sha1: hashes.and_then(|h| h.sha1.clone()),
                        sha512: hashes.and_then(|h| h.sha512.clone()),
                    },
                    downloads: Vec::new(),
                    env: side_env(module.side),
                });
            }
            continue;
        }

        skip_paths.insert(index_path.clone());
        files.push(ModrinthFile {
            path: index_path,
            hashes: ModrinthHashes {
                sha1: hashes.and_then(|h| h.sha1.clone()),
                sha512: hashes.and_then(|h| h.sha512.clone()),
            },
            downloads,
            env: side_env(module.side),
        });
    }

    (files, override_content, skip_paths)
}

fn prism_instance_cfg(manifest: &ProjectManifest) -> String {
    format!(
        "InstanceType=OneSix
name={}
notes=Exported by TuffBox
iconKey=default
",
        manifest.project.name
    )
}

fn prism_pack(manifest: &ProjectManifest) -> PrismPack {
    let mut components = vec![PrismComponent {
        cached_name: "Minecraft".to_string(),
        cached_version: manifest.minecraft.version.clone(),
        uid: "net.minecraft".to_string(),
        version: manifest.minecraft.version.clone(),
    }];
    if !matches!(manifest.loader.kind, LoaderKind::Vanilla) {
        let uid = match manifest.loader.kind {
            LoaderKind::Fabric => "net.fabricmc.fabric-loader",
            LoaderKind::Forge => "net.minecraftforge",
            LoaderKind::Neoforge => "net.neoforged",
            LoaderKind::Quilt => "org.quiltmc.quilt-loader",
            LoaderKind::Vanilla => "net.minecraft",
        };
        components.push(PrismComponent {
            cached_name: format!("{:?}", manifest.loader.kind),
            cached_version: manifest.loader.version.clone(),
            uid: uid.to_string(),
            version: manifest.loader.version.clone(),
        });
    }
    PrismPack {
        components,
        format_version: 1,
    }
}

fn curseforge_loaders(manifest: &ProjectManifest) -> Vec<CurseForgeLoader> {
    if matches!(manifest.loader.kind, LoaderKind::Vanilla) {
        return Vec::new();
    }
    let prefix = match manifest.loader.kind {
        LoaderKind::Fabric => "fabric",
        LoaderKind::Forge => "forge",
        LoaderKind::Neoforge => "neoforge",
        LoaderKind::Quilt => "quilt",
        LoaderKind::Vanilla => "vanilla",
    };
    vec![CurseForgeLoader {
        id: format!("{prefix}-{}", manifest.loader.version),
        primary: true,
    }]
}

fn remote_mod_manifest(manifest: &ProjectManifest) -> Vec<ServerPackRemoteMod> {
    manifest
        .mods
        .iter()
        .filter_map(|module| {
            let url = module.source.url.clone()?;
            let hashes = module.hashes.as_ref();
            Some(ServerPackRemoteMod {
                id: module.id.clone(),
                name: module.name.clone(),
                version: module.version.clone(),
                file_name: module.file_name.clone(),
                url,
                sha1: hashes.and_then(|h| h.sha1.clone()),
                sha512: hashes.and_then(|h| h.sha512.clone()),
            })
        })
        .collect()
}

fn add_prism_files<W: Write + Seek>(
    zip: &mut ZipWriter<W>,
    project_dir: &Path,
    options: SimpleFileOptions,
    warnings: &mut Vec<String>,
) -> Result<usize, ExportError> {
    let mut count = 0;
    for root in [
        "config",
        "defaultconfigs",
        "kubejs",
        "scripts",
        "resourcepacks",
        "shaderpacks",
        "mods",
    ] {
        let dir = project_dir.join(root);
        if dir.is_dir() {
            count += add_dir_plain(zip, project_dir, &dir, options, warnings)?;
        }
    }
    Ok(count)
}

fn add_dir_plain<W: Write + Seek>(
    zip: &mut ZipWriter<W>,
    project_dir: &Path,
    dir: &Path,
    options: SimpleFileOptions,
    warnings: &mut Vec<String>,
) -> Result<usize, ExportError> {
    let mut count = 0;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_symlink() {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            count += add_dir_plain(zip, project_dir, &path, options, warnings)?;
        } else if path.is_file() {
            let relative = path
                .strip_prefix(project_dir)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let bytes = match read_export_file(&path) {
                Ok(bytes) => bytes,
                Err(e) => {
                    // A mod jar is pack payload — a missing jar means a
                    // broken export, so mods/ still fails hard. Everything
                    // else (configs etc.) is skipped with a loud warning.
                    if relative.starts_with("mods/") {
                        return Err(ExportError::Io(e));
                    }
                    warnings.push(format!("skipped {relative}: {e}"));
                    continue;
                }
            };
            zip.start_file(relative, options)?;
            zip.write_all(&bytes)?;
            count += 1;
        }
    }
    Ok(count)
}

fn add_overrides<W: Write + Seek>(
    zip: &mut ZipWriter<W>,
    project_dir: &Path,
    options: SimpleFileOptions,
    warnings: &mut Vec<String>,
) -> Result<usize, ExportError> {
    add_modrinth_overrides(
        zip,
        project_dir,
        options,
        &std::collections::HashSet::new(),
        warnings,
    )
}

fn add_modrinth_overrides<W: Write + Seek>(
    zip: &mut ZipWriter<W>,
    project_dir: &Path,
    options: SimpleFileOptions,
    skip_paths: &std::collections::HashSet<String>,
    warnings: &mut Vec<String>,
) -> Result<usize, ExportError> {
    let ignore = TuffboxIgnore::load(project_dir);
    let mut count = 0;
    for root in [
        "config",
        "defaultconfigs",
        "kubejs",
        "scripts",
        "resourcepacks",
        "shaderpacks",
        "datapacks",
    ] {
        let dir = project_dir.join(root);
        if dir.is_dir() {
            count += add_dir(zip, project_dir, &dir, options, &ignore, skip_paths, warnings)?;
        }
    }
    Ok(count)
}

#[derive(Clone, Copy)]
enum ListingIconMode {
    Modrinth,
    CurseForge,
}

fn add_listing_pack_icon<W: Write + Seek>(
    zip: &mut ZipWriter<W>,
    manifest: &ProjectManifest,
    project_dir: &Path,
    options: SimpleFileOptions,
    mode: ListingIconMode,
) -> Result<usize, ExportError> {
    let Some(listing) = manifest.listing.as_ref() else {
        return Ok(0);
    };
    let Some(src) = crate::listing::resolve_listing_icon(project_dir, listing) else {
        return Ok(0);
    };
    let bytes = read_export_file(&src)?;
    let mut count = 0;
    match mode {
        ListingIconMode::Modrinth => {
            zip.start_file("overrides/icon.png", options)?;
            zip.write_all(&bytes)?;
            count += 1;
        }
        ListingIconMode::CurseForge => {
            zip.start_file("pack.png", options)?;
            zip.write_all(&bytes)?;
            count += 1;
            zip.start_file("overrides/icon.png", options)?;
            zip.write_all(&bytes)?;
            count += 1;
        }
    }
    Ok(count)
}

fn add_server_overrides<W: Write + Seek>(
    zip: &mut ZipWriter<W>,
    project_dir: &Path,
    options: SimpleFileOptions,
    warnings: &mut Vec<String>,
) -> Result<usize, ExportError> {
    let ignore = TuffboxIgnore::load(project_dir);
    let mut count = 0;
    for root in ["config", "defaultconfigs", "kubejs", "scripts"] {
        let dir = project_dir.join(root);
        if dir.is_dir() {
            count += add_dir(
                zip,
                project_dir,
                &dir,
                options,
                &ignore,
                &std::collections::HashSet::new(),
                warnings,
            )?;
        }
    }
    Ok(count)
}

fn server_readme(manifest: &ProjectManifest, server_manifest: &ServerPackManifest) -> String {
    let mut readme = format!(
        "# {} {} server pack\n\nMinecraft: {}\nLoader: {} {}\n\n",
        manifest.project.name,
        manifest.project.version,
        manifest.minecraft.version,
        server_manifest.loader.kind,
        server_manifest.loader.version,
    );
    readme.push_str("## Install\n\n");
    readme.push_str("1. Install the matching Minecraft server and loader.\n");
    readme.push_str("2. Copy folders from this archive into the server directory.\n");
    readme.push_str(
        "3. If `tuffbox.server-pack.json` contains `remoteMods`, download them into `mods/`.\n",
    );
    readme.push_str("4. Review EULA and run `start.bat` or `start.sh`.\n\n");
    if !server_manifest.remote_mods.is_empty() {
        readme.push_str("## Remote mods to download\n\n");
        for module in &server_manifest.remote_mods {
            readme.push_str(&format!(
                "- {} {}: {}\n",
                module.name, module.version, module.url
            ));
        }
        readme.push('\n');
    }
    if !server_manifest.skipped_client_mods.is_empty() {
        readme.push_str("## Client-only mods skipped\n\n");
        for id in &server_manifest.skipped_client_mods {
            readme.push_str(&format!("- {id}\n"));
        }
    }
    readme
}

/// Packs `logs/`, `crash-reports/` and `.tuffbox/test-runs/` from a project
/// into a single zip, for easy sharing when asking for help debugging a
/// modpack (the classic launcher "Create logs.zip" action).
pub fn export_logs_zip(
    project_dir: impl AsRef<Path>,
    output_path: impl AsRef<Path>,
) -> Result<ExportResult, ExportError> {
    let project_dir = project_dir.as_ref();
    let output_path = output_path.as_ref();
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let output = fs::File::create(output_path)?;
    let mut zip = ZipWriter::new(output);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let mut file_count = 0;
    for relative_root in ["logs", "crash-reports", ".tuffbox/test-runs"] {
        let dir = project_dir.join(relative_root);
        if dir.is_dir() {
            file_count += add_dir_flat(&mut zip, project_dir, &dir, options)?;
        }
    }

    zip.finish()?;

    Ok(ExportResult {
        path: output_path.to_path_buf(),
        file_count,
        override_count: 0,
        warnings: Vec::new(),
    })
}

/// Like [`add_dir`], but preserves the file's path relative to
/// `project_dir` as-is inside the archive instead of nesting everything
/// under an `overrides/` prefix (used for logs.zip, not modpack exports).
fn add_dir_flat<W: Write + Seek>(
    zip: &mut ZipWriter<W>,
    project_dir: &Path,
    dir: &Path,
    options: SimpleFileOptions,
) -> Result<usize, ExportError> {
    let mut count = 0;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_symlink() {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            count += add_dir_flat(zip, project_dir, &path, options)?;
        } else if path.is_file() {
            let relative = path
                .strip_prefix(project_dir)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            zip.start_file(relative, options)?;
            zip.write_all(&read_export_file(&path)?)?;
            count += 1;
        }
    }
    Ok(count)
}

fn start_bat() -> &'static str {
    "@echo off\r\njava -Xmx4G -jar server.jar nogui\r\npause\r\n"
}

fn start_sh() -> &'static str {
    "#!/usr/bin/env sh\njava -Xmx4G -jar server.jar nogui\n"
}

fn add_dir<W: Write + Seek>(
    zip: &mut ZipWriter<W>,
    project_dir: &Path,
    dir: &Path,
    options: SimpleFileOptions,
    ignore: &TuffboxIgnore,
    skip_paths: &std::collections::HashSet<String>,
    warnings: &mut Vec<String>,
) -> Result<usize, ExportError> {
    let mut count = 0;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_symlink() {
            continue;
        }
        let path = entry.path();
        let relative = path
            .strip_prefix(project_dir)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if ignore.is_ignored(&relative) || skip_paths.contains(&relative) {
            continue;
        }
        if path.is_dir() {
            count += add_dir(zip, project_dir, &path, options, ignore, skip_paths, warnings)?;
        } else if path.is_file() {
            let bytes = match read_export_file(&path) {
                Ok(bytes) => bytes,
                // Config-style override: skip with a loud warning instead of
                // failing a multi-GB export at 95% because one file was
                // briefly locked by the running game or an antivirus.
                Err(e) => {
                    warnings.push(format!("skipped {relative}: {e}"));
                    continue;
                }
            };
            zip.start_file(format!("overrides/{relative}"), options)?;
            zip.write_all(&bytes)?;
            count += 1;
        }
    }
    Ok(count)
}

/// Minimal `.tuffboxignore` (gitignore-like prefixes / `*` globs). Empty = no filter.
struct TuffboxIgnore {
    patterns: Vec<String>,
}

impl TuffboxIgnore {
    fn load(project_dir: &Path) -> Self {
        let path = project_dir.join(".tuffboxignore");
        let Ok(raw) = fs::read_to_string(path) else {
            return Self {
                patterns: Vec::new(),
            };
        };
        let patterns = raw
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(|s| s.replace('\\', "/"))
            .collect();
        Self { patterns }
    }

    fn is_ignored(&self, relative: &str) -> bool {
        let rel = relative.replace('\\', "/");
        self.patterns.iter().any(|pat| match_ignore(pat, &rel))
    }
}

fn match_ignore(pattern: &str, path: &str) -> bool {
    let pat = pattern.trim_start_matches('/');
    if let Some(prefix) = pat.strip_suffix("/**") {
        return path == prefix || path.starts_with(&format!("{prefix}/"));
    }
    if pat.ends_with('/') {
        let prefix = pat.trim_end_matches('/');
        return path == prefix || path.starts_with(&format!("{prefix}/"));
    }
    if pat.contains('*') {
        return wildcard_match(pat, path)
            || path
                .rsplit('/')
                .next()
                .map(|name| wildcard_match(pat, name))
                .unwrap_or(false);
    }
    path == pat || path.ends_with(&format!("/{pat}")) || path.starts_with(&format!("{pat}/"))
}

fn wildcard_match(pattern: &str, text: &str) -> bool {
    // ponytail: single-`*` glob only; `**` via prefix rules above.
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return text == pattern;
    }
    if !text.starts_with(parts[0]) {
        return false;
    }
    let mut rest = &text[parts[0].len()..];
    for (i, part) in parts.iter().enumerate().skip(1) {
        if part.is_empty() {
            if i == parts.len() - 1 {
                return true;
            }
            continue;
        }
        match rest.find(part) {
            Some(idx) => rest = &rest[idx + part.len()..],
            None => return false,
        }
    }
    rest.is_empty() || pattern.ends_with('*')
}

fn side_env(side: Side) -> HashMap<String, String> {
    let mut env = HashMap::new();
    match side {
        Side::Client => {
            env.insert("client".to_string(), "required".to_string());
            env.insert("server".to_string(), "unsupported".to_string());
        }
        Side::Server => {
            env.insert("client".to_string(), "unsupported".to_string());
            env.insert("server".to_string(), "required".to_string());
        }
        Side::Optional => {
            env.insert("client".to_string(), "optional".to_string());
            env.insert("server".to_string(), "optional".to_string());
        }
        Side::Both | Side::Unknown => {
            env.insert("client".to_string(), "required".to_string());
            env.insert("server".to_string(), "required".to_string());
        }
    }
    env
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{
        ContentType, FileHashes, LoaderSpec, MinecraftSpec, ModSource, ModSpec, ProfileSpec,
        ProjectMetadata, SourceKind,
    };
    use std::io::Read;

    fn fixture_manifest(_dir: &Path) -> ProjectManifest {
        ProjectManifest {
            schema_version: "1.0".to_string(),
            project: ProjectMetadata {
                id: "test-pack".to_string(),
                name: "Test Pack".to_string(),
                version: "1.0.0".to_string(),
                description: Some("smoke test".to_string()),
                authors: vec![],
            },
            minecraft: MinecraftSpec {
                version: "1.20.1".to_string(),
            },
            loader: LoaderSpec {
                kind: LoaderKind::Fabric,
                version: "0.16.0".to_string(),
            },
            brief: None,
            listing: None,
            java: None,
            profiles: vec![ProfileSpec {
                id: "client".to_string(),
                name: "Client".to_string(),
                side: Side::Client,
                include_optional_mods: false,
                include_shaders: false,
                memory_mb: None,
                jvm_args: vec![],
                include_mods: vec![],
                player_name: None,
            }],
            mods: vec![
                ModSpec {
                    id: "sodium".to_string(),
                    name: "Sodium".to_string(),
                    source: ModSource {
                        kind: SourceKind::Modrinth,
                        project_id: Some("sodium".to_string()),
                        file_id: None,
                        url: Some("https://example.com/sodium.jar".to_string()),
                        path: None,
                        icon_url: None,
                        categories: Vec::new(),
                    },
                    version: "0.5.0".to_string(),
                    file_name: Some("sodium.jar".to_string()),
                    hashes: Some(FileHashes {
                        sha1: Some("abc".to_string()),
                        sha512: None,
                    }),
                    side: Side::Both,
                    dependencies: vec![],
                    status: vec![],
                    content_type: ContentType::Mod,
                    authors: Vec::new(),
                    option: None,
                },
                ModSpec {
                    id: "clientmod".to_string(),
                    name: "Client Mod".to_string(),
                    source: ModSource {
                        kind: SourceKind::Modrinth,
                        project_id: Some("clientmod".to_string()),
                        file_id: None,
                        url: Some("https://example.com/clientmod.jar".to_string()),
                        path: None,
                        icon_url: None,
                        categories: Vec::new(),
                    },
                    version: "1.0.0".to_string(),
                    file_name: Some("clientmod.jar".to_string()),
                    hashes: None,
                    side: Side::Client,
                    dependencies: vec![],
                    status: vec![],
                    content_type: ContentType::Mod,
                    authors: Vec::new(),
                    option: None,
                },
                ModSpec {
                    id: "cf-jei".to_string(),
                    name: "JEI".to_string(),
                    source: ModSource {
                        kind: SourceKind::Curseforge,
                        project_id: Some("238222".to_string()),
                        file_id: Some("5101366".to_string()),
                        url: None,
                        path: None,
                        icon_url: None,
                        categories: Vec::new(),
                    },
                    version: "15.20.0.106".to_string(),
                    file_name: Some("jei.jar".to_string()),
                    hashes: None,
                    side: Side::Both,
                    dependencies: vec![],
                    status: vec![],
                    content_type: ContentType::Mod,
                    authors: Vec::new(),
                    option: None,
                },
            ],
            overrides: None,
        }
    }

    fn write_manifest(dir: &Path) -> PathBuf {
        let manifest_path = dir.join("tuffbox.json");
        let manifest = fixture_manifest(dir);
        fs::write(
            &manifest_path,
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
        // mods/ folder so the exporter can resolve files
        fs::create_dir_all(dir.join("mods")).unwrap();
        fs::write(dir.join("mods").join("sodium.jar"), b"dummy").unwrap();
        fs::write(dir.join("mods").join("clientmod.jar"), b"dummy").unwrap();
        manifest_path
    }

    #[test]
    fn export_modrinth_pack_smoke() {
        let dir = std::env::temp_dir().join("tuffbox_export_test_mr");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let manifest_path = write_manifest(&dir);
        let out = dir.join("pack.mrpack");
        let result = export_modrinth_pack(&fixture_manifest(&dir), &manifest_path, &out);
        assert!(
            result.is_ok(),
            "modrinth pack export failed: {:?}",
            result.err()
        );
        let res = result.unwrap();
        assert!(out.exists(), "output mrpack not created");
        // fixture has 3 mods with URLs (sodium, clientmod, cf-jei) — all index rows.
        // CF entry has no url so it is override-only if a jar exists; fixture has no jei.jar.
        assert_eq!(res.file_count, 2);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_modrinth_pack_embeds_local_jar_and_uses_content_type_paths() {
        use zip::ZipArchive;

        let dir = std::env::temp_dir().join("tuffbox_export_test_mr_local");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("mods")).unwrap();
        fs::create_dir_all(dir.join("resourcepacks")).unwrap();
        fs::write(dir.join("mods").join("localmod.jar"), b"local-jar-bytes").unwrap();
        fs::write(
            dir.join("resourcepacks").join("vanilla-tweaks.zip"),
            b"rp-bytes",
        )
        .unwrap();

        let manifest = ProjectManifest {
            schema_version: "1.0".to_string(),
            project: ProjectMetadata {
                id: "local-pack".to_string(),
                name: "Local Pack".to_string(),
                version: "1.0.0".to_string(),
                description: None,
                authors: vec![],
            },
            minecraft: MinecraftSpec {
                version: "1.20.1".to_string(),
            },
            loader: LoaderSpec {
                kind: LoaderKind::Fabric,
                version: "0.16.0".to_string(),
            },
            brief: None,
            listing: None,
            java: None,
            profiles: vec![],
            mods: vec![
                ModSpec {
                    id: "localmod".to_string(),
                    name: "Local Mod".to_string(),
                    source: ModSource {
                        kind: SourceKind::Local,
                        project_id: None,
                        file_id: None,
                        url: None,
                        path: None,
                        icon_url: None,
                        categories: Vec::new(),
                    },
                    version: "1.0.0".to_string(),
                    file_name: Some("localmod.jar".to_string()),
                    hashes: Some(FileHashes {
                        sha1: Some("local-sha1".to_string()),
                        sha512: None,
                    }),
                    side: Side::Both,
                    dependencies: vec![],
                    status: vec![],
                    content_type: ContentType::Mod,
                    authors: Vec::new(),
                    option: None,
                },
                ModSpec {
                    id: "remote-rp".to_string(),
                    name: "Remote RP".to_string(),
                    source: ModSource {
                        kind: SourceKind::Modrinth,
                        project_id: Some("vanilla-tweaks".to_string()),
                        file_id: None,
                        url: Some("https://example.com/vanilla-tweaks.zip".to_string()),
                        path: None,
                        icon_url: None,
                        categories: Vec::new(),
                    },
                    version: "1.0.0".to_string(),
                    file_name: Some("vanilla-tweaks.zip".to_string()),
                    hashes: Some(FileHashes {
                        sha1: Some("rp-sha1".to_string()),
                        sha512: None,
                    }),
                    side: Side::Both,
                    dependencies: vec![],
                    status: vec![],
                    content_type: ContentType::Resourcepack,
                    authors: Vec::new(),
                    option: None,
                },
            ],
            overrides: None,
        };

        let manifest_path = dir.join("tuffbox.json");
        fs::write(
            &manifest_path,
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let out = dir.join("pack.mrpack");
        let result = export_modrinth_pack(&manifest, &manifest_path, &out);
        assert!(result.is_ok(), "{:?}", result.err());
        let res = result.unwrap();
        assert_eq!(res.file_count, 2, "local mod + remote rp in index");
        assert_eq!(res.override_count, 1, "only local jar embedded, not remote rp on disk");

        let file = fs::File::open(&out).unwrap();
        let mut zip = ZipArchive::new(file).unwrap();

        let mut index = String::new();
        zip.by_name("modrinth.index.json")
            .unwrap()
            .read_to_string(&mut index)
            .unwrap();
        let index_json: serde_json::Value = serde_json::from_str(&index).unwrap();
        let files = index_json.get("files").and_then(|v| v.as_array()).unwrap();
        assert_eq!(files.len(), 2, "local mod + remote rp in index files[]");

        let local_entry = files
            .iter()
            .find(|f| f.get("path").and_then(|p| p.as_str()) == Some("mods/localmod.jar"))
            .expect("local mod in index");
        assert!(local_entry
            .get("downloads")
            .and_then(|d| d.as_array())
            .unwrap()
            .is_empty());
        assert_eq!(
            local_entry
                .get("hashes")
                .and_then(|h| h.get("sha1"))
                .and_then(|s| s.as_str()),
            Some("local-sha1")
        );

        let rp_entry = files
            .iter()
            .find(|f| {
                f.get("path").and_then(|p| p.as_str()) == Some("resourcepacks/vanilla-tweaks.zip")
            })
            .expect("resourcepack uses resourcepacks/ path");
        assert_eq!(
            rp_entry
                .get("downloads")
                .and_then(|d| d.as_array())
                .and_then(|d| d.first())
                .and_then(|u| u.as_str()),
            Some("https://example.com/vanilla-tweaks.zip")
        );

        let mut embedded = Vec::new();
        zip.by_name("overrides/mods/localmod.jar")
            .unwrap()
            .read_to_end(&mut embedded)
            .unwrap();
        assert_eq!(embedded, b"local-jar-bytes");
        assert!(
            zip.by_name("overrides/resourcepacks/vanilla-tweaks.zip")
                .is_err(),
            "remote resourcepack on disk must not be double-packed"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn validate_modrinth_export_warns_embedded_not_skipped() {
        let manifest = ProjectManifest {
            schema_version: "1.0".to_string(),
            project: ProjectMetadata {
                id: "test".to_string(),
                name: "Test".to_string(),
                version: "1.0.0".to_string(),
                description: None,
                authors: vec![],
            },
            minecraft: MinecraftSpec {
                version: "1.20.1".to_string(),
            },
            loader: LoaderSpec {
                kind: LoaderKind::Fabric,
                version: "0.16.0".to_string(),
            },
            brief: None,
            listing: None,
            java: None,
            profiles: vec![],
            mods: vec![ModSpec {
                id: "local".to_string(),
                name: "Local".to_string(),
                source: ModSource {
                    kind: SourceKind::Local,
                    project_id: None,
                    file_id: None,
                    url: None,
                    path: None,
                    icon_url: None,
                    categories: Vec::new(),
                },
                version: "1.0.0".to_string(),
                file_name: Some("local.jar".to_string()),
                hashes: None,
                side: Side::Both,
                dependencies: vec![],
                status: vec![],
                content_type: ContentType::Mod,
                authors: Vec::new(),
                option: None,
            }],
            overrides: None,
        };
        let issues = validate_modrinth_export(&manifest);
        let url_issue = issues
            .iter()
            .find(|i| i.code == "MOD_WITHOUT_DOWNLOAD_URL")
            .expect("missing url warning");
        assert!(url_issue.message.contains("embedded"));
        assert!(!url_issue.message.contains("skipped"));
    }

    #[test]
    fn export_modrinth_pack_uses_listing_summary_and_icon() {
        use crate::manifest::ProjectListing;
        use std::io::Read;
        use zip::ZipArchive;

        let dir = std::env::temp_dir().join("tuffbox_export_test_listing");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".tuffbox/listing")).unwrap();
        let icon = dir.join(".tuffbox/listing/icon.png");
        fs::write(&icon, b"fake-png-bytes").unwrap();

        let mut manifest = fixture_manifest(&dir);
        manifest.project.description = Some("fallback desc".into());
        manifest.listing = Some(ProjectListing {
            name: "Listed".into(),
            summary: "Listing summary wins".into(),
            body_markdown: String::new(),
            icon_path: Some(".tuffbox/listing/icon.png".into()),
            gallery: vec![],
            categories: vec![],
            authors: vec![],
        });

        let manifest_path = dir.join("tuffbox.project.json");
        fs::write(
            &manifest_path,
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
        let out = dir.join("pack.mrpack");
        let result = export_modrinth_pack(&manifest, &manifest_path, &out);
        assert!(result.is_ok(), "{:?}", result.err());

        let file = fs::File::open(&out).unwrap();
        let mut zip = ZipArchive::new(file).unwrap();
        let mut index = String::new();
        zip.by_name("modrinth.index.json")
            .unwrap()
            .read_to_string(&mut index)
            .unwrap();
        assert!(index.contains("Listing summary wins"));
        assert!(!index.contains("fallback desc"));
        assert!(zip.by_name("overrides/icon.png").is_ok());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_server_pack_skips_client_mods() {
        let dir = std::env::temp_dir().join("tuffbox_export_test_srv");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let manifest_path = write_manifest(&dir);
        let out = dir.join("server.zip");
        let result = export_server_pack(&fixture_manifest(&dir), &manifest_path, &out);
        assert!(
            result.is_ok(),
            "server pack export failed: {:?}",
            result.err()
        );
        assert!(out.exists(), "output server zip not created");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_prism_instance_smoke() {
        let dir = std::env::temp_dir().join("tuffbox_export_test_prism");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let manifest_path = write_manifest(&dir);
        let out = dir.join("instance.zip");
        let result = export_prism_instance(&fixture_manifest(&dir), &manifest_path, &out);
        assert!(
            result.is_ok(),
            "prism instance export failed: {:?}",
            result.err()
        );
        assert!(out.exists(), "output prism zip not created");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_curseforge_pack_smoke() {
        let dir = std::env::temp_dir().join("tuffbox_export_test_cf");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let manifest_path = write_manifest(&dir);
        let out = dir.join("modpack.zip");
        let result = export_curseforge_pack(&fixture_manifest(&dir), &manifest_path, &out);
        assert!(
            result.is_ok(),
            "curseforge pack export failed: {:?}",
            result.err()
        );
        assert!(out.exists(), "output curseforge zip not created");

        let file = fs::File::open(&out).unwrap();
        let mut zip = zip::ZipArchive::new(file).unwrap();
        let mut manifest_raw = String::new();
        zip.by_name("manifest.json")
            .unwrap()
            .read_to_string(&mut manifest_raw)
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&manifest_raw).unwrap();
        let files = value.get("files").and_then(|v| v.as_array()).unwrap();
        assert_eq!(files.len(), 1, "expected one CF file entry, got {files:?}");
        assert_eq!(
            files[0].get("projectID").and_then(|v| v.as_u64()),
            Some(238222)
        );
        assert_eq!(
            files[0].get("fileID").and_then(|v| v.as_u64()),
            Some(5101366)
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn tuffboxignore_skips_override_paths() {
        assert!(match_ignore("config/secret.cfg", "config/secret.cfg"));
        assert!(match_ignore("*.log", "config/debug.log"));
        assert!(match_ignore("kubejs/**", "kubejs/server_scripts/foo.js"));
        assert!(!match_ignore("config/secret.cfg", "config/other.cfg"));
    }
}
