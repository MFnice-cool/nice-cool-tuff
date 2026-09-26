//! Parser for "conflict pair" candidates extracted from crash reports / logs.
//!
//! The resolver operates on pairs `(a, b, kind)` rather than a single ranked
//! suspect, so both sides of a "mod breaks mod" conflict get a fair shot at a
//! resolution option instead of only ever blaming the first suspect.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConflictKind {
    /// One mod breaks / is incompatible with another.
    Breaking,
    /// One mod depends on another that is missing or unloadable.
    DependsOn,
    /// A mixin target collision between two mods.
    Mixin,
    /// Two copies of the same mod.
    Duplicate,
    /// A jar built for the wrong loader / wrong Minecraft version.
    Loader,
    /// One mod is at an outdated version relative to the other.
    Outdated,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Conflict {
    /// First mod id (the "owner" / active side, whichever text names first).
    pub a: String,
    /// Second mod id.
    pub b: String,
    pub kind: ConflictKind,
    /// Human reason (also serves as a stable key for the conflict).
    pub reason: String,
}

fn normalize_token(s: &str) -> String {
    s.trim()
        .trim_matches(|c: char| c == '\'' || c == '"' || c == '`')
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .collect::<String>()
        .to_ascii_lowercase()
}

/// Extract single-quoted identifiers from a log line, e.g. `mod 'Sodium' (sodium)`.
pub fn extract_quoted_ids(line: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut rest = line;
    while let Some(pos) = rest.find('\'') {
        let after = &rest[pos + 1..];
        if let Some(end) = after.find('\'') {
            let inner = &after[..end];
            let token = normalize_token(inner);
            if !token.is_empty() && token.len() >= 2 {
                ids.push(token);
            }
            rest = &after[end + 1..];
        } else {
            break;
        }
    }
    // Dedupe, preserving order.
    let mut uniq = Vec::new();
    for id in ids {
        if !uniq.iter().any(|x| x == &id) {
            uniq.push(id);
        }
    }
    uniq
}

/// Classify a conflict log line into a ConflictKind by keywords.
fn classify_kind(line: &str) -> ConflictKind {
    let l = line.to_ascii_lowercase();
    if l.contains("mixinextras")
        || l.contains("mixin")
        || l.contains("conflicting mixin")
        || l.contains("apply mixin")
    {
        return ConflictKind::Mixin;
    }
    if l.contains("duplicate") || l.contains("two mods") || l.contains("found ourself") {
        return ConflictKind::Duplicate;
    }
    if l.contains("wrong loader")
        || l.contains("forge mod on fabric")
        || l.contains("fabric mod on forge")
        || l.contains("built for a different loader")
    {
        return ConflictKind::Loader;
    }
    if l.contains("outdated")
        || l.contains("old version")
        || l.contains("requires a newer")
        || l.contains("is missing")
    {
        if l.contains("requires a newer") || l.contains("old version") {
            return ConflictKind::Outdated;
        }
        return ConflictKind::DependsOn;
    }
    if l.contains("depends") || l.contains("requires") || l.contains("missing dependency") {
        return ConflictKind::DependsOn;
    }
    ConflictKind::Breaking
}

fn has_breakage_markers(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("is incompatible with")
        || l.contains("incompatible with")
        || l.contains("conflicts with")
        || l.contains(" breaks ")
        || l.contains("{breaks")
        || l.contains("breaks '")
        || l.contains("breaking mod")
        || (l.contains("neg_hard_dep") && l.contains("breaks"))
}

/// Parse a single log / crash line into a `Conflict` pair when it looks like a
/// "mod A is incompatible with / breaks / conflicts with mod B" statement.
pub fn parse_breakage_line(line: &str) -> Option<Conflict> {
    if !has_breakage_markers(line) {
        return None;
    }
    let kind = classify_kind(line);
    let reason = line.trim().to_string();

    // "NEG_HARD_DEP <owner> <ver> {breaks <target> @ [*]}" — owner first, target
    // inside the braces after "breaks".
    let l = line.to_ascii_lowercase();
    if l.contains("neg_hard_dep") {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        let owner = tokens
            .iter()
            .find(|t| !t.eq_ignore_ascii_case("NEG_HARD_DEP"))
            .map(|t| normalize_token(t));
        // target inside "... breaks X ..."
        let mut target = None;
        let lower_line = line.to_ascii_lowercase();
        for kw in ["breaks ", "conflicts with ", "incompatible with "] {
            if let Some(pos) = lower_line.find(kw) {
                let after = &line[pos + kw.len()..];
                let chunk = after
                    .split(|c: char| c == '{' || c == '}' || c == ';' || c == '@')
                    .next()
                    .unwrap_or("")
                    .trim();
                let t = normalize_token(chunk.split_whitespace().next().unwrap_or(""));
                if !t.is_empty() {
                    target = Some(t);
                    break;
                }
            }
        }
        if let (Some(a), Some(b)) = (owner, target) {
            if a != b && !a.is_empty() && !b.is_empty() {
                return Some(Conflict { a, b, kind, reason });
            }
        }
        return None;
    }

    // General "A <connector> B": owner = mod id before the connector, target =
    // mod id after it (whichever side, tolerating display-name noise).
    for connector in [
        "is incompatible with",
        "incompatible with",
        "conflicts with",
        "breaks",
    ] {
        let lower = line.to_ascii_lowercase();
        let Some(pos) = lower.find(connector) else {
            continue;
        };
        let before = &line[..pos];
        let after = &line[pos + connector.len()..];
        let owner = paren_ids(before)
            .into_iter()
            .next_back()
            .or_else(|| extract_quoted_ids(before).into_iter().next())
            .or_else(|| mod_token(before, TokenSide::Last));
        let target = extract_quoted_ids(after)
            .into_iter()
            .next()
            .or_else(|| paren_ids(after).into_iter().next())
            .or_else(|| mod_token(after, TokenSide::First));
        if let (Some(a), Some(b)) = (owner, target) {
            if a != b && !a.is_empty() && !b.is_empty() {
                return Some(Conflict { a, b, kind, reason });
            }
        }
    }
    None
}

