//! Server/options.properties parser for Minecraft config files.
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct PropertiesFile {
    pub entries: Vec<PropertyEntry>,
}

#[derive(Debug, Clone)]
pub struct PropertyEntry {
    pub key: String,
    pub value: String,
    pub comment_before: Option<String>,
}

impl PropertiesFile {
    pub fn parse(content: &str) -> Self {
        let mut entries = Vec::new();
        let mut pending_comment = String::new();
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                pending_comment.clear();
                continue;
            }
            if trimmed.starts_with('#') || trimmed.starts_with('!') {
                if !pending_comment.is_empty() {
                    pending_comment.push('\n');
                }
                pending_comment.push_str(trimmed);
                continue;
            }
            if let Some(eq_pos) = trimmed.find('=') {
                let key = trimmed[..eq_pos].trim().to_string();
                if key.is_empty() {
                    pending_comment.clear();
                    continue;
                }
                let value = trimmed[eq_pos + 1..].trim().to_string();
                entries.push(PropertyEntry {
                    key,
                    value,
                    comment_before: if pending_comment.is_empty() {
                        None
                    } else {
                        Some(pending_comment.clone())
                    },
                });
                pending_comment.clear();
            }
        }
        Self { entries }
    }

    pub fn to_map(&self) -> HashMap<String, String> {
        let mut map = HashMap::new();
        for e in &self.entries {
            map.insert(e.key.clone(), e.value.clone());
        }
        map
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|e| e.key == key)
            .map(|e| e.value.as_str())
    }

    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.get(key)
            .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
    }

    pub fn get_int(&self, key: &str) -> Option<i64> {
        self.get(key).and_then(|v| v.parse().ok())
    }

    pub fn set(&mut self, key: &str, value: &str) {
        for e in &mut self.entries {
            if e.key == key {
                e.value = value.to_string();
                return;
            }
        }
        self.entries.push(PropertyEntry {
            key: key.into(),
            value: value.into(),
            comment_before: None,
        });
    }

    pub fn to_string(&self) -> String {
        let mut out = String::new();
        for e in &self.entries {
            if let Some(c) = &e.comment_before {
                out.push_str(c);
                out.push('\n');
            }
            out.push_str(&format!("{}={}\n", e.key, e.value));
        }
        out
    }

    /// Replace the TuffBox-managed marker block (kjsgen-style
    /// `// tuffbox:start <name>` … `// tuffbox:end <name>`) with fresh
    /// content, leaving everything outside the block untouched. Used for
    /// machine-generated sections (FPS-boost video settings, server tuning)
    /// inside user-owned files: re-generation never clobbers hand edits.
    ///
    /// In .properties files markers are written as `#` comments. `content`
    /// may be empty — that removes the block entirely.
    pub fn replace_managed_block(content: &str, block_name: &str, new_body: &str) -> String {
        let start_marker = format!("# tuffbox:start {block_name}");
        let end_marker = format!("# tuffbox:end {block_name}");
        let mut out = String::new();
        let mut in_block = false;
        let mut block_written = false;
        for line in content.lines() {
            let trimmed = line.trim_start();
            if trimmed == start_marker {
                in_block = true;
                if !block_written {
                    out.push_str(&start_marker);
                    out.push('\n');
                    if !new_body.is_empty() {
                        out.push_str(new_body);
                        if !new_body.ends_with('\n') {
                            out.push('\n');
                        }
                    }
                    out.push_str(&end_marker);
                    out.push('\n');
                    block_written = true;
                }
                continue;
            }
            if in_block {
                if trimmed == end_marker {
                    in_block = false;
                }
                // Skip everything until the end marker.
                continue;
            }
            out.push_str(line);
            out.push('\n');
        }
        // No pre-existing block: append one at the end.
        if !block_written {
            if !out.is_empty() && !out.ends_with("\n\n") {
                if !out.ends_with('\n') {
                    out.push('\n');
                }
                out.push('\n');
            }
            out.push_str(&start_marker);
            out.push('\n');
            if !new_body.is_empty() {
                out.push_str(new_body);
                if !new_body.ends_with('\n') {
                    out.push('\n');
                }
            }
            out.push_str(&end_marker);
            out.push('\n');
        }
        // Trim triple+ blank lines that removal may leave behind.
        while out.contains("\n\n\n") {
            out = out.replace("\n\n\n", "\n\n");
        }
        out
    }

    /// True when `content` contains a managed block named `block_name`.
    pub fn has_managed_block(content: &str, block_name: &str) -> bool {
        let start = format!("# tuffbox:start {block_name}");
        content.lines().any(|l| l.trim_start() == start)
    }

    pub fn minecraft_defaults() -> HashMap<&'static str, &'static str> {
        HashMap::from([
            ("server-port", "25565"),
            ("max-players", "20"),
            ("view-distance", "10"),
            ("simulation-distance", "10"),
            ("online-mode", "true"),
            ("difficulty", "normal"),
            ("gamemode", "survival"),
            ("enable-command-block", "false"),
            ("spawn-protection", "16"),
            ("max-tick-time", "60000"),
            ("level-name", "world"),
            ("allow-flight", "false"),
            ("pvp", "true"),
            ("spawn-npcs", "true"),
            ("spawn-animals", "true"),
            ("spawn-monsters", "true"),
            ("generate-structures", "true"),
            ("allow-nether", "true"),
            ("force-gamemode", "false"),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses() {
        let p = PropertiesFile::parse("server-port=25565\nmax-players=20\n");
        assert_eq!(p.get("server-port"), Some("25565"));
    }
    #[test]
    fn bools() {
        let p = PropertiesFile::parse("a=true\nb=false\n");
        assert_eq!(p.get_bool("a"), Some(true));
    }
    #[test]
    fn roundtrip() {
        let t = "server-port=25565\nmax-players=20\n";
        assert_eq!(PropertiesFile::parse(t).to_string(), t);
    }
    #[test]
    fn set_val() {
        let mut p = PropertiesFile::parse("a=1\n");
        p.set("a", "2");
        p.set("b", "3");
        assert_eq!(p.get("a"), Some("2"));
    }

    #[test]
    fn managed_block_replaces_only_its_section() {
        let original = "user-setting=keepme\n# tuffbox:start fps\nrenderDistance=32\n# tuffbox:end fps\nanother=1\n";
        let updated = PropertiesFile::replace_managed_block(original, "fps", "renderDistance=12");
        assert!(updated.contains("user-setting=keepme"));
        assert!(updated.contains("another=1"));
        assert!(updated.contains("renderDistance=12"));
        assert!(!updated.contains("renderDistance=32"));
        assert!(PropertiesFile::has_managed_block(&updated, "fps"));
        // Round-trip through the parser: managed block lines are comments,
        // so parse() keeps user keys intact.
        let parsed = PropertiesFile::parse(&updated);
        assert_eq!(parsed.get("user-setting"), Some("keepme"));
    }

    #[test]
    fn managed_block_appends_when_missing() {
        let updated = PropertiesFile::replace_managed_block("a=1\n", "fps", "b=2");
        assert!(updated.contains("a=1"));
        assert!(updated.contains("# tuffbox:start fps"));
        assert!(updated.contains("b=2"));
        assert!(updated.contains("# tuffbox:end fps"));
    }

    #[test]
    fn managed_block_removal_with_empty_body() {
        let original = "a=1\n# tuffbox:start fps\nx=1\ny=2\n# tuffbox:end fps\n";
        let updated = PropertiesFile::replace_managed_block(original, "fps", "");
        assert!(updated.contains("a=1"));
        assert!(!updated.contains("x=2"));
        assert!(!updated.contains("x="));
        // Empty block still has markers (removable again later).
        assert!(PropertiesFile::has_managed_block(&updated, "fps"));
    }
}
