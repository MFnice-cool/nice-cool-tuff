#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TagId {
    pub namespace: String,
    pub path: String,
}

impl std::fmt::Display for TagId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.namespace, self.path)
    }
}

#[derive(Debug, Clone)]
pub struct UnifiedTag {
    pub id: TagId,
    pub entries: Vec<TagEntry>,
    pub replace: bool,
}

#[derive(Debug, Clone)]
pub struct TagEntry {
    pub id: String,
    pub required: bool,
}

pub fn tag_id_from_path(path: &str) -> Option<TagId> {
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() >= 5 && parts[0] == "data" && parts[2] == "tags" {
        let namespace = parts[1];
        let tag_path = parts[4..].join("/");
        let tag_name = tag_path.trim_end_matches(".json");
        Some(TagId {
            namespace: namespace.to_string(),
            path: tag_name.to_string(),
        })
    } else {
        None
    }
}
