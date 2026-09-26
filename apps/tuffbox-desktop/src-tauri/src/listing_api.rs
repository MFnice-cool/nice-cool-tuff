//! Storefront listing commands for the Brief tab.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use tuffbox_core::{
    gallery_dir, icon_extension, listing_dir, mime_for_ext, ListingGalleryItem, PackBrief,
    ProjectListing, ProjectManifest, GALLERY_DIR_REL, LISTING_DIR_REL,
};

use crate::{auto_snapshot, resolve_manifest_path, save_manifest};

fn resolve_paths(path: &str) -> Result<(PathBuf, PathBuf), String> {
    let manifest_path = resolve_manifest_path(path)?;
    let project_dir = manifest_path
        .parent()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| "manifest has no parent directory".to_string())?;
    Ok((manifest_path, project_dir))
}

fn seed_listing(manifest: &ProjectManifest) -> ProjectListing {
    let mut listing = manifest.listing.clone().unwrap_or_default();
    if listing.name.trim().is_empty() {
        listing.name = manifest.project.name.clone();
    }
    if listing.summary.trim().is_empty() {
        listing.summary = manifest
            .project
            .description
            .clone()
            .unwrap_or_default();
    }
    if listing.authors.is_empty() {
        listing.authors = manifest.project.authors.clone();
    }
    listing
}

fn apply_listing_to_project(manifest: &mut ProjectManifest, listing: &ProjectListing) {
    if !listing.name.trim().is_empty() {
        manifest.project.name = listing.name.trim().to_string();
    }
    let summary = listing.summary.trim().to_string();
    manifest.project.description = if summary.is_empty() {
        None
    } else {
        Some(summary)
    };
    if !listing.authors.is_empty() {
        manifest.project.authors = listing.authors.clone();
    }
}

fn save_listing(manifest_path: &Path, listing: ProjectListing) -> Result<ProjectListing, String> {
    auto_snapshot(manifest_path, "update-listing").map_err(|e| e.to_string())?;
    let mut manifest =
        ProjectManifest::load_from_path(manifest_path).map_err(|e| e.to_string())?;
    apply_listing_to_project(&mut manifest, &listing);
    manifest.listing = Some(listing.clone());
    save_manifest(manifest_path, &manifest).map_err(|e| e.to_string())?;
    Ok(listing)
}

fn bytes_to_data_url(bytes: &[u8], path: &Path) -> String {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png");
    let mime = mime_for_ext(ext);
    format!("data:{mime};base64,{}", STANDARD.encode(bytes))
}

#[tauri::command(rename_all = "camelCase")]
pub fn get_project_listing(path: String) -> Result<ProjectListing, String> {
    let (manifest_path, _) = resolve_paths(&path)?;
    let manifest = ProjectManifest::load_from_path(&manifest_path).map_err(|e| e.to_string())?;
    Ok(seed_listing(&manifest))
}

#[tauri::command(rename_all = "camelCase")]
pub fn update_project_listing(path: String, listing: ProjectListing) -> Result<(), String> {
    let (manifest_path, _) = resolve_paths(&path)?;
    let _ = save_listing(&manifest_path, listing)?;
    Ok(())
}

#[tauri::command(rename_all = "camelCase")]
pub fn set_project_listing_icon(path: String, source_file: String) -> Result<ProjectListing, String> {
    let (manifest_path, project_dir) = resolve_paths(&path)?;
    let source = PathBuf::from(&source_file);
    if !source.is_file() {
        return Err(format!("source file not found: {source_file}"));
    }

    let dir = listing_dir(&project_dir);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let ext = icon_extension(&source);
    let dest_rel = format!("{LISTING_DIR_REL}/icon.{ext}");
    let dest = project_dir.join(&dest_rel);
    fs::copy(&source, &dest).map_err(|e| e.to_string())?;

    let mut listing = {
        let manifest = ProjectManifest::load_from_path(&manifest_path).map_err(|e| e.to_string())?;
        seed_listing(&manifest)
    };
    listing.icon_path = Some(dest_rel.replace('\\', "/"));
    let updated = save_listing(&manifest_path, listing)?;
    crate::helpers::invalidate_recent_home_cache(&path);
    Ok(updated)
}

