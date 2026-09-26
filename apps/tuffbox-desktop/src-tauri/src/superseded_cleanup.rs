//! Superseded-jar cleanup shared by the mod-update flows.
//!
//! Core extracted from `lib.rs::remove_superseded_mod_files` so the rename /
//! `.disabled` / same-hash edge cases are unit-testable without a Tauri
//! runtime. Behavior mirrors the original scanner; the thin lib.rs wrapper
//! resolves the instance/content dirs and maps `ModSpec` onto [`SupersededOld`].

use std::path::{Path, PathBuf};

/// Minimal view of the mod whose artifacts are being replaced. Decoupled from
/// `tuffbox_core::ModSpec` so tests don't need a full manifest entry.
pub(crate) struct SupersededOld<'a> {
    /// Slug/id of the updated mod; lowercase `_`→`-` forms match filename prefixes.
    pub id: &'a str,
    /// Base file name (without any `.disabled` suffix) of the jar being replaced.
    pub file_name: Option<&'a str>,
    /// Expected SHA-1 (hex) of the pre-update artifact. An empty string means
    /// "unknown", exactly like the original inline implementation.
    pub sha1: Option<&'a str>,
}

/// Directory-entry name → candidate base name worth considering for removal:
/// strips a trailing `.disabled` and keeps only `.jar`/`.zip` payloads.
fn candidate_base(name: &str) -> Option<&str> {
    let base = name.strip_suffix(".disabled").unwrap_or(name);
    if base.ends_with(".jar") || base.ends_with(".zip") {
        Some(base)
    } else {
        None
    }
}

/// Leftover-jar rule: filename starts with the mod slug (case-insensitive,
/// `_` normalized to `-`).
fn slug_prefix_matches(id: &str, base: &str) -> bool {
    let id = id.to_lowercase().replace('_', "-");
    if id.is_empty() {
        return false;
    }
    let base_l = base.to_lowercase();
    base_l.starts_with(&id) || base_l.starts_with(&format!("{id}-"))
}

