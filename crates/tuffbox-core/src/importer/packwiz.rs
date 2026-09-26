//! packwiz pack import with integrity verification (`pack.toml` + `index.toml`).
//!
//! Handles pack directories and zipped packs. Every index-listed file is
//! hash-checked (sha256/sha512/sha1, per the packwiz index) before any download
//! URL is trusted; a mismatch aborts the import with the offending path.
//! The `overrides/` folder is materialized into the instance game dir at apply
//! time via [`extract_packwiz_overrides`], mirroring the CurseForge overrides
//! handling. Manifest construction (metafile TOML → [`ModSpec`]) is delegated
//! to [`crate::packwiz::import_packwiz_pack`].

use super::ImportError;
use crate::manifest::ProjectManifest;
use serde::Deserialize;
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha512};
use std::{fs, io::Read, path::Path};

/// Zip-extraction limits, mirroring the github_pack guards.
const MAX_FILES: usize = 20_000;
const MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Deserialize)]
struct PackTomlVerify {
    index: IndexRefVerify,
}

#[derive(Debug, Deserialize)]
struct IndexRefVerify {
    #[serde(default = "default_index_file")]
    file: String,
    #[serde(default, rename = "hash-format")]
    hash_format: Option<String>,
    #[serde(default)]
    hash: Option<String>,
}

fn default_index_file() -> String {
    "index.toml".into()
}

#[derive(Debug, Deserialize)]
struct IndexTomlVerify {
    #[serde(default, rename = "hash-format")]
    hash_format: Option<String>,
    #[serde(default)]
    files: Vec<IndexEntryVerify>,
}

#[derive(Debug, Deserialize)]
struct IndexEntryVerify {
    file: String,
    #[serde(default, rename = "hash-format")]
    hash_format: Option<String>,
    #[serde(default)]
    hash: Option<String>,
}

/// True when `dir` contains a packwiz `pack.toml`.
pub fn detect_packwiz_dir(dir: impl AsRef<Path>) -> bool {
    crate::packwiz::is_packwiz_pack(dir)
}

/// True when the zip is a packed packwiz repo (`pack.toml` at the archive root).
pub fn detect_packwiz_zip(zip_path: impl AsRef<Path>) -> bool {
    let Ok(file) = fs::File::open(zip_path) else {
        return false;
    };
    let Ok(mut archive) = zip::ZipArchive::new(file) else {
        return false;
    };
    (0..archive.len()).any(|i| {
        archive
            .by_index(i)
            .map(|e| zip_entry_name(e.name()) == "pack.toml")
            .unwrap_or(false)
    })
}

fn zip_entry_name(raw: &str) -> String {
    raw.replace('\\', "/").trim_start_matches("./").to_string()
}

/// Import a packwiz pack (directory or zip) into a [`ProjectManifest`].
///
/// Verifies the index hash from `pack.toml` and every index entry hash before
/// building the manifest — a mismatch aborts with [`ImportError::PackwizHashMismatch`].
pub fn import_packwiz(path: impl AsRef<Path>) -> Result<ProjectManifest, ImportError> {
    let path = path.as_ref();
    if path.is_dir() {
        import_packwiz_dir(path)
    } else {
        // Zipped pack: extract once into a temp dir and reuse the directory
        // path — verification, metafile parsing and overrides all need a tree.
        let tmp = tempfile::tempdir()?;
        extract_zip_tree(path, tmp.path())?;
        import_packwiz_dir(tmp.path())
    }
}

fn import_packwiz_dir(root: &Path) -> Result<ProjectManifest, ImportError> {
    let pack_raw = fs::read_to_string(root.join("pack.toml"))
        .map_err(|e| ImportError::Packwiz(format!("cannot read pack.toml: {e}")))?;
    let pack: PackTomlVerify = toml::from_str(&pack_raw)
        .map_err(|e| ImportError::Packwiz(format!("invalid pack.toml: {e}")))?;

    let index_rel = safe_rel(&pack.index.file)?;
    let index_path = root.join(&index_rel);
    let index_raw = fs::read_to_string(&index_path)
        .map_err(|e| ImportError::Packwiz(format!("cannot read {}: {e}", pack.index.file)))?;

    // 1. pack.toml vouches for index.toml.
    verify_hash(
        index_raw.as_bytes(),
        pack.index.hash_format.as_deref().unwrap_or("sha256"),
        pack.index.hash.as_deref(),
        &pack.index.file,
    )?;

    // 2. index.toml vouches for every listed file (metafiles + loose files).
    let index: IndexTomlVerify = toml::from_str(&index_raw)
        .map_err(|e| ImportError::Packwiz(format!("invalid {}: {e}", pack.index.file)))?;
    let index_dir = index_path.parent().unwrap_or(root).to_path_buf();
    for entry in &index.files {
        if entry.file.starts_with("overrides/") {
            continue; // never listed by packwiz refresh; ignore defensively
        }
        let rel = safe_rel(&entry.file)?;
        let bytes = fs::read(index_dir.join(&rel)).map_err(|e| {
            ImportError::Packwiz(format!("index lists missing file {}: {e}", entry.file))
        })?;
        verify_hash(
            &bytes,
            entry
                .hash_format
                .as_deref()
                .or(index.hash_format.as_deref())
                .unwrap_or("sha256"),
            entry.hash.as_deref(),
            &entry.file,
        )?;
    }

    // 3. Verified — build the manifest (parses metafile TOMLs into ModSpecs).
    crate::packwiz::import_packwiz_pack(root).map_err(|e| ImportError::Packwiz(e.to_string()))
}