#[tauri::command(rename_all = "camelCase")]
pub fn clear_project_listing_icon(path: String) -> Result<ProjectListing, String> {
    let (manifest_path, project_dir) = resolve_paths(&path)?;
    let mut listing = {
        let manifest = ProjectManifest::load_from_path(&manifest_path).map_err(|e| e.to_string())?;
        seed_listing(&manifest)
    };
    if let Some(rel) = listing.icon_path.take() {
        let abs = project_dir.join(&rel);
        let _ = fs::remove_file(abs);
    }
    let updated = save_listing(&manifest_path, listing)?;
    crate::helpers::invalidate_recent_home_cache(&path);
    Ok(updated)
}

#[tauri::command(rename_all = "camelCase")]
pub fn add_listing_gallery_image(
    path: String,
    source_file: Option<String>,
    url: Option<String>,
    caption: Option<String>,
) -> Result<ProjectListing, String> {
    let (manifest_path, project_dir) = resolve_paths(&path)?;
    let mut listing = {
        let manifest = ProjectManifest::load_from_path(&manifest_path).map_err(|e| e.to_string())?;
        seed_listing(&manifest)
    };

    let item = if let Some(src) = source_file {
        let source = PathBuf::from(&src);
        if !source.is_file() {
            return Err(format!("source file not found: {src}"));
        }
        let gdir = gallery_dir(&project_dir);
        fs::create_dir_all(&gdir).map_err(|e| e.to_string())?;
        let ext = icon_extension(&source);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let name = format!("{stamp}.{ext}");
        let dest = gdir.join(&name);
        fs::copy(&source, &dest).map_err(|e| e.to_string())?;
        let rel = format!("{GALLERY_DIR_REL}/{name}").replace('\\', "/");
        ListingGalleryItem {
            path: Some(rel),
            url: None,
            caption,
        }
    } else if let Some(u) = url {
        let trimmed = u.trim().to_string();
        if trimmed.is_empty() {
            return Err("url is empty".into());
        }
        ListingGalleryItem {
            path: None,
            url: Some(trimmed),
            caption,
        }
    } else {
        return Err("provide sourceFile or url".into());
    };

    listing.gallery.push(item);
    save_listing(&manifest_path, listing)
}

#[tauri::command(rename_all = "camelCase")]
pub fn remove_listing_gallery_image(path: String, index: usize) -> Result<ProjectListing, String> {
    let (manifest_path, project_dir) = resolve_paths(&path)?;
    let mut listing = {
        let manifest = ProjectManifest::load_from_path(&manifest_path).map_err(|e| e.to_string())?;
        seed_listing(&manifest)
    };
    if index >= listing.gallery.len() {
        return Err(format!("gallery index out of range: {index}"));
    }
    let removed = listing.gallery.remove(index);
    if let Some(rel) = removed.path {
        let abs = project_dir.join(&rel);
        let _ = fs::remove_file(abs);
    }
    save_listing(&manifest_path, listing)
}

#[tauri::command(rename_all = "camelCase")]
pub fn reorder_listing_gallery(
    path: String,
    from: usize,
    to: usize,
) -> Result<ProjectListing, String> {
    let (manifest_path, _) = resolve_paths(&path)?;
    let mut listing = {
        let manifest = ProjectManifest::load_from_path(&manifest_path).map_err(|e| e.to_string())?;
        seed_listing(&manifest)
    };
    let len = listing.gallery.len();
    if from >= len || to >= len {
        return Err("gallery reorder index out of range".into());
    }
    let item = listing.gallery.remove(from);
    listing.gallery.insert(to, item);
    save_listing(&manifest_path, listing)
}