/// Scans `content_dir` and deletes every jar/zip superseded by the update to
/// `keep_name`: the previous filename, any file whose sha1 still matches the
/// pre-update artifact, and leftovers sharing the mod slug prefix. Returns
/// the paths actually removed.
pub(crate) fn remove_superseded_in_dir(
    content_dir: &Path,
    old: &SupersededOld<'_>,
    keep_name: Option<&str>,
    tracked_bases: &std::collections::HashSet<String>,
) -> Vec<PathBuf> {
    // Empty stored hash must behave as "unknown", never as a real expectation.
    let old_sha1 = old.sha1.filter(|h| !h.is_empty());
    let Ok(entries) = std::fs::read_dir(content_dir) else {
        return Vec::new();
    };
    let mut removed = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let Some(base) = candidate_base(&name) else {
            continue;
        };
        // The freshly installed artifact is never its own victim.
        if keep_name == Some(base) {
            continue;
        }

        let mut remove = old.file_name == Some(base);
        if !remove {
            if let Some(expected) = old_sha1 {
                if let Ok(actual) = tuffbox_core::sha1_file(&path) {
                    if actual.eq_ignore_ascii_case(expected) {
                        remove = true;
                    }
                }
            }
        }
        // Also drop leftover jars that share the mod slug as a filename prefix
        // (e.g. sodium-fabric-0.5.0.jar after updating to sodium-fabric-0.5.8.jar).
        // Unreachable for keep_name thanks to the guard above.
        // Skip files tracked by other mods (e.g. sodium-extra must not be removed
        // when updating sodium).
        if !remove && slug_prefix_matches(old.id, base) && !tracked_bases.contains(base) {
            remove = true;
        }
        if remove {
            let _ = std::fs::remove_file(&path);
            removed.push(path);
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_mods() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let mods = dir.path().join("mods");
        fs::create_dir_all(&mods).unwrap();
        (dir, mods)
    }

    fn write(dir: &Path, name: &str, contents: &[u8]) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, contents).unwrap();
        path
    }

    fn sha(path: &Path) -> String {
        tuffbox_core::sha1_file(path).unwrap()
    }

    fn sorted_names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// Acceptance (a): old jar renamed to `.disabled` is removed when the new
    /// version lands. Mirrors how `commit_single_mod_update` hands over the
    /// stripped base name plus the on-disk hash of the disabled artifact.
    #[test]
    fn renamed_disabled_old_jar_removed_on_new_version() {
        let (_tmp, mods) = temp_mods();
        let old_path = write(&mods, "mymod-1.0.jar.disabled", b"old payload");
        write(&mods, "mymod-1.2.jar", b"new payload");
        let hash = sha(&old_path);
        let old = SupersededOld {
            id: "mymod",
            file_name: Some("mymod-1.0.jar"),
            sha1: Some(hash.as_str()),
        };

        let removed = remove_superseded_in_dir(&mods, &old, Some("mymod-1.2.jar"), &std::collections::HashSet::new());

        assert_eq!(removed, vec![mods.join("mymod-1.0.jar.disabled")]);
        assert_eq!(sorted_names(&mods), ["mymod-1.2.jar"]);
    }

    /// Acceptance (b): a `.disabled` leftover orphaned by a crashed previous
    /// update cycle is cleaned up alongside the current replacement.
    #[test]
    fn stale_disabled_leftover_from_previous_cycle_cleaned() {
        let (_tmp, mods) = temp_mods();
        write(&mods, "mymod-0.9.jar.disabled", b"ancient");
        let current = write(&mods, "mymod-1.0.jar", b"current");
        write(&mods, "mymod-1.2.jar", b"new");
        let hash = sha(&current);
        let old = SupersededOld {
            id: "mymod",
            file_name: Some("mymod-1.0.jar"),
            sha1: Some(hash.as_str()),
        };

        let removed = remove_superseded_in_dir(&mods, &old, Some("mymod-1.2.jar"), &std::collections::HashSet::new());

        assert_eq!(removed.len(), 2, "old jar and stale leftover both go");
        assert_eq!(sorted_names(&mods), ["mymod-1.2.jar"]);
    }

    /// Acceptance (c): same-hash old+new under the same name is a no-op —
    /// nothing is deleted.
    #[test]
    fn same_hash_no_op_deletes_nothing() {
        let (_tmp, mods) = temp_mods();
        let jar = write(&mods, "mymod-1.0.jar", b"identical payload");
        let hash = sha(&jar);
        let old = SupersededOld {
            id: "mymod",
            file_name: Some("mymod-1.0.jar"),
            sha1: Some(hash.as_str()),
        };

        let removed = remove_superseded_in_dir(&mods, &old, Some("mymod-1.0.jar"), &std::collections::HashSet::new());

        assert!(removed.is_empty());
        assert_eq!(sorted_names(&mods), ["mymod-1.0.jar"]);
    }

    /// Acceptance (d): unrelated jars, zips and non-payload files survive
    /// cleanup untouched.
    #[test]
    fn unrelated_files_untouched() {
        let (_tmp, mods) = temp_mods();
        write(&mods, "mymod-1.0.jar.disabled", b"stale");
        write(&mods, "mymod-1.1.jar", b"new");
        write(&mods, "jei-1.20.1-19.21.0.jar", b"jei");
        write(&mods, "create-forge-6.0.4.jar", b"create");
        write(&mods, "resourcepack.zip", b"zipped");
        write(&mods, "pack.png", b"\x89PNG");
        write(&mods, "README.txt", b"hi");
        // Hash deliberately matches nothing on disk.
        let old = SupersededOld {
            id: "mymod",
            file_name: Some("mymod-1.0.jar"),
            sha1: Some("deadbeef"),
        };

        let removed = remove_superseded_in_dir(&mods, &old, Some("mymod-1.1.jar"), &std::collections::HashSet::new());

        assert_eq!(removed, vec![mods.join("mymod-1.0.jar.disabled")]);
        assert_eq!(
            sorted_names(&mods),
            [
                "README.txt",
                "create-forge-6.0.4.jar",
                "jei-1.20.1-19.21.0.jar",
                "mymod-1.1.jar",
                "pack.png",
                "resourcepack.zip",
            ]
        );
    }

    /// Acceptance (e): cleanup is scoped to the updated mod's file names —
    /// another mod's current and disabled jars stay even in the same folder.
    #[test]
    fn cleanup_scoped_to_updated_mod_only() {
        let (_tmp, mods) = temp_mods();
        write(&mods, "alpha-1.0.jar", b"alpha old");
        write(&mods, "alpha-2.0.jar", b"alpha new");
        write(&mods, "beta-1.0.jar", b"beta current");
        write(&mods, "beta-0.9.jar.disabled", b"beta stale");
        let old = SupersededOld {
            id: "alpha",
            file_name: Some("alpha-1.0.jar"),
            sha1: None,
        };

        let removed = remove_superseded_in_dir(&mods, &old, Some("alpha-2.0.jar"), &std::collections::HashSet::new());

        assert_eq!(removed, vec![mods.join("alpha-1.0.jar")]);
        assert_eq!(
            sorted_names(&mods),
            ["alpha-2.0.jar", "beta-0.9.jar.disabled", "beta-1.0.jar"]
        );
    }

    /// Doc-commented purpose of the hash rule: when the manifest's recorded
    /// file name went stale (Modrinth renamed the artifact), the old jar is
    /// still found — purely by sha1, isolated from the slug-prefix rule.
    #[test]
    fn old_artifact_found_by_hash_alone_when_name_went_stale() {
        let (_tmp, mods) = temp_mods();
        let legacy = write(&mods, "legacy-renderer-0.5.0.jar", b"renderer bits");
        write(&mods, "renderer-next-0.6.0.jar", b"renderer next");
        // Recorded name matches nothing on disk; hash (uppercased to also pin
        // case-insensitive comparison) points at the legacy file.
        let hash = sha(&legacy).to_uppercase();
        let old = SupersededOld {
            id: "renderer-next",
            file_name: Some("renderer-0.5.0.jar"),
            sha1: Some(hash.as_str()),
        };

        let removed =
            remove_superseded_in_dir(&mods, &old, Some("renderer-next-0.6.0.jar"), &std::collections::HashSet::new());

        assert_eq!(removed, vec![mods.join("legacy-renderer-0.5.0.jar")]);
        assert_eq!(sorted_names(&mods), ["renderer-next-0.6.0.jar"]);
    }

    /// Empty stored hash means "unknown": it must never wipe arbitrary jars.
    #[test]
    fn empty_stored_hash_treated_as_unknown() {
        let (_tmp, mods) = temp_mods();
        write(&mods, "gamma-2.0.jar", b"whatever");
        let old = SupersededOld {
            id: "delta",
            file_name: None,
            sha1: Some(""),
        };

        let removed = remove_superseded_in_dir(&mods, &old, None, &std::collections::HashSet::new());

        assert!(removed.is_empty());
        assert_eq!(sorted_names(&mods), ["gamma-2.0.jar"]);
    }

    #[test]
    fn candidate_base_rules_match_original_scanner() {
        assert_eq!(candidate_base("sodium-0.5.jar"), Some("sodium-0.5.jar"));
        assert_eq!(
            candidate_base("sodium-0.5.jar.disabled"),
            Some("sodium-0.5.jar")
        );
        assert_eq!(candidate_base("pack.zip.disabled"), Some("pack.zip"));
        assert_eq!(candidate_base("notes.txt"), None);
        assert_eq!(candidate_base("pack.png"), None);
        // Case-sensitive, exactly like the original ends_with checks.
        assert_eq!(candidate_base("PACK.JAR"), None);
    }

    #[test]
    fn slug_prefix_rule_normalizes_case_and_underscores() {
        assert!(slug_prefix_matches(
            "Sodium_Fabric",
            "sodium-fabric-0.5.0.jar"
        ));
        assert!(slug_prefix_matches("jei", "JEI-1.20.1.jar"));
        assert!(!slug_prefix_matches("", "anything.jar"));
        assert!(!slug_prefix_matches("jei", "forge-jei-1.0.jar"));
    }
}