fn verify_hash(
    bytes: &[u8],
    format: &str,
    expected: Option<&str>,
    display_path: &str,
) -> Result<(), ImportError> {
    let Some(expected) = expected.map(str::trim).filter(|h| !h.is_empty()) else {
        return Ok(()); // no recorded hash — nothing to verify (packwiz allows this)
    };
    let actual = match format {
        "sha256" => hex::encode(Sha256::digest(bytes)),
        "sha512" => hex::encode(Sha512::digest(bytes)),
        "sha1" => hex::encode(Sha1::digest(bytes)),
        other => {
            return Err(ImportError::Packwiz(format!(
                "unsupported hash format {other:?} for {display_path}",
            )));
        }
    };
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(ImportError::PackwizHashMismatch(display_path.to_string()));
    }
    Ok(())
}

fn safe_rel(rel: &str) -> Result<String, ImportError> {
    crate::github_pack::safe_relative_path(rel)
        .map_err(|e| ImportError::Packwiz(format!("unsafe path: {e}")))
}

fn extract_zip_tree(zip_path: &Path, dest: &Path) -> Result<(), ImportError> {
    let file = fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    fs::create_dir_all(dest)?;
    let mut files = 0usize;
    let mut total = 0u64;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = zip_entry_name(entry.name());
        if name.is_empty() {
            continue;
        }
        let safe = safe_rel(&name)?;
        let out = dest.join(&safe);
        if entry.is_dir() {
            fs::create_dir_all(&out)?;
            continue;
        }
        files += 1;
        if files > MAX_FILES {
            return Err(ImportError::Packwiz("zip has too many files".into()));
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        total += bytes.len() as u64;
        if total > MAX_TOTAL_BYTES {
            return Err(ImportError::Packwiz("zip exceeds size limit".into()));
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&out, &bytes)?;
    }
    Ok(())
}

/// Copy the packwiz `overrides/` folder into the instance game dir.
/// Accepts the same source shapes as [`import_packwiz`] (directory or zip).
/// Returns the number of files copied.
pub fn extract_packwiz_overrides(
    pack_path: impl AsRef<Path>,
    instance_dir: impl AsRef<Path>,
) -> Result<usize, ImportError> {
    let pack_path = pack_path.as_ref();
    let instance_dir = instance_dir.as_ref();
    if pack_path.is_dir() {
        copy_overrides_tree(&pack_path.join("overrides"), instance_dir)
    } else {
        extract_zip_overrides(pack_path, instance_dir)
    }
}

fn copy_overrides_tree(overrides_dir: &Path, instance_dir: &Path) -> Result<usize, ImportError> {
    if !overrides_dir.is_dir() {
        return Ok(0);
    }
    let mut count = 0usize;
    copy_overrides_walk(overrides_dir, overrides_dir, instance_dir, &mut count)?;
    Ok(count)
}

fn copy_overrides_walk(
    root: &Path,
    dir: &Path,
    instance_dir: &Path,
    count: &mut usize,
) -> Result<(), ImportError> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            copy_overrides_walk(root, &path, instance_dir, count)?;
        } else {
            let rel = path
                .strip_prefix(root)
                .map_err(|_| ImportError::Packwiz("override path escape".into()))?
                .to_string_lossy()
                .replace('\\', "/");
            let safe = safe_rel(&rel)?;
            let dest = instance_dir.join(&safe);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&path, &dest)?;
            *count += 1;
        }
    }
    Ok(())
}