enum TokenSide {
    First,
    Last,
}

/// Tokens inside parentheses that look like mod ids, e.g. `(sodium)`.
fn paren_ids(s: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut rest = s;
    while let Some(pos) = rest.find('(') {
        let after = &rest[pos + 1..];
        if let Some(end) = after.find(')') {
            let inner = &after[..end];
            let t = normalize_token(inner);
            if !t.is_empty() && t.len() >= 2 && mod_idish(&t) {
                ids.push(t);
            }
            rest = &after[end + 1..];
        } else {
            break;
        }
    }
    ids
}

/// First / last whitespace token that looks like a mod id (skipping connector
/// noise words such as "any", "version", "of", "mod", "the").
fn mod_token(s: &str, side: TokenSide) -> Option<String> {
    let mut tokens: Vec<String> = s
        .split(|c: char| c.is_whitespace() || c == '\'' || c == '"' || c == '[' || c == ']')
        .map(normalize_token)
        .filter(|t| !t.is_empty() && mod_idish(t))
        .collect();
    if tokens.is_empty() {
        return None;
    }
    match side {
        TokenSide::First => Some(tokens.remove(0)),
        TokenSide::Last => Some(tokens.pop().unwrap()),
    }
}

fn mod_idish(t: &str) -> bool {
    // Reject pure version numbers like `1.2.0` → `120`.
    if !t.is_empty() && t.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    !matches!(
        t,
        "any" | "version" | "of" | "mod" | "the" | "all" | "latest" | "with" | "that" | "this"
    )
}

/// Iterate over several lines and collect conflict pairs (dedup by reason).
pub fn parse_conflicts_from_lines(lines: &[&str]) -> Vec<Conflict> {
    let mut out = Vec::new();
    for line in lines {
        if let Some(conflict) = parse_breakage_line(line) {
            if !out.iter().any(|c: &Conflict| c.reason == conflict.reason) {
                out.push(conflict);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_quoted_ids() {
        let ids = extract_quoted_ids("incompatible with any version of mod 'Sodium'");
        assert!(ids.contains(&"sodium".to_string()));
    }

    #[test]
    fn parses_incompatible_with_quoted() {
        let c =
            parse_breakage_line("SP-Backrooms is incompatible with any version of mod 'Sodium'")
                .expect("should parse");
        assert_eq!(c.a, "sp-backrooms");
        assert_eq!(c.b, "sodium");
        assert_eq!(c.kind, ConflictKind::Breaking);
    }

    #[test]
    fn parses_breaks_quoted() {
        let c = parse_breakage_line("spb-revamped 1.2.0 breaks 'indium'").expect("should parse");
        assert_eq!(c.b, "indium");
        assert_eq!(c.kind, ConflictKind::Breaking);
    }

    #[test]
    fn parses_conflicts_with() {
        let c = parse_breakage_line("Mod 'OptiFine' (optifine) conflicts with 'Sodium'")
            .expect("should parse");
        assert_eq!(c.kind, ConflictKind::Breaking);
    }

    #[test]
    fn parses_neg_hard_dep_breaks() {
        let c = parse_breakage_line("NEG_HARD_DEP spb-revamped 1.2.0 {breaks indium @ [*]}")
            .expect("should parse");
        assert_eq!(c.a, "spb-revamped");
        assert_eq!(c.b, "indium");
        assert_eq!(c.kind, ConflictKind::Breaking);
    }

    #[test]
    fn parses_mixin_conflict_kind() {
        let c = parse_breakage_line("MixinExtras failed for 'canvas' (conflicts with 'sodium')")
            .expect("should parse");
        assert_eq!(c.kind, ConflictKind::Mixin);
    }

    #[test]
    fn ignores_non_conflict_lines() {
        assert!(parse_breakage_line("Server thread/WARN: Nothing to see here.").is_none());
        assert!(parse_breakage_line("Loaded 137 mods").is_none());
    }

    #[test]
    fn collects_from_lines() {
        let lines = [
            "NEG_HARD_DEP spb-revamped 1.2.0 {breaks indium @ [*]}",
            "NEG_HARD_DEP spb-revamped 1.2.0 {breaks sodium @ [*]}",
            "spun a wheel",
        ];
        let conflicts = parse_conflicts_from_lines(&lines);
        assert_eq!(conflicts.len(), 2);
    }
}
