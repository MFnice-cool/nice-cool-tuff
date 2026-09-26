//! Storefront listing helpers (icon / gallery paths under `.tuffbox/listing`).

use std::path::{Path, PathBuf};

use crate::manifest::{ProjectListing, ProjectManifest};

pub const LISTING_DIR_REL: &str = ".tuffbox/listing";
pub const GALLERY_DIR_REL: &str = ".tuffbox/listing/gallery";

pub fn listing_dir(project_dir: &Path) -> PathBuf {
    project_dir.join(".tuffbox").join("listing")
}

pub fn gallery_dir(project_dir: &Path) -> PathBuf {
    listing_dir(project_dir).join("gallery")
}

pub fn resolve_listing_icon(project_dir: &Path, listing: &ProjectListing) -> Option<PathBuf> {
    let rel = listing.icon_path.as_ref()?;
    let path = if Path::new(rel).is_absolute() {
        PathBuf::from(rel)
    } else {
        project_dir.join(rel)
    };
    if path.is_file() {
        Some(path)
    } else {
        None
    }
}

pub fn pack_summary(manifest: &ProjectManifest) -> Option<String> {
    manifest
        .listing
        .as_ref()
        .map(|l| l.summary.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            manifest
                .project
                .description
                .as_ref()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
}

/// Prefer image extensions; default to png for unknown types.
pub fn icon_extension(source: &Path) -> &'static str {
    match source
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("jpg") | Some("jpeg") => "jpg",
        Some("webp") => "webp",
        Some("gif") => "gif",
        Some("png") => "png",
        _ => "png",
    }
}

pub fn mime_for_ext(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "png" => "image/png",
        _ => "application/octet-stream",
    }
}