#[tauri::command(rename_all = "camelCase")]
pub fn read_listing_asset(path: String, relative_path: String) -> Result<String, String> {
    let (_, project_dir) = resolve_paths(&path)?;
    let rel = relative_path.replace('\\', "/");
    if rel.contains("..") || Path::new(&rel).is_absolute() {
        return Err("invalid relative path".into());
    }
    if !rel.starts_with(".tuffbox/listing/") {
        return Err("asset must be under .tuffbox/listing/".into());
    }
    let abs = project_dir.join(&rel);
    let bytes = fs::read(&abs).map_err(|e| e.to_string())?;
    Ok(bytes_to_data_url(&bytes, &abs))
}

/// Best-effort listing icon → data URL for home bootstrap / sidebar cache.
pub(crate) fn try_read_listing_icon_data_url(path: &str) -> Option<String> {
    let (manifest_path, project_dir) = resolve_paths(path).ok()?;
    let manifest = ProjectManifest::load_from_path(&manifest_path).ok()?;
    let listing = seed_listing(&manifest);
    let rel = listing.icon_path.as_ref()?;
    let rel = rel.replace('\\', "/");
    if rel.contains("..") || !rel.starts_with(".tuffbox/listing/") {
        return None;
    }
    let abs = project_dir.join(&rel);
    let bytes = fs::read(&abs).ok()?;
    Some(bytes_to_data_url(&bytes, &abs))
}

#[tauri::command(rename_all = "camelCase")]
pub fn ensure_listing_folder(path: String) -> Result<String, String> {
    let (_, project_dir) = resolve_paths(&path)?;
    let dir = listing_dir(&project_dir);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(gallery_dir(&project_dir)).map_err(|e| e.to_string())?;
    Ok(dir.to_string_lossy().to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub fn update_project_brief_and_listing(
    path: String,
    brief: PackBrief,
    listing: ProjectListing,
) -> Result<(), String> {
    let (manifest_path, _) = resolve_paths(&path)?;
    auto_snapshot(&manifest_path, "update-listing").map_err(|e| e.to_string())?;
    let mut manifest =
        ProjectManifest::load_from_path(&manifest_path).map_err(|e| e.to_string())?;
    apply_listing_to_project(&mut manifest, &listing);
    manifest.listing = Some(listing);
    manifest.brief = Some(brief);
    save_manifest(&manifest_path, &manifest).map_err(|e| e.to_string())
}

/// Write clipboard / dropped image bytes into gallery (base64 payload, no data: prefix required).
#[tauri::command(rename_all = "camelCase")]
pub fn add_listing_gallery_bytes(
    path: String,
    bytes_base64: String,
    extension: Option<String>,
    caption: Option<String>,
) -> Result<ProjectListing, String> {
    let (manifest_path, project_dir) = resolve_paths(&path)?;
    let raw = STANDARD
        .decode(bytes_base64.trim())
        .map_err(|e| format!("invalid base64: {e}"))?;
    if raw.is_empty() {
        return Err("empty image bytes".into());
    }
    let ext = extension
        .as_deref()
        .map(|e| e.trim().trim_start_matches('.').to_ascii_lowercase())
        .filter(|e| matches!(e.as_str(), "png" | "jpg" | "jpeg" | "webp" | "gif"))
        .unwrap_or_else(|| "png".to_string());
    let gdir = gallery_dir(&project_dir);
    fs::create_dir_all(&gdir).map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let name = format!("{stamp}.{ext}");
    let dest = gdir.join(&name);
    let mut file = fs::File::create(&dest).map_err(|e| e.to_string())?;
    file.write_all(&raw).map_err(|e| e.to_string())?;

    let mut listing = {
        let manifest = ProjectManifest::load_from_path(&manifest_path).map_err(|e| e.to_string())?;
        seed_listing(&manifest)
    };
    let rel = format!("{GALLERY_DIR_REL}/{name}").replace('\\', "/");
    listing.gallery.push(ListingGalleryItem {
        path: Some(rel),
        url: None,
        caption,
    });
    save_listing(&manifest_path, listing)
}
