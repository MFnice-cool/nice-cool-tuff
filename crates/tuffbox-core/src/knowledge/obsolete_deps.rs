//! Obsolete (no-longer-required) dependency knowledge.
//!
//! Mods evolve: a hard dependency declared in an older release's
//! `fabric.mod.json` can become unnecessary once the mod bundles the feature
//! itself. Diagnostics must not nag users to install deps that modern versions
//! of the requester no longer need — otherwise Health shows noise like
//! "Indium is missing" next to a Sodium 0.6+ install that ships the Fabric
//! Rendering API support built in.

/// A dependency that a requester stopped requiring at some version.
struct ObsoleteDependency {
    requester: &'static str,
    dependency: &'static str,
    /// Requester version that stopped needing the dep (semver prefix match).
    since_requester_version: &'static str,
    /// Human note surfaced in logs when a stale edge is suppressed.
    reason: &'static str,
}

const OBSOLETE_DEPENDENCIES: &[ObsoleteDependency] = &[ObsoleteDependency {
    requester: "sodium",
    dependency: "indium",
    // Sodium 0.6.0+ implements the Fabric Rendering API natively.
    since_requester_version: "0.6",
    reason: "Sodium 0.6+ has built-in Fabric Rendering API support; Indium is no longer needed",
}];

/// Compare dotted versions: `a >= b` on the shared numeric prefix.
/// "0.6.3" >= "0.6", "0.5.8" < "0.6", "1.21.4" >= "1.21".
fn version_at_least(version: &str, minimum: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> {
        s.split(|c: char| !c.is_ascii_digit())
            .filter(|p| !p.is_empty())
            .filter_map(|p| p.parse::<u64>().ok())
            .collect()
    };
    let v = parse(version);
    let m = parse(minimum);
    if m.is_empty() {
        return true;
    }
    for i in 0..m.len() {
        let a = v.get(i).copied().unwrap_or(0);
        let b = m[i];
        if a != b {
            return a > b;
        }
    }
    true
}

fn normalize(id: &str) -> String {
    id.trim().to_ascii_lowercase().replace('_', "-")
}

/// True when `requester version` no longer needs `dependency` because the
/// requester bundles it. Unknown requesters/versions return false (keep the
/// diagnostic — only confidently-obsolete edges are suppressed).
pub fn is_obsolete_dependency(
    requester: &str,
    requester_version: Option<&str>,
    dependency: &str,
) -> bool {
    let dep = normalize(dependency);
    for entry in OBSOLETE_DEPENDENCIES {
        if normalize(entry.requester) != normalize(requester) || normalize(entry.dependency) != dep
        {
            continue;
        }
        match requester_version {
            // No version info: suppress only when the dep is fully obsolete for
            // every shipped version of the requester (empty `since` semantics
            // are not used by the current table).
            None => {}
            Some(v) => {
                if version_at_least(v, entry.since_requester_version) {
                    return true;
                }
            }
        }
    }
    false
}

/// Log-worthy reason for suppression (for debug output).
pub fn obsolete_dependency_reason(requester: &str, dependency: &str) -> Option<&'static str> {
    let dep = normalize(dependency);
    OBSOLETE_DEPENDENCIES
        .iter()
        .find(|e| normalize(e.requester) == normalize(requester) && normalize(e.dependency) == dep)
        .map(|e| e.reason)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sodium_indium_obsolete_from_0_6() {
        assert!(is_obsolete_dependency("sodium", Some("0.6.0"), "indium"));
        assert!(is_obsolete_dependency("sodium", Some("0.6.13"), "Indium"));
        assert!(is_obsolete_dependency(
            "Sodium",
            Some("1.21.4-0.6.4"),
            "indium"
        ));
    }

    #[test]
    fn old_sodium_still_needs_indium() {
        assert!(!is_obsolete_dependency("sodium", Some("0.5.8"), "indium"));
        assert!(!is_obsolete_dependency("sodium", Some("0.5.0"), "indium"));
    }

    #[test]
    fn unknown_version_keeps_diagnostic() {
        assert!(!is_obsolete_dependency("sodium", None, "indium"));
        assert!(!is_obsolete_dependency("sodium", Some("unknown"), "indium"));
    }

    #[test]
    fn sodium_06_version_strings_from_modrinth_are_recognized() {
        // Modrinth version numbers for Sodium 0.6+ look like "mc1.21.4-0.6.4-fabric"
        // or "0.6.13+mc1.21.4" — the numeric prefix parse must not choke on the
        // leading "mc" segment and still recognize the dependency as obsolete.
        assert!(is_obsolete_dependency(
            "sodium",
            Some("0.6.0+mc1.21.1"),
            "indium"
        ));
    }

    #[test]
    fn unrelated_deps_untouched() {
        assert!(!is_obsolete_dependency(
            "sodium",
            Some("0.6.0"),
            "fabric-api"
        ));
        assert!(!is_obsolete_dependency("iris", Some("1.8.0"), "sodium"));
    }

    #[test]
    fn version_compare_prefixes() {
        assert!(version_at_least("0.6.3", "0.6"));
        assert!(version_at_least("0.6", "0.6"));
        assert!(!version_at_least("0.5.8", "0.6"));
        assert!(version_at_least("1.21.4", "1.21"));
        assert!(version_at_least("2.0", "0.6"));
    }
}