fn extract_zip_overrides(zip_path: &Path, instance_dir: &Path) -> Result<usize, ImportError> {
    let file = fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    let mut count = 0usize;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = zip_entry_name(entry.name());
        let Some(rel) = name.strip_prefix("overrides/") else {
            continue;
        };
        if rel.is_empty() || entry.is_dir() {
            continue;
        }
        let safe = safe_rel(rel)?;
        let dest = instance_dir.join(&safe);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        fs::write(&dest, &bytes)?;
        count += 1;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{LoaderKind, Side, SourceKind};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Tests run in parallel — each fixture gets its own tree.
    static FIXTURE_SEQ: AtomicUsize = AtomicUsize::new(0);

    /// Write a valid packwiz fixture: client-only mod, modrinth-updated mod,
    /// one overrides file. Returns the temp root.
    fn write_fixture() -> PathBuf {
        let seq = FIXTURE_SEQ.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!(
            "tuffbox_packwiz_importer_test_{}_{seq}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("mods")).unwrap();
        fs::create_dir_all(root.join("overrides").join("config")).unwrap();

        let client_mod = r#"
name = "Client Only"
filename = "client-only.jar"
side = "client"

[download]
url = "https://example.com/client-only.jar"
hash-format = "sha256"
hash = "aaaa"
"#;
        let engine_mod = r#"
name = "Engine Mod"
filename = "engine-mod.jar"
side = "both"

[download]
mode = "metadata:modrinth"
hash-format = "sha512"
hash = "bbbb"

[update.modrinth]
mod-id = "AANobbMI"
version = "mc1.20.1-0.5.8"
"#;
        fs::write(root.join("mods/client_only.pw.toml"), client_mod).unwrap();
        fs::write(root.join("mods/engine_mod.pw.toml"), engine_mod).unwrap();
        fs::write(root.join("overrides/config/example.cfg"), "key=value\n").unwrap();

        let sha256_hex = |bytes: &[u8]| hex::encode(Sha256::digest(bytes));
        let index_toml = format!(
            "hash-format = \"sha256\"\n\n[[files]]\nfile = \"mods/client_only.pw.toml\"\nhash = \"{}\"\nmetafile = true\n\n[[files]]\nfile = \"mods/engine_mod.pw.toml\"\nhash = \"{}\"\nmetafile = true\n",
            sha256_hex(client_mod.as_bytes()),
            sha256_hex(engine_mod.as_bytes()),
        );
        fs::write(root.join("index.toml"), &index_toml).unwrap();

        let pack_toml = format!(
            "name = \"Demo Pack\"\nauthor = \"Tester\"\nversion = \"0.1.0\"\npack-format = \"packwiz:1.1.0\"\n\n[index]\nfile = \"index.toml\"\nhash-format = \"sha256\"\nhash = \"{}\"\n\n[versions]\nminecraft = \"1.20.1\"\nfabric = \"0.15.0\"\n",
            sha256_hex(index_toml.as_bytes()),
        );
        fs::write(root.join("pack.toml"), pack_toml).unwrap();
        root
    }

    fn zip_fixture(root: &Path) -> PathBuf {
        let zip_path = root.parent().unwrap().join("packwiz_fixture.zip");
        let file = fs::File::create(&zip_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        for rel in [
            "pack.toml",
            "index.toml",
            "mods/client_only.pw.toml",
            "mods/engine_mod.pw.toml",
            "overrides/config/example.cfg",
        ] {
            let bytes = fs::read(root.join(rel)).unwrap();
            zip.start_file(rel, options).unwrap();
            std::io::Write::write_all(&mut zip, &bytes).unwrap();
        }
        zip.finish().unwrap();
        zip_path
    }

    #[test]
    fn detect_and_import_dir() {
        let root = write_fixture();
        assert!(detect_packwiz_dir(&root));
        let manifest = import_packwiz(&root).unwrap();
        assert_eq!(manifest.project.name, "Demo Pack");
        assert_eq!(manifest.minecraft.version, "1.20.1");
        assert_eq!(manifest.loader.kind, LoaderKind::Fabric);
        assert_eq!(manifest.mods.len(), 2);

        let client = manifest
            .mods
            .iter()
            .find(|m| m.name == "Client Only")
            .unwrap();
        assert_eq!(client.side, Side::Client);
        assert_eq!(client.source.kind, SourceKind::Direct);
        assert_eq!(
            client.source.url.as_deref(),
            Some("https://example.com/client-only.jar")
        );
        assert_eq!(client.file_name.as_deref(), Some("client-only.jar"));
        assert!(client.hashes.is_some(), "download hash must be attached");

        let engine = manifest
            .mods
            .iter()
            .find(|m| m.name == "Engine Mod")
            .unwrap();
        assert_eq!(engine.side, Side::Both);
        assert_eq!(engine.source.kind, SourceKind::Modrinth);
        assert_eq!(engine.source.project_id.as_deref(), Some("AANobbMI"));
        assert_eq!(engine.file_name.as_deref(), Some("engine-mod.jar"));

        // overrides are not part of the manifest; they materialize at apply time
        let out = tempfile::tempdir().unwrap();
        let copied = extract_packwiz_overrides(&root, out.path()).unwrap();
        assert_eq!(copied, 1);
        assert_eq!(
            fs::read_to_string(out.path().join("config/example.cfg")).unwrap(),
            "key=value\n"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn corrupt_metafile_hash_rejected() {
        let root = write_fixture();
        // Tamper with a verified metafile after the index recorded its hash.
        let meta = root.join("mods/engine_mod.pw.toml");
        fs::write(&meta, "name = \"Engine Mod\"\nfilename = \"evil.jar\"\n").unwrap();
        let err = import_packwiz(&root).unwrap_err();
        assert!(
            matches!(&err, ImportError::PackwizHashMismatch(p) if p.contains("engine_mod.pw.toml")),
            "expected hash mismatch, got {err:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn corrupt_index_hash_in_pack_toml_rejected() {
        let root = write_fixture();
        let pack_path = root.join("pack.toml");
        let pack = fs::read_to_string(&pack_path)
            .unwrap()
            .replace("hash = \"", "hash = \"f");
        fs::write(&pack_path, pack).unwrap();
        let err = import_packwiz(&root).unwrap_err();
        assert!(
            matches!(&err, ImportError::PackwizHashMismatch(p) if p.contains("index.toml")),
            "expected index hash mismatch, got {err:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_index_entry_rejected() {
        let root = write_fixture();
        fs::remove_file(root.join("mods/client_only.pw.toml")).unwrap();
        let err = import_packwiz(&root).unwrap_err();
        assert!(
            matches!(&err, ImportError::Packwiz(msg) if msg.contains("client_only.pw.toml")),
            "expected missing-file error, got {err:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn unsafe_index_path_rejected() {
        let root = write_fixture();
        let index_path = root.join("index.toml");
        let index = fs::read_to_string(&index_path).unwrap().replace(
            "file = \"mods/client_only.pw.toml\"",
            "file = \"../evil.pw.toml\"",
        );
        fs::write(&index_path, &index).unwrap();
        // Re-sign the mutated index so the pack.toml hash check passes —
        // the safe-path guard is the remaining defense.
        let index_hash = hex::encode(Sha256::digest(index.as_bytes()));
        let pack_toml = format!(
            "name = \"Demo Pack\"\nauthor = \"Tester\"\nversion = \"0.1.0\"\npack-format = \"packwiz:1.1.0\"\n\n[index]\nfile = \"index.toml\"\nhash-format = \"sha256\"\nhash = \"{index_hash}\"\n\n[versions]\nminecraft = \"1.20.1\"\nfabric = \"0.15.0\"\n",
        );
        fs::write(root.join("pack.toml"), pack_toml).unwrap();
        let err = import_packwiz(&root).unwrap_err();
        assert!(
            matches!(&err, ImportError::Packwiz(msg) if msg.contains("unsafe path")),
            "expected unsafe-path error, got {err:?}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn detect_and_import_zip() {
        let root = write_fixture();
        let zip_path = zip_fixture(&root);
        assert!(detect_packwiz_zip(&zip_path));

        let manifest = import_packwiz(&zip_path).unwrap();
        assert_eq!(manifest.project.name, "Demo Pack");
        assert_eq!(manifest.mods.len(), 2);

        let out = tempfile::tempdir().unwrap();
        let copied = extract_packwiz_overrides(&zip_path, out.path()).unwrap();
        assert_eq!(copied, 1);
        assert!(out.path().join("config/example.cfg").is_file());

        // A mods-only zip is not a packwiz pack.
        let plain = root.parent().unwrap().join("plain.zip");
        {
            let file = fs::File::create(&plain).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            zip.start_file("some_mod.jar", zip::write::SimpleFileOptions::default())
                .unwrap();
            std::io::Write::write_all(&mut zip, b"jar bytes").unwrap();
            zip.finish().unwrap();
        }
        assert!(!detect_packwiz_zip(&plain));
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_file(&zip_path);
        let _ = fs::remove_file(&plain);
    }
}
