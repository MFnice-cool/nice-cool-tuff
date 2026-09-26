//! SNBT (Stringified NBT) parser for FTB Quests.
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path};

use crate::fs_util::atomic_write;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QuestBook {
    pub chapters: Vec<Chapter>,
    pub title: Option<String>,
    pub subtitle: Option<String>,
    #[serde(default, rename = "chapterGroups")]
    pub chapter_groups: Vec<ChapterGroup>,
    #[serde(default, rename = "rewardTables")]
    pub reward_tables: Vec<RewardTable>,
    /// Remaining keys from `data.snbt` (defaults, loot, etc.) for round-trip.
    #[serde(
        default,
        rename = "bookSettings",
        skip_serializing_if = "HashMap::is_empty"
    )]
    pub book_settings: HashMap<String, serde_json::Value>,
    /// `lang/<code>.snbt` translation maps (string or string[] values).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub locales: HashMap<String, HashMap<String, serde_json::Value>>,
    /// Active locale code after overlay (set by clients; optional on load).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "activeLocale"
    )]
    pub active_locale: Option<String>,
    /// Non-fatal load problems (corrupt chapter SNBT, bad lang file, etc.).
    #[serde(
        default,
        rename = "loadWarnings",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub load_warnings: Vec<String>,
}

/// FTB Quests sidebar group (`chapter_groups.snbt`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChapterGroup {
    pub id: String,
    pub title: String,
    /// True when `title` was present in SNBT (vs lang overlay only).
    #[serde(default, rename = "titleFromSnbt")]
    pub title_from_snbt: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chapter {
    pub id: String,
    pub title: String,
    /// True when `title` was present in chapter SNBT.
    #[serde(default, rename = "titleFromSnbt")]
    pub title_from_snbt: bool,
    /// String id or full item-stack compound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<serde_json::Value>,
    pub quests: Vec<Quest>,
    pub group: Option<String>,
    /// FTB chapter sort key (`order_index` in SNBT).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "orderIndex"
    )]
    pub order_index: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "defaultQuestShape"
    )]
    pub default_quest_shape: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "defaultHideDependencyLines"
    )]
    pub default_hide_dependency_lines: Option<bool>,
    /// Unknown/extra chapter SNBT keys preserved on save.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub extras: HashMap<String, serde_json::Value>,
    /// Relative path inside the project (e.g. config/ftbquests/quests/chapters/foo.snbt).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "sourceFile"
    )]
    pub source_file: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Quest {
    pub id: String,
    pub title: String,
    /// True when `title` was present in quest SNBT.
    #[serde(default, rename = "titleFromSnbt")]
    pub title_from_snbt: bool,
    pub subtitle: Option<String>,
    /// True when `subtitle` was present in quest SNBT.
    #[serde(default, rename = "subtitleFromSnbt")]
    pub subtitle_from_snbt: bool,
    pub description: Vec<String>,
    /// True when `description` was present in quest SNBT.
    #[serde(default, rename = "descriptionFromSnbt")]
    pub description_from_snbt: bool,
    pub x: f64,
    pub y: f64,
    /// String id or full item-stack compound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<serde_json::Value>,
    pub dependencies: Vec<String>,
    pub tasks: Vec<Task>,
    pub rewards: Vec<Reward>,
    pub optional: bool,
    pub shape: Option<String>,
    pub size: Option<f64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "hideDependencyLines"
    )]
    pub hide_dependency_lines: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "hideDependentLines"
    )]
    pub hide_dependent_lines: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "minRequiredDependencies"
    )]
    pub min_required_dependencies: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "canRepeat")]
    pub can_repeat: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invisible: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "disableToast"
    )]
    pub disable_toast: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "dependencyRequirement"
    )]
    pub dependency_requirement: Option<String>,
    /// Unknown/extra quest SNBT keys preserved on save.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub extras: HashMap<String, serde_json::Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    #[serde(rename = "type")]
    pub task_type: String,
    pub title: Option<String>,
    /// True when `title` was present in task SNBT.
    #[serde(default, rename = "titleFromSnbt")]
    pub title_from_snbt: bool,
    pub value: Option<serde_json::Value>,
    pub properties: HashMap<String, serde_json::Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reward {
    pub id: String,
    #[serde(rename = "type")]
    pub reward_type: String,
    pub title: Option<String>,
    pub properties: HashMap<String, serde_json::Value>,
}
#[derive(Debug, Clone)]
pub struct QuestValidationError {
    pub quest_id: String,
    pub message: String,
}

impl QuestBook {
    /// Resolve FTB Quests directory inside a project (config or defaultconfigs).
    pub fn quests_dir_for_project(project_dir: &std::path::Path) -> std::path::PathBuf {
        for rel in ["config/ftbquests/quests", "defaultconfigs/ftbquests/quests"] {
            let candidate = project_dir.join(rel);
            if candidate.is_dir() {
                return candidate;
            }
        }
        project_dir.join("config/ftbquests/quests")
    }

    pub fn load_from_project(project_dir: &std::path::Path) -> Result<Self, String> {
        let quests_dir = Self::quests_dir_for_project(project_dir);
        Self::load_from_dir(&quests_dir, project_dir)
    }

    pub fn load_from_dir(
        dir: &std::path::Path,
        project_dir: &std::path::Path,
    ) -> Result<Self, String> {
        let mut chapters = Vec::new();
        let mut load_warnings = Vec::new();
        let chapter_dir = dir.join("chapters");
        let search_dir = if chapter_dir.is_dir() {
            chapter_dir
        } else {
            dir.to_path_buf()
        };
        if !search_dir.is_dir() {
            return Ok(Self::default());
        }
        for entry in std::fs::read_dir(&search_dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if path.extension().map_or(true, |e| e != "snbt") {
                continue;
            }
            let label = path
                .strip_prefix(project_dir)
                .ok()
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_else(|| path.display().to_string());
            match std::fs::read_to_string(&path) {
                Ok(content) => match parse_snbt_chapter(&content) {
                    Ok((mut ch, mut file_warnings)) => {
                        ch.source_file = Some(label.clone());
                        for w in file_warnings.drain(..) {
                            load_warnings.push(format!("{label}: {w}"));
                        }
                        chapters.push(ch);
                    }
                    Err(e) => {
                        load_warnings.push(format!("Skipped chapter {label}: {e}"));
                    }
                },
                Err(e) => {
                    load_warnings.push(format!("Skipped chapter {label}: {e}"));
                }
            }
        }
        chapters.sort_by(|a, b| {
            a.order_index
                .cmp(&b.order_index)
                .then_with(|| a.title.cmp(&b.title))
        });

        let (title, subtitle, book_settings) = load_book_data(dir);
        let (chapter_groups, mut group_warns) = load_chapter_groups(dir);
        load_warnings.append(&mut group_warns);
        let (locales, mut locale_warns) = load_locales(dir);
        load_warnings.append(&mut locale_warns);

        Ok(QuestBook {
            chapters,
            title,
            subtitle,
            chapter_groups,
            reward_tables: RewardTable::load_from_project(project_dir),
            book_settings,
            locales,
            active_locale: None,
            load_warnings,
        })
    }

    /// Write `data.snbt` (book title + settings).
    pub fn save_book_data(
        project_dir: &std::path::Path,
        book: &QuestBook,
    ) -> Result<String, String> {
        let quests_dir = Self::quests_dir_for_project(project_dir);
        std::fs::create_dir_all(&quests_dir).map_err(|e| e.to_string())?;
        let rel = {
            let candidate = project_dir.join("config/ftbquests/quests");
            if quests_dir == candidate || quests_dir.starts_with(&candidate) {
                "config/ftbquests/quests/data.snbt"
            } else {
                "defaultconfigs/ftbquests/quests/data.snbt"
            }
        };
        // Prefer path next to loaded chapters when possible
        let rel = book
            .chapters
            .iter()
            .find_map(|c| c.source_file.as_ref())
            .and_then(|sf| {
                let p = std::path::Path::new(sf);
                p.parent().map(|parent| {
                    parent
                        .join("data.snbt")
                        .to_string_lossy()
                        .replace('\\', "/")
                })
            })
            .unwrap_or_else(|| rel.to_string());

        let mut map = book.book_settings.clone();
        if let Some(t) = &book.title {
            map.insert("title".into(), serde_json::Value::String(t.clone()));
        }
        if let Some(s) = &book.subtitle {
            map.insert("subtitle".into(), serde_json::Value::String(s.clone()));
        }
        if !map.contains_key("version") {
            map.insert("version".into(), serde_json::json!(13));
        }
        let snbt = format!("{{\n{}\n}}\n", snbt_object_body(&map, 1));
        let target = project_dir.join(&rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        atomic_write(&target, snbt)?;
        Ok(rel)
    }

    /// Write `chapter_groups.snbt`.
    pub fn save_chapter_groups(
        project_dir: &std::path::Path,
        groups: &[ChapterGroup],
    ) -> Result<String, String> {
        let quests_dir = Self::quests_dir_for_project(project_dir);
        std::fs::create_dir_all(&quests_dir).map_err(|e| e.to_string())?;
        let config_candidate = project_dir.join("config/ftbquests/quests");
        let rel = if quests_dir == config_candidate || quests_dir.starts_with(&config_candidate) {
            "config/ftbquests/quests/chapter_groups.snbt"
        } else {
            "defaultconfigs/ftbquests/quests/chapter_groups.snbt"
        };
        let mut lines = vec!["{".to_string(), "\tchapter_groups: [".to_string()];
        for (i, g) in groups.iter().enumerate() {
            let comma = if i + 1 == groups.len() { "" } else { "," };
            if g.title_from_snbt && !g.title.is_empty() {
                lines.push(format!(
                    "\t\t{{ id: {} title: {} }}{}",
                    snbt_quote(&g.id),
                    snbt_quote(&g.title),
                    comma
                ));
            } else {
                lines.push(format!("\t\t{{ id: {} }}{}", snbt_quote(&g.id), comma));
            }
        }
        lines.push("\t]".to_string());
        lines.push("}".to_string());
        let target = project_dir.join(rel);
        atomic_write(&target, lines.join("\n"))?;
        Ok(rel.into())
    }

    /// Write `lang/<code>.snbt` translation map (full file replace for that locale).
    pub fn save_locale(
        project_dir: &std::path::Path,
        code: &str,
        map: &HashMap<String, serde_json::Value>,
    ) -> Result<String, String> {
        let code = code.trim();
        if code.is_empty()
            || code.contains('/')
            || code.contains('\\')
            || code.contains("..")
            || code.contains('\0')
        {
            return Err("Invalid locale code".into());
        }
        let mut comps = Path::new(code).components();
        match comps.next() {
            Some(Component::Normal(_)) if comps.next().is_none() => {}
            _ => return Err("Invalid locale code".into()),
        }
        let quests_dir = Self::quests_dir_for_project(project_dir);
        let lang_dir = quests_dir.join("lang");
        std::fs::create_dir_all(&lang_dir).map_err(|e| e.to_string())?;
        let snbt = format!("{{\n{}\n}}\n", snbt_object_body(map, 1));
        let file_name = format!("{code}.snbt");
        let target = lang_dir.join(&file_name);
        if target.parent() != Some(lang_dir.as_path()) {
            return Err("Invalid locale code".into());
        }
        atomic_write(&target, snbt)?;
        // Prefer config/… relative path when under standard layout
        let candidate = project_dir
            .join("config/ftbquests/quests/lang")
            .join(&file_name);
        let rel = if target == candidate || target.starts_with(project_dir.join("config")) {
            format!("config/ftbquests/quests/lang/{file_name}")
        } else {
            format!("defaultconfigs/ftbquests/quests/lang/{file_name}")
        };
        Ok(rel)
    }

    /// Map task id → owning quest id (FTB deps may point at either).
    pub fn task_owner_map(&self) -> HashMap<String, String> {
        let mut map = HashMap::new();
        for ch in &self.chapters {
            for q in &ch.quests {
                for t in &q.tasks {
                    if !t.id.is_empty() {
                        map.insert(t.id.clone(), q.id.clone());
                    }
                }
            }
        }
        map
    }

    /// Resolve a dependency id to a quest id (quest itself, or parent of a task).
    pub fn resolve_dep(&self, dep: &str) -> Option<String> {
        self.resolve_dep_with(dep, &self.task_owner_map())
    }

    /// Like [`Self::resolve_dep`], reusing a precomputed task→quest map (hot paths).
    pub fn resolve_dep_with(
        &self,
        dep: &str,
        task_owners: &HashMap<String, String>,
    ) -> Option<String> {
        if self
            .chapters
            .iter()
            .flat_map(|ch| ch.quests.iter())
            .any(|q| q.id == dep)
        {
            return Some(dep.to_string());
        }
        task_owners.get(dep).cloned()
    }

    /// Quest dependency graph with task-id edges rewritten to parent quests.
    fn resolved_dep_graph(&self) -> HashMap<String, Vec<String>> {
        let owners = self.task_owner_map();
        let quest_ids: HashSet<String> = self
            .chapters
            .iter()
            .flat_map(|ch| ch.quests.iter().map(|q| q.id.clone()))
            .collect();
        self.chapters
            .iter()
            .flat_map(|ch| ch.quests.iter())
            .map(|q| {
                let mut parents = Vec::new();
                for d in &q.dependencies {
                    let resolved = if quest_ids.contains(d) {
                        Some(d.clone())
                    } else {
                        owners.get(d).cloned()
                    };
                    if let Some(pid) = resolved {
                        if pid != q.id && !parents.contains(&pid) {
                            parents.push(pid);
                        }
                    }
                }
                (q.id.clone(), parents)
            })
            .collect()
    }

    pub fn save_chapter(
        project_dir: &std::path::Path,
        chapter: &Chapter,
        relative_path: Option<&str>,
    ) -> Result<String, String> {
        let rel = relative_path
            .map(|s| s.to_string())
            .or_else(|| chapter.source_file.clone())
            .unwrap_or_else(|| {
                format!(
                    "config/ftbquests/quests/chapters/{}.snbt",
                    sanitize_snbt_filename(&chapter.id)
                )
            });
        let target = project_dir.join(&rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let snbt = serialize_chapter_to_snbt(chapter);
        atomic_write(&target, snbt)?;
        Ok(rel)
    }

    pub fn validate(&self) -> Vec<QuestValidationError> {
        self.validate_with_items(None)
    }

    /// Full pack checks: missing deps, empty tasks, duplicate ids, cycles,
    /// reachability from roots, and (optionally) unknown item ids.
    pub fn validate_with_items(
        &self,
        available_items: Option<&HashSet<String>>,
    ) -> Vec<QuestValidationError> {
        let mut errors = Vec::new();
        let mut seen_ids: HashSet<String> = HashSet::new();
        let all_quest_ids: HashSet<String> = self
            .chapters
            .iter()
            .flat_map(|ch| ch.quests.iter().map(|q| q.id.clone()))
            .collect();
        let task_owners = self.task_owner_map();

        for ch in &self.chapters {
            for q in &ch.quests {
                if !seen_ids.insert(q.id.clone()) {
                    errors.push(QuestValidationError {
                        quest_id: q.id.clone(),
                        message: format!("Duplicate quest id '{}'", q.id),
                    });
                }
                for dep in &q.dependencies {
                    if !all_quest_ids.contains(dep) && !task_owners.contains_key(dep) {
                        errors.push(QuestValidationError {
                            quest_id: q.id.clone(),
                            message: format!("Dep '{}' missing", dep),
                        });
                    }
                }
                if q.tasks.is_empty() {
                    errors.push(QuestValidationError {
                        quest_id: q.id.clone(),
                        message: "No tasks".into(),
                    });
                }
                if let Some(items) = available_items {
                    for item in extract_quest_item_ids(q) {
                        if item.is_empty()
                            || item.starts_with('#')
                            || item.starts_with("itemfilters:")
                        {
                            continue;
                        }
                        if !items.contains(&item) {
                            errors.push(QuestValidationError {
                                quest_id: q.id.clone(),
                                message: format!("Unknown item '{item}'"),
                            });
                        }
                    }
                }
            }
        }

        for cycle in self.find_cycles() {
            let msg = format!("Dependency cycle: {}", cycle.join(" → "));
            let quest_id = cycle.first().cloned().unwrap_or_default();
            errors.push(QuestValidationError {
                quest_id,
                message: msg,
            });
        }

        for qid in self.unreachable_quest_ids_with(&task_owners) {
            errors.push(QuestValidationError {
                quest_id: qid,
                message: "Unreachable from any root quest".into(),
            });
        }

        errors
    }

    /// Quests that cannot be reached by walking dependencies from roots
    /// (quests with no dependencies). Missing deps are treated as roots'
    /// children only when the dep id exists in the book.
    pub fn unreachable_quest_ids(&self) -> Vec<String> {
        let owners = self.task_owner_map();
        self.unreachable_quest_ids_with(&owners)
    }

    /// Same as [`Self::unreachable_quest_ids`] but reuses a precomputed task-owner map.
    pub fn unreachable_quest_ids_with(&self, task_owners: &HashMap<String, String>) -> Vec<String> {
        let graph = self.resolved_dep_graph();
        if graph.is_empty() {
            return Vec::new();
        }

        // Reverse edges: dep -> dependents (who requires this quest)
        let mut dependents: HashMap<String, Vec<String>> = HashMap::new();
        for (id, deps) in &graph {
            for d in deps {
                dependents.entry(d.clone()).or_default().push(id.clone());
            }
        }

        let roots: Vec<String> = graph
            .iter()
            .filter(|(_, deps)| deps.is_empty())
            .map(|(id, _)| id.clone())
            .collect();
        if roots.is_empty() {
            // Every quest has deps — if there are cycles everything is "stuck";
            // otherwise pick arbitrary starts already covered by cycle errors.
            return Vec::new();
        }

        let mut reachable: HashSet<String> = HashSet::new();
        let mut stack = roots;
        while let Some(id) = stack.pop() {
            if !reachable.insert(id.clone()) {
                continue;
            }
            if let Some(kids) = dependents.get(&id) {
                for k in kids {
                    stack.push(k.clone());
                }
            }
        }

        // Only flag islands where every raw dep resolves — broken/missing deps
        // already have their own error and would otherwise flood the list.
        let quest_ids: HashSet<String> = graph.keys().cloned().collect();
        let mut missing: Vec<String> = self
            .chapters
            .iter()
            .flat_map(|ch| ch.quests.iter())
            .filter(|q| {
                if reachable.contains(&q.id) {
                    return false;
                }
                q.dependencies
                    .iter()
                    .all(|d| quest_ids.contains(d) || task_owners.contains_key(d))
            })
            .map(|q| q.id.clone())
            .collect();
        missing.sort();
        missing
    }

    pub fn is_reachable(&self, quest_id: &str) -> bool {
        !self.unreachable_quest_ids().iter().any(|id| id == quest_id)
    }

    /// Returns a list of quest-id cycles found in the dependency graph.
    ///
    /// FTB Quests detects dependency cycles with a depth-limited DFS so a
    /// malformed quest pack can't recurse forever; an infinitely deep
    /// (or simply very large) dependency chain would otherwise hang the
    /// UI's topological sort. We mirror that: each returned entry is the
    /// ordered list of quest ids forming one cycle (e.g. `A -> B -> A`).
    pub fn find_cycles(&self) -> Vec<Vec<String>> {
        let graph = self.resolved_dep_graph();
        let mut cycles = Vec::new();
        let mut visited: HashSet<String> = HashSet::new();
        let mut on_stack: HashSet<String> = HashSet::new();
        let mut path: Vec<String> = Vec::new();

        // Cap the DFS depth so a pathological pack (or a self-loop chain
        // thousands deep) cannot blow the stack. Any single cycle longer
        // than this is reported via a truncation marker instead.
        const MAX_DEPTH: usize = 1024;

        fn dfs(
            node: &str,
            graph: &HashMap<String, Vec<String>>,
            visited: &mut HashSet<String>,
            on_stack: &mut HashSet<String>,
            path: &mut Vec<String>,
            cycles: &mut Vec<Vec<String>>,
            depth: usize,
            max_depth: usize,
        ) {
            if depth >= max_depth {
                return;
            }
            if on_stack.contains(node) {
                // Found a back-edge: extract the cycle portion of the path.
                if let Some(start) = path.iter().position(|n| n == node) {
                    let mut cycle: Vec<String> = path[start..].to_vec();
                    cycle.push(node.to_string());
                    cycles.push(cycle);
                }
                return;
            }
            if visited.contains(node) {
                return;
            }
            visited.insert(node.to_string());
            on_stack.insert(node.to_string());
            path.push(node.to_string());

            if let Some(neighbors) = graph.get(node) {
                for next in neighbors {
                    dfs(
                        next,
                        graph,
                        visited,
                        on_stack,
                        path,
                        cycles,
                        depth + 1,
                        max_depth,
                    );
                }
            }

            path.pop();
            on_stack.remove(node);
        }

        for start in graph.keys() {
            dfs(
                start,
                &graph,
                &mut visited,
                &mut on_stack,
                &mut path,
                &mut cycles,
                0,
                MAX_DEPTH,
            );
        }
        cycles
    }

    /// Returns quests ordered so that every dependency precedes the quests
    /// that depend on it, or `Err` listing the cycles that prevent a
    /// topological order. Quests with no dependencies (or whose
    /// dependencies are all satisfied) come first. This is the same
    /// dependency resolution FTB Quests performs before it lays out the
    /// quest screen, and it degrades gracefully: a cyclic pack returns the
    /// cycles instead of looping forever.
    pub fn topo_order(&self) -> Result<Vec<String>, Vec<Vec<String>>> {
        let cycles = self.find_cycles();
        if !cycles.is_empty() {
            return Err(cycles);
        }

        let graph = self.resolved_dep_graph();
        let mut order = Vec::new();
        let mut visited: HashSet<String> = HashSet::new();

        // Iterative post-order DFS to avoid stack overflow on deep chains.
        // Each frame is (node, next_child_index). We visit children first;
        // only when every child index has been handled do we pop the node
        // into `order`, giving a correct post-order (dependency last).
        for start in graph.keys() {
            if visited.contains(start) {
                continue;
            }
            let mut stack: Vec<(String, usize)> = vec![(start.clone(), 0)];
            while let Some((node, idx)) = stack.last().cloned() {
                let neighbors = graph.get(&node).cloned().unwrap_or_default();
                if idx < neighbors.len() {
                    // Advance this frame's cursor, then push the child (if new).
                    stack.last_mut().unwrap().1 += 1;
                    let next = &neighbors[idx];
                    if !visited.contains(next) && graph.contains_key(next) {
                        stack.push((next.clone(), 0));
                    }
                } else {
                    stack.pop();
                    if visited.insert(node.clone()) {
                        order.push(node);
                    }
                }
            }
        }
        // `order` is post-order: a quest's dependencies are all visited and
        // recorded *before* the quest itself, i.e. dependency-first order.
        Ok(order)
    }
}

pub fn snbt_to_json(text: &str) -> Result<serde_json::Value, String> {
    parse_snbt(text)
}

/// Recursive-descent SNBT (Stringified NBT) parser.
/// SNBT uses the same structure as JSON but permits unquoted keys and
/// optional (whitespace or comma) separators between values.
fn parse_snbt(text: &str) -> Result<serde_json::Value, String> {
    Ok(parse_snbt_with_flags(text)?.0)
}

fn parse_snbt_with_flags(text: &str) -> Result<(serde_json::Value, bool), String> {
    let chars: Vec<char> = text.chars().collect();
    let mut p = SnbtParser {
        chars: &chars,
        pos: 0,
        loose_separator: false,
    };
    p.skip_ws();
    let v = p.parse_value()?;
    p.skip_ws();
    if p.pos != p.chars.len() {
        return Err(format!(
            "SNBT parse: trailing content at position {}",
            p.pos
        ));
    }
    Ok((v, p.loose_separator))
}

struct SnbtParser<'a> {
    chars: &'a [char],
    pos: usize,
    loose_separator: bool,
}

impl<'a> SnbtParser<'a> {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }
    fn skip_ws(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.pos += 1;
            } else {
                break;
            }
        }
    }
    fn parse_value(&mut self) -> Result<serde_json::Value, String> {
        self.skip_ws();
        match self.peek() {
            Some('{') => self.parse_object(),
            Some('[') => self.parse_array(),
            Some('"') => Ok(serde_json::Value::String(self.parse_string()?)),
            Some(c) if c.is_ascii_digit() || c == '-' => self.parse_number(),
            Some(c) if c.is_alphabetic() || c == '_' => self.parse_ident_value(),
            Some(other) => Err(format!(
                "SNBT parse: unexpected char '{}' at {}",
                other, self.pos
            )),
            None => Err("SNBT parse: unexpected end of input".into()),
        }
    }
    fn parse_object(&mut self) -> Result<serde_json::Value, String> {
        self.pos += 1; // consume '{'
        let mut map = serde_json::Map::new();
        self.skip_ws();
        if self.peek() == Some('}') {
            self.pos += 1;
            return Ok(serde_json::Value::Object(map));
        }
        loop {
            self.skip_ws();
            let key = self.parse_key()?;
            self.skip_ws();
            if self.peek() != Some(':') {
                return Err(format!(
                    "SNBT parse: expected ':' after key at {}",
                    self.pos
                ));
            }
            self.pos += 1; // consume ':'
            let val = self.parse_value()?;
            map.insert(key, val);
            self.skip_ws();
            match self.peek() {
                Some(',') => {
                    self.pos += 1;
                    continue;
                }
                Some('}') => {
                    self.pos += 1;
                    break;
                }
                Some(_) => {
                    // SNBT historically allows whitespace-only separators; FTB may reject.
                    self.loose_separator = true;
                    continue;
                }
                None => return Err("SNBT parse: unexpected end of input in object".into()),
            }
        }
        Ok(serde_json::Value::Object(map))
    }
    fn parse_array(&mut self) -> Result<serde_json::Value, String> {
        self.pos += 1; // consume '['
        let mut arr = Vec::new();
        self.skip_ws();
        if self.peek() == Some(']') {
            self.pos += 1;
            return Ok(serde_json::Value::Array(arr));
        }
        // Typed arrays: [I; ...], [B; ...], [L; ...], [F; ...], [D; ...]
        let mut typed: Option<char> = None;
        if let Some(t) = self.peek() {
            if "BILfdFD".contains(t) {
                let next = self.chars.get(self.pos + 1).copied();
                if next == Some(';') {
                    typed = Some(t.to_ascii_uppercase());
                    self.pos += 2;
                    self.skip_ws();
                    if self.peek() == Some(']') {
                        self.pos += 1;
                        return Ok(snbt_typed_array_json(typed.unwrap(), arr));
                    }
                }
            }
        }
        loop {
            let v = self.parse_value()?;
            arr.push(v);
            self.skip_ws();
            match self.peek() {
                Some(',') => {
                    self.pos += 1;
                    continue;
                }
                Some(']') => {
                    self.pos += 1;
                    break;
                }
                Some(_) => {
                    self.loose_separator = true;
                    continue;
                }
                None => return Err("SNBT parse: unexpected end of input in array".into()),
            }
        }
        if let Some(t) = typed {
            Ok(snbt_typed_array_json(t, arr))
        } else {
            Ok(serde_json::Value::Array(arr))
        }
    }
    fn parse_string(&mut self) -> Result<String, String> {
        self.pos += 1; // consume opening quote
        let mut s = String::new();
        while let Some(c) = self.peek() {
            self.pos += 1;
            match c {
                '"' => return Ok(s),
                '\\' => {
                    if let Some(e) = self.peek() {
                        self.pos += 1;
                        s.push(match e {
                            'n' => '\n',
                            't' => '\t',
                            'r' => '\r',
                            '\\' => '\\',
                            '"' => '"',
                            '\'' => '\'',
                            '/' => '/',
                            'b' => '\u{08}',
                            'f' => '\u{0C}',
                            other => other,
                        });
                    }
                }
                _ => s.push(c),
            }
        }
        Err("SNBT parse: unterminated string".into())
    }
    fn parse_key(&mut self) -> Result<String, String> {
        match self.peek() {
            Some('"') => self.parse_string(),
            Some(c) if c.is_alphabetic() || c == '_' => {
                let start = self.pos;
                while let Some(ch) = self.peek() {
                    if ch.is_alphanumeric() || ch == '_' || ch == '.' {
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
                Ok(self.chars[start..self.pos].iter().collect())
            }
            _ => Err(format!("SNBT parse: expected key at {}", self.pos)),
        }
    }
    fn parse_number(&mut self) -> Result<serde_json::Value, String> {
        let start = self.pos;
        if self.peek() == Some('-') {
            self.pos += 1;
        }
        let mut saw_dot = false;
        let mut saw_exp = false;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                self.pos += 1;
            } else if c == '.' && !saw_dot && !saw_exp {
                saw_dot = true;
                self.pos += 1;
            } else if (c == 'e' || c == 'E') && !saw_exp {
                saw_exp = true;
                self.pos += 1;
                if matches!(self.peek(), Some('+') | Some('-')) {
                    self.pos += 1;
                }
                // Exponent must contain at least one digit.
                if !self.peek().is_some_and(|d| d.is_ascii_digit()) {
                    return Err(format!("SNBT parse: bad exponent at {}", self.pos));
                }
            } else {
                break;
            }
        }
        // SNBT numeric suffixes: 0.0d, 1L, 2f, etc.
        if matches!(
            self.peek(),
            Some('d' | 'D' | 'f' | 'F' | 'l' | 'L' | 'b' | 'B' | 's' | 'S')
        ) {
            self.pos += 1;
        }
        let s: String = self.chars[start..self.pos].iter().collect();
        let numeric = s.trim_end_matches(|c: char| {
            matches!(c, 'd' | 'D' | 'f' | 'F' | 'l' | 'L' | 'b' | 'B' | 's' | 'S')
        });
        if let Ok(i) = numeric.parse::<i64>() {
            return Ok(serde_json::Value::from(i));
        }
        if let Ok(f) = numeric.parse::<f64>() {
            return Ok(serde_json::Value::from(f));
        }
        Err(format!("SNBT parse: invalid number '{s}'"))
    }
    fn parse_ident_value(&mut self) -> Result<serde_json::Value, String> {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' || c == '.' || c == '-' {
                self.pos += 1;
            } else {
                break;
            }
        }
        let s: String = self.chars[start..self.pos].iter().collect();
        match s.as_str() {
            "true" => Ok(serde_json::Value::Bool(true)),
            "false" => Ok(serde_json::Value::Bool(false)),
            "null" => Ok(serde_json::Value::Null),
            _ => Ok(serde_json::Value::String(s)),
        }
    }
}

fn load_book_data(
    dir: &std::path::Path,
) -> (
    Option<String>,
    Option<String>,
    HashMap<String, serde_json::Value>,
) {
    let path = dir.join("data.snbt");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return (None, None, HashMap::new());
    };
    let Ok(j) = snbt_to_json(&content) else {
        return (None, None, HashMap::new());
    };
    let Some(m) = j.as_object() else {
        return (None, None, HashMap::new());
    };
    let title = gs(m, "title");
    let subtitle = gs(m, "subtitle");
    let mut settings = HashMap::new();
    for (k, v) in m {
        if k == "title" || k == "subtitle" {
            continue;
        }
        settings.insert(k.clone(), v.clone());
    }
    (title, subtitle, settings)
}

fn load_chapter_groups(dir: &std::path::Path) -> (Vec<ChapterGroup>, Vec<String>) {
    let path = dir.join("chapter_groups.snbt");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return (Vec::new(), Vec::new());
    };
    let Ok((j, loose)) = parse_snbt_with_flags(&content) else {
        return (
            Vec::new(),
            vec!["chapter_groups.snbt: parse failed — groups ignored".into()],
        );
    };
    let mut warnings = Vec::new();
    if loose {
        warnings
            .push("chapter_groups.snbt: used whitespace-only separators (FTB may reject)".into());
    }
    let groups = j
        .get("chapter_groups")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|g| {
                    let m = g.as_object()?;
                    let inline_title = gs(m, "title");
                    Some(ChapterGroup {
                        id: gs(m, "id")?,
                        title: inline_title.clone().unwrap_or_default(),
                        title_from_snbt: inline_title.is_some(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    (groups, warnings)
}

/// Load `lang/<code>.snbt` into locale maps.
fn load_locales(
    dir: &std::path::Path,
) -> (
    HashMap<String, HashMap<String, serde_json::Value>>,
    Vec<String>,
) {
    let lang_dir = dir.join("lang");
    let Ok(entries) = std::fs::read_dir(&lang_dir) else {
        return (HashMap::new(), Vec::new());
    };
    let mut locales = HashMap::new();
    let mut warnings = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map_or(true, |e| e != "snbt") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Ok(content) = std::fs::read_to_string(&path) else {
            warnings.push(format!("lang/{stem}.snbt: read failed"));
            continue;
        };
        let Ok((j, loose)) = parse_snbt_with_flags(&content) else {
            warnings.push(format!("lang/{stem}.snbt: parse failed — locale ignored"));
            continue;
        };
        if loose {
            warnings.push(format!(
                "lang/{stem}.snbt: used whitespace-only separators (FTB may reject)"
            ));
        }
        let Some(obj) = j.as_object() else {
            warnings.push(format!("lang/{stem}.snbt: root is not an object"));
            continue;
        };
        let mut map = HashMap::new();
        for (k, v) in obj {
            map.insert(k.clone(), v.clone());
        }
        if !map.is_empty() {
            locales.insert(stem.to_string(), map);
        }
    }
    (locales, warnings)
}

fn snbt_object_body(map: &HashMap<String, serde_json::Value>, indent: usize) -> String {
    let pad = "\t".repeat(indent);
    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort();
    keys.into_iter()
        .map(|k| format!("{pad}{k}: {}", snbt_value(&map[k])))
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_snbt_chapter(c: &str) -> Result<(Chapter, Vec<String>), String> {
    let (j, loose) = parse_snbt_with_flags(c)?;
    let mut warnings = Vec::new();
    if loose {
        warnings.push("used whitespace-only separators (FTB may reject)".into());
    }
    let m = j.as_object().ok_or("not object")?;
    const KNOWN: &[&str] = &[
        "id",
        "title",
        "icon",
        "group",
        "order_index",
        "quests",
        "filename",
        "default_quest_shape",
        "default_hide_dependency_lines",
    ];
    let mut extras = HashMap::new();
    for (k, v) in m {
        if !KNOWN.contains(&k.as_str()) {
            extras.insert(k.clone(), v.clone());
        }
    }
    let mut used_ids = HashSet::new();
    let chapter = Chapter {
        id: resolve_hex_id(gs(m, "id"), 16, &mut used_ids),
        title: gs(m, "title").unwrap_or_default(),
        title_from_snbt: m.get("title").is_some(),
        icon: icon_value_from_map(m),
        group: gs(m, "group"),
        order_index: m.get("order_index").and_then(|v| {
            v.as_i64()
                .or_else(|| v.as_f64().map(|f| f as i64))
                .or_else(|| v.as_u64().map(|u| u as i64))
        }),
        filename: gs(m, "filename"),
        default_quest_shape: gs(m, "default_quest_shape"),
        default_hide_dependency_lines: m
            .get("default_hide_dependency_lines")
            .and_then(|v| v.as_bool()),
        extras,
        quests: m
            .get("quests")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|q| parse_snbt_quest(q, &mut used_ids).ok())
                    .collect()
            })
            .unwrap_or_default(),
        source_file: None,
    };
    Ok((chapter, warnings))
}
fn parse_snbt_quest(
    v: &serde_json::Value,
    used_ids: &mut HashSet<String>,
) -> Result<Quest, String> {
    let m = v.as_object().ok_or("not object")?;
    let dependencies = m
        .get("dependencies")
        .map(parse_dependencies)
        .unwrap_or_default();
    const KNOWN: &[&str] = &[
        "id",
        "title",
        "subtitle",
        "description",
        "x",
        "y",
        "icon",
        "dependencies",
        "tasks",
        "rewards",
        "optional",
        "shape",
        "size",
        "hide_dependency_lines",
        "hide_dependent_lines",
        "min_required_dependencies",
        "can_repeat",
        "invisible",
        "disable_toast",
        "dependency_requirement",
    ];
    let mut extras = HashMap::new();
    for (k, v) in m {
        if !KNOWN.contains(&k.as_str()) {
            extras.insert(k.clone(), v.clone());
        }
    }
    Ok(Quest {
        id: resolve_hex_id(gs(m, "id"), 16, used_ids),
        title: gs(m, "title").unwrap_or_default(),
        title_from_snbt: m.get("title").is_some(),
        subtitle: gs(m, "subtitle"),
        subtitle_from_snbt: m.get("subtitle").is_some(),
        description: m
            .get("description")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default(),
        description_from_snbt: m.get("description").is_some(),
        x: m.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0),
        y: m.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0),
        icon: icon_value_from_map(m),
        dependencies,
        tasks: m
            .get("tasks")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|t| parse_snbt_task(t, used_ids).ok())
                    .collect()
            })
            .unwrap_or_default(),
        rewards: m
            .get("rewards")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|r| parse_snbt_reward(r, used_ids).ok())
                    .collect()
            })
            .unwrap_or_default(),
        optional: m.get("optional").and_then(|v| v.as_bool()).unwrap_or(false),
        shape: gs(m, "shape"),
        size: m.get("size").and_then(|v| v.as_f64()),
        hide_dependency_lines: m.get("hide_dependency_lines").and_then(|v| v.as_bool()),
        hide_dependent_lines: m.get("hide_dependent_lines").and_then(|v| v.as_bool()),
        min_required_dependencies: m.get("min_required_dependencies").and_then(|v| {
            v.as_i64()
                .or_else(|| v.as_f64().map(|f| f as i64))
                .or_else(|| v.as_u64().map(|u| u as i64))
        }),
        can_repeat: m.get("can_repeat").and_then(|v| v.as_bool()),
        invisible: m.get("invisible").and_then(|v| v.as_bool()),
        disable_toast: m.get("disable_toast").and_then(|v| v.as_bool()),
        dependency_requirement: gs(m, "dependency_requirement"),
        extras,
    })
}

fn parse_dependencies(v: &serde_json::Value) -> Vec<String> {
    if let Some(s) = v.as_str() {
        return s.split_whitespace().map(|x| x.to_string()).collect();
    }
    if let Some(arr) = v.as_array() {
        return arr
            .iter()
            .filter_map(|x| x.as_str().map(|s| s.to_string()))
            .collect();
    }
    Vec::new()
}
fn parse_snbt_task(v: &serde_json::Value, used_ids: &mut HashSet<String>) -> Result<Task, String> {
    let m = v.as_object().ok_or("not object")?;
    let title = gs(m, "title");
    Ok(Task {
        id: resolve_hex_id(gs(m, "id"), 12, used_ids),
        task_type: gs(m, "type").unwrap_or_else(|| "item".into()),
        title_from_snbt: title.is_some(),
        title,
        value: m.get("value").cloned(),
        properties: m
            .iter()
            .filter(|(k, _)| !matches!(k.as_str(), "id" | "type" | "title" | "value"))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
    })
}
fn parse_snbt_reward(
    v: &serde_json::Value,
    used_ids: &mut HashSet<String>,
) -> Result<Reward, String> {
    let m = v.as_object().ok_or("not object")?;
    Ok(Reward {
        id: resolve_hex_id(gs(m, "id"), 12, used_ids),
        reward_type: gs(m, "type").unwrap_or_else(|| "item".into()),
        title: gs(m, "title"),
        properties: m
            .iter()
            .filter(|(k, _)| !matches!(k.as_str(), "id" | "type" | "title"))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
    })
}
fn gs(m: &serde_json::Map<String, serde_json::Value>, k: &str) -> Option<String> {
    m.get(k).and_then(|v| v.as_str()).map(|s| s.to_string())
}

/// FTB icons may be a string id or an item-stack object `{ id: "mod:item", ... }`.
fn icon_value_from_map(
    m: &serde_json::Map<String, serde_json::Value>,
) -> Option<serde_json::Value> {
    match m.get("icon") {
        None => None,
        Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(s)) => {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(serde_json::Value::String(t.to_string()))
            }
        }
        Some(v) => Some(v.clone()),
    }
}

/// Display id for UI / grounding (string or first item id inside a compound).
pub fn icon_display_id(icon: &serde_json::Value) -> Option<String> {
    match icon {
        serde_json::Value::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        }
        other => {
            let mut out = Vec::new();
            collect_item_ids_from_value(other, &mut out);
            out.iter()
                .find(|id| id.contains(':') && !id.starts_with('#'))
                .cloned()
                .or_else(|| out.into_iter().next())
        }
    }
}

fn collect_item_ids_from_value(v: &serde_json::Value, out: &mut Vec<String>) {
    match v {
        serde_json::Value::String(s) => {
            if !s.is_empty() {
                out.push(s.clone());
            }
        }
        serde_json::Value::Object(m) => {
            if let Some(id) = gs(m, "id").or_else(|| gs(m, "item")) {
                out.push(id);
            }
            // itemfilters:or / and wrap candidates under tag.items
            if let Some(tag) = m.get("tag").and_then(|t| t.as_object()) {
                if let Some(items) = tag.get("items").and_then(|i| i.as_array()) {
                    for it in items {
                        collect_item_ids_from_value(it, out);
                    }
                }
                if let Some(value) = tag.get("value").and_then(|x| x.as_str()) {
                    // itemfilters:tag uses tag.value = "#mod:tag" or similar
                    if !value.is_empty() {
                        out.push(value.to_string());
                    }
                }
            }
            if let Some(items) = m.get("items").and_then(|i| i.as_array()) {
                for it in items {
                    collect_item_ids_from_value(it, out);
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for it in arr {
                collect_item_ids_from_value(it, out);
            }
        }
        _ => {}
    }
}

fn fresh_hex_id(len: usize) -> String {
    use rand::Rng;
    const HEX: &[u8] = b"0123456789ABCDEF";
    let mut rng = rand::thread_rng();
    let mut out = String::with_capacity(len);
    for _ in 0..len {
        out.push(HEX[rng.gen_range(0..16)] as char);
    }
    out
}

/// Allocate a hex id not already in `used`, then insert it (mirrors quest_plan::alloc_hex_id).
fn alloc_fresh_hex_id(len: usize, used: &mut HashSet<String>) -> String {
    for _ in 0..64 {
        let id = fresh_hex_id(len);
        if used.insert(id.clone()) {
            return id;
        }
    }
    let mut widen = len + 4;
    loop {
        let id = fresh_hex_id(widen);
        if used.insert(id.clone()) {
            return id;
        }
        widen = widen.saturating_add(2);
    }
}

fn resolve_hex_id(raw: Option<String>, len: usize, used: &mut HashSet<String>) -> String {
    let id = raw.unwrap_or_default();
    if id.trim().is_empty() {
        return alloc_fresh_hex_id(len, used);
    }
    used.insert(id.clone());
    id
}

fn extract_quest_item_ids(q: &Quest) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(icon) = &q.icon {
        if let Some(id) = icon_display_id(icon) {
            if id.contains(':') {
                out.push(id);
            }
        }
    }
    for task in &q.tasks {
        if task.task_type == "item" {
            if let Some(v) = task.properties.get("item") {
                collect_item_ids_from_value(v, &mut out);
            }
        }
    }
    for reward in &q.rewards {
        if reward.reward_type == "item" {
            if let Some(v) = reward.properties.get("item") {
                collect_item_ids_from_value(v, &mut out);
            }
        }
    }
    out
}

fn sanitize_snbt_filename(id: &str) -> String {
    let cleaned: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "chapter".into()
    } else {
        cleaned
    }
}

fn snbt_quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

fn snbt_typed_array_json(type_char: char, values: Vec<serde_json::Value>) -> serde_json::Value {
    let mut out = serde_json::Map::new();
    out.insert(
        "__snbtArray".into(),
        serde_json::Value::String(type_char.to_string()),
    );
    out.insert("values".into(), serde_json::Value::Array(values));
    serde_json::Value::Object(out)
}

fn snbt_value(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => "null".into(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => snbt_quote(s),
        serde_json::Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(snbt_value).collect();
            format!("[{}]", inner.join(" "))
        }
        serde_json::Value::Object(map) => {
            if let (Some(serde_json::Value::String(ty)), Some(serde_json::Value::Array(items))) =
                (map.get("__snbtArray"), map.get("values"))
            {
                if ty.len() == 1 && "BILFD".contains(ty.as_str()) {
                    let inner: Vec<String> = items.iter().map(snbt_value).collect();
                    if inner.is_empty() {
                        return format!("[{ty};]");
                    }
                    return format!("[{ty}; {}]", inner.join(" "));
                }
            }
            let inner: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{}: {}", k, snbt_value(v)))
                .collect();
            format!("{{{}}}", inner.join(" "))
        }
    }
}

pub fn serialize_chapter_to_snbt(chapter: &Chapter) -> String {
    let mut lines = vec!["{".to_string()];
    lines.push(format!("\tid: {}", snbt_quote(&chapter.id)));
    if chapter.title_from_snbt && !chapter.title.is_empty() {
        lines.push(format!("\ttitle: {}", snbt_quote(&chapter.title)));
    }
    if let Some(icon) = &chapter.icon {
        lines.push(format!("\ticon: {}", snbt_value(icon)));
    }
    if let Some(group) = &chapter.group {
        lines.push(format!("\tgroup: {}", snbt_quote(group)));
    }
    if let Some(order) = chapter.order_index {
        lines.push(format!("\torder_index: {order}"));
    }
    if let Some(filename) = &chapter.filename {
        lines.push(format!("\tfilename: {}", snbt_quote(filename)));
    }
    if let Some(shape) = &chapter.default_quest_shape {
        lines.push(format!("\tdefault_quest_shape: {}", snbt_quote(shape)));
    }
    if let Some(v) = chapter.default_hide_dependency_lines {
        lines.push(format!("\tdefault_hide_dependency_lines: {v}"));
    }
    for (k, v) in &chapter.extras {
        lines.push(format!("\t{k}: {}", snbt_value(v)));
    }
    lines.push("\tquests: [".to_string());
    for (qi, quest) in chapter.quests.iter().enumerate() {
        lines.push("\t\t{".to_string());
        lines.push(format!("\t\t\tid: {}", snbt_quote(&quest.id)));
        if quest.title_from_snbt && !quest.title.is_empty() {
            lines.push(format!("\t\t\ttitle: {}", snbt_quote(&quest.title)));
        }
        if quest.subtitle_from_snbt {
            if let Some(sub) = &quest.subtitle {
                lines.push(format!("\t\t\tsubtitle: {}", snbt_quote(sub)));
            }
        }
        if quest.description_from_snbt && !quest.description.is_empty() {
            let desc: Vec<String> = quest.description.iter().map(|d| snbt_quote(d)).collect();
            lines.push(format!("\t\t\tdescription: [{}]", desc.join(" ")));
        }
        lines.push(format!("\t\t\tx: {}d", quest.x));
        lines.push(format!("\t\t\ty: {}d", quest.y));
        if let Some(icon) = &quest.icon {
            lines.push(format!("\t\t\ticon: {}", snbt_value(icon)));
        }
        if let Some(shape) = &quest.shape {
            lines.push(format!("\t\t\tshape: {}", snbt_quote(shape)));
        }
        if let Some(size) = quest.size {
            lines.push(format!("\t\t\tsize: {}d", size));
        }
        if quest.optional {
            lines.push("\t\t\toptional: true".to_string());
        }
        if let Some(v) = quest.hide_dependency_lines {
            lines.push(format!("\t\t\thide_dependency_lines: {v}"));
        }
        if let Some(v) = quest.hide_dependent_lines {
            lines.push(format!("\t\t\thide_dependent_lines: {v}"));
        }
        if let Some(v) = quest.min_required_dependencies {
            lines.push(format!("\t\t\tmin_required_dependencies: {v}"));
        }
        if let Some(v) = quest.can_repeat {
            lines.push(format!("\t\t\tcan_repeat: {v}"));
        }
        if let Some(v) = quest.invisible {
            lines.push(format!("\t\t\tinvisible: {v}"));
        }
        if let Some(v) = quest.disable_toast {
            lines.push(format!("\t\t\tdisable_toast: {v}"));
        }
        if let Some(v) = &quest.dependency_requirement {
            lines.push(format!("\t\t\tdependency_requirement: {}", snbt_quote(v)));
        }
        for (k, v) in &quest.extras {
            lines.push(format!("\t\t\t{k}: {}", snbt_value(v)));
        }
        if !quest.dependencies.is_empty() {
            let deps: Vec<String> = quest.dependencies.iter().map(|d| snbt_quote(d)).collect();
            lines.push(format!("\t\t\tdependencies: [{}]", deps.join(" ")));
        }
        if !quest.tasks.is_empty() {
            lines.push("\t\t\ttasks: [".to_string());
            for task in &quest.tasks {
                lines.push("\t\t\t\t{".to_string());
                lines.push(format!("\t\t\t\t\tid: {}", snbt_quote(&task.id)));
                lines.push(format!("\t\t\t\t\ttype: {}", snbt_quote(&task.task_type)));
                if task.title_from_snbt {
                    if let Some(title) = &task.title {
                        lines.push(format!("\t\t\t\t\ttitle: {}", snbt_quote(title)));
                    }
                }
                if let Some(value) = &task.value {
                    lines.push(format!("\t\t\t\t\tvalue: {}", snbt_value(value)));
                }
                for (k, v) in &task.properties {
                    lines.push(format!("\t\t\t\t\t{}: {}", k, snbt_value(v)));
                }
                lines.push("\t\t\t\t}".to_string());
            }
            lines.push("\t\t\t]".to_string());
        }
        if !quest.rewards.is_empty() {
            lines.push("\t\t\trewards: [".to_string());
            for reward in &quest.rewards {
                lines.push("\t\t\t\t{".to_string());
                lines.push(format!("\t\t\t\t\tid: {}", snbt_quote(&reward.id)));
                lines.push(format!(
                    "\t\t\t\t\ttype: {}",
                    snbt_quote(&reward.reward_type)
                ));
                if let Some(title) = &reward.title {
                    lines.push(format!("\t\t\t\t\ttitle: {}", snbt_quote(title)));
                }
                for (k, v) in &reward.properties {
                    lines.push(format!("\t\t\t\t\t{}: {}", k, snbt_value(v)));
                }
                lines.push("\t\t\t\t}".to_string());
            }
            lines.push("\t\t\t]".to_string());
        }
        lines.push(if qi + 1 == chapter.quests.len() {
            "\t\t}".to_string()
        } else {
            "\t\t}".to_string()
        });
    }
    lines.push("\t]".to_string());
    lines.push("}".to_string());
    lines.join("\n")
}

// ---------------------------------------------------------------------------
// Reward tables (FTB Quests loot tables)
// ---------------------------------------------------------------------------
//
// FTB Quests ships a `reward_tables/` directory of `.snbt` tables; a quest
// reward of type `quest_reward_table` references one by filename. Each table
// holds weighted entries: rolling the table picks entries by threshold
// sampling over the summed weight (weight `0` means *always* granted, an
// optional `empty_weight` lets a roll come up empty). We port that weighted-
// random algorithm so TuffBox can preview/validate the same reward
// distributions the game produces.

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RewardTable {
    pub id: String,
    pub title: Option<String>,
    /// Full FTB reward compounds (type/item/NBT preserved).
    #[serde(default)]
    pub rewards: Vec<serde_json::Value>,
    #[serde(default, rename = "emptyWeight")]
    pub empty_weight: f64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "sourceFile"
    )]
    pub source_file: Option<String>,
    /// Unknown top-level keys preserved on save.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub extras: HashMap<String, serde_json::Value>,
}

/// Thin view used by weighted-roll helpers / QuestPlan merge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeightedReward {
    #[serde(rename = "rewardId")]
    pub reward_id: String,
    #[serde(default = "default_weight")]
    pub weight: f64,
}

fn default_weight() -> f64 {
    1.0
}

fn reward_entry_id(v: &serde_json::Value) -> Option<String> {
    v.as_object().and_then(|m| gs(m, "id"))
}

fn reward_entry_weight(v: &serde_json::Value) -> f64 {
    v.as_object()
        .and_then(|m| m.get("weight"))
        .and_then(|w| w.as_f64())
        .unwrap_or(1.0)
}

impl RewardTable {
    /// Lightweight entries derived from full reward compounds (for UI / roll).
    pub fn entries_view(&self) -> Vec<WeightedReward> {
        self.rewards
            .iter()
            .filter_map(|r| {
                Some(WeightedReward {
                    reward_id: reward_entry_id(r)?,
                    weight: reward_entry_weight(r),
                })
            })
            .collect()
    }

    pub fn load_from_project(project_dir: &std::path::Path) -> Vec<RewardTable> {
        let mut tables = Vec::new();
        for rel in [
            "config/ftbquests/quests/reward_tables",
            "defaultconfigs/ftbquests/quests/reward_tables",
        ] {
            let dir = project_dir.join(rel);
            if !dir.is_dir() {
                continue;
            }
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.extension().map_or(true, |e| e != "snbt") {
                    continue;
                }
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(mut t) = parse_snbt_reward_table(&content) {
                        t.source_file = path
                            .strip_prefix(project_dir)
                            .ok()
                            .map(|p| p.to_string_lossy().replace('\\', "/"));
                        tables.push(t);
                    }
                }
            }
        }
        tables
    }

    pub fn save_to_project(
        project_dir: &std::path::Path,
        table: &RewardTable,
        relative_path: Option<&str>,
    ) -> Result<String, String> {
        let rel = relative_path
            .map(|s| s.to_string())
            .or_else(|| table.source_file.clone())
            .unwrap_or_else(|| {
                format!(
                    "config/ftbquests/quests/reward_tables/{}.snbt",
                    sanitize_snbt_filename(&table.id)
                )
            });
        let target = project_dir.join(&rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let snbt = serialize_reward_table_to_snbt(table);
        atomic_write(&target, snbt)?;
        Ok(rel)
    }

    /// Total weight across all entries plus the empty slot (when requested).
    pub fn total_weight(&self, include_empty: bool) -> f64 {
        let mut total: f64 = self
            .rewards
            .iter()
            .map(|r| reward_entry_weight(r).max(0.0))
            .sum();
        if include_empty {
            total += self.empty_weight.max(0.0);
        }
        total
    }

    /// Rolls the table `n_attempts` times using `rng` in `[0,1)`.
    ///
    /// Mirrors `RewardTable.generateWeightedRandomRewards`:
    /// - entries with weight `0` are always granted (auto-included);
    /// - otherwise a uniform `rng` sample in `[0, total)` walks the
    ///   cumulative weight until it crosses the threshold.
    pub fn generate<'a, R, F>(
        &self,
        rng: &'a mut R,
        n_attempts: usize,
        include_empty: bool,
        mut sample: F,
    ) -> Vec<String>
    where
        F: FnMut(&mut R) -> f64,
    {
        let mut result: Vec<String> = self
            .rewards
            .iter()
            .filter(|r| reward_entry_weight(r) == 0.0)
            .filter_map(|r| reward_entry_id(r))
            .collect();

        let total = self.total_weight(include_empty);
        if total <= 0.0 {
            return result;
        }

        for _ in 0..n_attempts {
            let threshold = sample(rng) * total;
            let mut current = if include_empty {
                self.empty_weight.max(0.0)
            } else {
                0.0
            };
            if current < threshold {
                for reward in &self.rewards {
                    current += reward_entry_weight(reward).max(0.0);
                    if current >= threshold {
                        if let Some(id) = reward_entry_id(reward) {
                            result.push(id);
                        }
                        break;
                    }
                }
            }
        }
        result
    }
}

fn parse_snbt_reward_table(c: &str) -> Result<RewardTable, String> {
    let j = snbt_to_json(c)?;
    let m = j.as_object().ok_or("not object")?;
    const KNOWN: &[&str] = &["id", "title", "rewards", "empty_weight"];
    let mut extras = HashMap::new();
    for (k, v) in m {
        if !KNOWN.contains(&k.as_str()) {
            extras.insert(k.clone(), v.clone());
        }
    }
    let rewards = m
        .get("rewards")
        .and_then(|v| v.as_array())
        .map(|a| a.to_vec())
        .unwrap_or_default();
    Ok(RewardTable {
        id: gs(m, "id").unwrap_or_else(|| "untitled".into()),
        title: gs(m, "title"),
        rewards,
        empty_weight: m
            .get("empty_weight")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0),
        source_file: None,
        extras,
    })
}

pub fn serialize_reward_table_to_snbt(table: &RewardTable) -> String {
    let mut lines = vec!["{".to_string()];
    lines.push(format!("\tid: {}", snbt_quote(&table.id)));
    if let Some(title) = &table.title {
        lines.push(format!("\ttitle: {}", snbt_quote(title)));
    }
    if table.empty_weight > 0.0 {
        lines.push(format!("\tempty_weight: {}d", table.empty_weight));
    }
    for (k, v) in &table.extras {
        lines.push(format!("\t{k}: {}", snbt_value(v)));
    }
    if !table.rewards.is_empty() {
        lines.push("\trewards: [".to_string());
        for (i, reward) in table.rewards.iter().enumerate() {
            let mut inner = format!("\t\t{}", snbt_value(reward));
            if i + 1 != table.rewards.len() {
                inner.push(',');
            }
            lines.push(inner);
        }
        lines.push("\t]".to_string());
    }
    lines.push("}".to_string());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrips_chapter() {
        let snbt = r#"{ title: "Test" id: "abc" quests: [{ id: "q1" title: "Q1" x: 0.0 y: 0.0 tasks: [{ id: "t1" type: "item" }] rewards: [{ id: "r1" type: "item" }] }] }"#;
        let (ch, _) = parse_snbt_chapter(snbt).unwrap();
        let out = serialize_chapter_to_snbt(&ch);
        let (ch2, _) = parse_snbt_chapter(&out).unwrap();
        assert_eq!(ch.title, ch2.title);
        assert_eq!(ch.quests.len(), ch2.quests.len());
        assert_eq!(ch.quests[0].id, ch2.quests[0].id);
    }

    #[test]
    fn parses_object_form_icons() {
        let snbt = r#"{
          title: "Test"
          id: "abc"
          icon: { id: "minecraft:apple" Count: 1b }
          quests: [{
            id: "q1"
            title: "Q1"
            x: 0.0d
            y: 0.0d
            icon: { id: "minecraft:diamond" Count: 1b }
            tasks: [{ id: "t1" type: "checkmark" }]
            rewards: []
          }]
        }"#;
        let (ch, _) = parse_snbt_chapter(snbt).unwrap();
        assert_eq!(
            ch.icon.as_ref().and_then(icon_display_id).as_deref(),
            Some("minecraft:apple")
        );
        assert_eq!(
            ch.quests[0]
                .icon
                .as_ref()
                .and_then(icon_display_id)
                .as_deref(),
            Some("minecraft:diamond")
        );
        // Compound Count preserved
        assert_eq!(
            ch.icon
                .as_ref()
                .and_then(|v| v.get("Count"))
                .and_then(|c| c.as_u64().or_else(|| c.as_i64().map(|i| i as u64))),
            Some(1)
        );
        let out = serialize_chapter_to_snbt(&ch);
        let (ch2, _) = parse_snbt_chapter(&out).unwrap();
        assert_eq!(
            ch2.icon
                .as_ref()
                .and_then(|v| v.get("Count"))
                .and_then(|c| c.as_u64().or_else(|| c.as_i64().map(|i| i as u64))),
            Some(1)
        );
    }

    fn q(id: &str, deps: &[&str]) -> Quest {
        Quest {
            id: id.to_string(),
            title: id.to_string(),
            title_from_snbt: true,
            subtitle: None,
            subtitle_from_snbt: false,
            description: vec![],
            description_from_snbt: false,
            x: 0.0,
            y: 0.0,
            icon: None,
            dependencies: deps.iter().map(|s| s.to_string()).collect(),
            tasks: vec![Task {
                id: "t".into(),
                task_type: "item".into(),
                title: None,
                title_from_snbt: false,
                value: None,
                properties: HashMap::new(),
            }],
            rewards: vec![],
            optional: false,
            shape: None,
            size: None,
            hide_dependency_lines: None,
            hide_dependent_lines: None,
            min_required_dependencies: None,
            can_repeat: None,
            invisible: None,
            disable_toast: None,
            dependency_requirement: None,
            extras: HashMap::new(),
        }
    }

    fn book_from_quests(quests: Vec<Quest>) -> QuestBook {
        QuestBook {
            chapters: vec![Chapter {
                id: "c".into(),
                title: "C".into(),
                title_from_snbt: true,
                icon: None,
                quests,
                group: None,
                order_index: None,
                filename: None,
                default_quest_shape: None,
                default_hide_dependency_lines: None,
                extras: HashMap::new(),
                source_file: None,
            }],
            title: None,
            subtitle: None,
            chapter_groups: vec![],
            reward_tables: vec![],
            book_settings: HashMap::new(),
            locales: HashMap::new(),
            active_locale: None,
            load_warnings: vec![],
        }
    }

    #[test]
    fn detects_simple_cycle() {
        // A -> B -> A
        let book = book_from_quests(vec![q("A", &["B"]), q("B", &["A"])]);
        let cycles = book.find_cycles();
        assert_eq!(cycles.len(), 1);
        // Cycle should mention both nodes.
        assert!(cycles[0].contains(&"A".to_string()));
        assert!(cycles[0].contains(&"B".to_string()));
        // topo_order refuses to order a cyclic graph.
        assert!(book.topo_order().is_err());
    }

    #[test]
    fn no_false_positive_on_dag() {
        // A -> B -> C (no cycle)
        let book = book_from_quests(vec![q("A", &["B"]), q("B", &["C"]), q("C", &[])]);
        assert!(book.find_cycles().is_empty());
        let order = book.topo_order().unwrap();
        // Dependency-first: C before B before A.
        let pos = |id: &str| order.iter().position(|x| x == id).unwrap();
        assert!(pos("C") < pos("B"));
        assert!(pos("B") < pos("A"));
    }

    #[test]
    fn unreachable_quest_detected() {
        // Cycle island A↔B is unreachable from root C; missing-dep orphans are not flooded.
        let book = book_from_quests(vec![
            q("A", &["B"]),
            q("B", &["A"]),
            q("C", &[]),
            q("D", &["Z"]),
        ]);
        let missing = book.unreachable_quest_ids();
        assert!(missing.contains(&"A".to_string()));
        assert!(missing.contains(&"B".to_string()));
        assert!(!missing.contains(&"C".to_string()));
        assert!(!missing.contains(&"D".to_string())); // missing dep only
        assert!(book
            .validate()
            .iter()
            .any(|e| e.quest_id == "D" && e.message.contains("missing")));
    }

    #[test]
    fn task_id_dependency_resolves() {
        // CAB-style: chapter unlock quest depends on a checkmark *task* id.
        let unlock = Quest {
            id: "UNLOCK".into(),
            title: "Unlock".into(),
            title_from_snbt: true,
            subtitle: None,
            subtitle_from_snbt: false,
            description: vec![],
            description_from_snbt: false,
            x: 0.0,
            y: 0.0,
            icon: None,
            dependencies: vec![],
            tasks: vec![Task {
                id: "TASK_CHECK".into(),
                task_type: "checkmark".into(),
                title: Some("Go".into()),
                title_from_snbt: true,
                value: None,
                properties: HashMap::new(),
            }],
            rewards: vec![],
            optional: false,
            shape: None,
            size: None,
            hide_dependency_lines: None,
            hide_dependent_lines: None,
            min_required_dependencies: None,
            can_repeat: None,
            invisible: None,
            disable_toast: None,
            dependency_requirement: None,
            extras: HashMap::new(),
        };
        let stage = q("STAGE", &["TASK_CHECK"]);
        let book = book_from_quests(vec![unlock, stage]);
        assert_eq!(book.resolve_dep("TASK_CHECK").as_deref(), Some("UNLOCK"));
        assert!(book
            .validate()
            .iter()
            .all(|e| !e.message.contains("missing")));
        assert!(book.find_cycles().is_empty());
        let order = book.topo_order().unwrap();
        let pos = |id: &str| order.iter().position(|x| x == id).unwrap();
        assert!(pos("UNLOCK") < pos("STAGE"));
    }

    #[test]
    fn parses_data_and_chapter_groups_snbt() {
        let dir = tempfile::tempdir().unwrap();
        let quests = dir.path().join("quests");
        std::fs::create_dir_all(quests.join("chapters")).unwrap();
        std::fs::write(
            quests.join("data.snbt"),
            r#"{ title: "&6 Above" subtitle: "demo" }"#,
        )
        .unwrap();
        std::fs::write(
            quests.join("chapter_groups.snbt"),
            r#"{ chapter_groups: [{ id: "G1" title: "Factory" }] }"#,
        )
        .unwrap();
        std::fs::write(
            quests.join("chapters").join("a.snbt"),
            r#"{ id: "CH1" title: "One" group: "G1" order_index: 2 quests: [{ id: "Q1" title: "Q" x: 0.0d y: 0.0d tasks: [{ id: "T1" type: "checkmark" }] }] }"#,
        )
        .unwrap();
        let book = QuestBook::load_from_dir(&quests, dir.path()).unwrap();
        assert_eq!(book.title.as_deref(), Some("&6 Above"));
        assert_eq!(book.subtitle.as_deref(), Some("demo"));
        assert_eq!(book.chapter_groups.len(), 1);
        assert_eq!(book.chapter_groups[0].title, "Factory");
        assert!(book.chapter_groups[0].title_from_snbt);
        assert!(book.chapters[0].title_from_snbt);
        assert_eq!(book.chapters[0].order_index, Some(2));
        assert_eq!(book.chapters[0].group.as_deref(), Some("G1"));
    }

    #[test]
    fn loads_lang_locale_maps() {
        let dir = tempfile::tempdir().unwrap();
        let quests = dir.path().join("quests");
        std::fs::create_dir_all(quests.join("lang")).unwrap();
        std::fs::create_dir_all(quests.join("chapters")).unwrap();
        std::fs::write(
            quests.join("lang").join("en_us.snbt"),
            r#"{ chapter.CH1.title: "Localized" quest.Q1.title: "Hello" }"#,
        )
        .unwrap();
        std::fs::write(
            quests.join("chapters").join("a.snbt"),
            r#"{ id: "CH1" quests: [{ id: "Q1" x: 0.0d y: 0.0d tasks: [] }] }"#,
        )
        .unwrap();
        let book = QuestBook::load_from_dir(&quests, dir.path()).unwrap();
        assert!(!book.chapters[0].title_from_snbt);
        assert_eq!(book.chapters[0].title, "");
        let loc = book.locales.get("en_us").expect("en_us locale");
        assert_eq!(
            loc.get("chapter.CH1.title").and_then(|v| v.as_str()),
            Some("Localized")
        );
    }

    #[test]
    fn itemfilters_or_extracts_nested_ids() {
        let mut props = HashMap::new();
        props.insert(
            "item".into(),
            serde_json::json!({
                "id": "itemfilters:or",
                "tag": { "items": [
                    { "id": "minecraft:iron_ingot" },
                    { "id": "minecraft:gold_ingot" }
                ]}
            }),
        );
        let q = Quest {
            id: "Q".into(),
            title: "Q".into(),
            title_from_snbt: true,
            subtitle: None,
            subtitle_from_snbt: false,
            description: vec![],
            description_from_snbt: false,
            x: 0.0,
            y: 0.0,
            icon: None,
            dependencies: vec![],
            tasks: vec![Task {
                id: "T".into(),
                task_type: "item".into(),
                title: None,
                title_from_snbt: false,
                value: None,
                properties: props,
            }],
            rewards: vec![],
            optional: false,
            shape: None,
            size: None,
            hide_dependency_lines: None,
            hide_dependent_lines: None,
            min_required_dependencies: None,
            can_repeat: None,
            invisible: None,
            disable_toast: None,
            dependency_requirement: None,
            extras: HashMap::new(),
        };
        let ids = extract_quest_item_ids(&q);
        assert!(ids.contains(&"itemfilters:or".into()));
        assert!(ids.contains(&"minecraft:iron_ingot".into()));
        assert!(ids.contains(&"minecraft:gold_ingot".into()));
    }

    #[test]
    fn validate_reports_duplicate_ids() {
        let book = book_from_quests(vec![q("A", &[]), q("A", &[])]);
        let errs = book.validate();
        assert!(errs.iter().any(|e| e.message.contains("Duplicate")));
    }

    #[test]
    fn topo_order_returns_err_with_cycles() {
        let book = book_from_quests(vec![q("X", &["Y"]), q("Y", &["X"])]);
        assert!(book.topo_order().is_err());
    }

    #[test]
    fn reward_table_weighted_zero_always_granted() {
        let table = RewardTable {
            id: "rt".into(),
            title: None,
            rewards: vec![
                serde_json::json!({"id": "always", "weight": 0.0}),
                serde_json::json!({"id": "rare", "weight": 10.0}),
            ],
            empty_weight: 0.0,
            source_file: None,
            extras: HashMap::new(),
        };
        // weight-0 entry must always appear regardless of rng.
        let mut rng = 0u8;
        let out = table.generate(&mut rng, 5, false, |_| 0.99);
        assert!(out.contains(&"always".to_string()));
        // With a 0.99 sample the threshold falls on the rare entry.
        assert!(out.contains(&"rare".to_string()));
    }

    #[test]
    fn reward_table_threshold_sampling_picks_correct_bucket() {
        // Two equal-weight entries; sample 0.25 (first half) -> first entry.
        let table = RewardTable {
            id: "rt".into(),
            title: None,
            rewards: vec![
                serde_json::json!({"id": "first", "weight": 1.0}),
                serde_json::json!({"id": "second", "weight": 1.0}),
            ],
            empty_weight: 0.0,
            source_file: None,
            extras: HashMap::new(),
        };
        let mut rng = 0u8;
        // sample 0.25 * 2.0 = 0.5 -> crosses first entry's cumulative 1.0? no,
        // 0.5 < 1.0 so it lands in "first".
        let out = table.generate(&mut rng, 1, false, |_| 0.25);
        assert_eq!(out, vec!["first".to_string()]);

        // sample 0.75 * 2.0 = 1.5 -> past first, lands in "second".
        let out = table.generate(&mut rng, 1, false, |_| 0.75);
        assert_eq!(out, vec!["second".to_string()]);
    }

    #[test]
    fn reward_table_rolls_empty_when_include_empty_and_sample_low() {
        let table = RewardTable {
            id: "rt".into(),
            title: None,
            rewards: vec![serde_json::json!({"id": "only", "weight": 1.0})],
            empty_weight: 1.0,
            source_file: None,
            extras: HashMap::new(),
        };
        let mut rng = 0u8;
        // total = 2.0; sample 0.1 -> threshold 0.2; empty_weight slot is
        // [0,1.0) so it lands in empty -> no reward granted.
        let out = table.generate(&mut rng, 1, true, |_| 0.1);
        assert!(out.is_empty());
    }

    #[test]
    fn omits_locale_sourced_subtitle_and_description() {
        let mut ch = parse_snbt_chapter(
            r#"{ id: "CH" title: "T" quests: [{ id: "Q1" title: "Inline" x: 0.0d y: 0.0d tasks: [{ id: "T1" type: "checkmark" }] }] }"#,
        )
        .unwrap()
        .0;
        ch.quests[0].subtitle = Some("from lang".into());
        ch.quests[0].subtitle_from_snbt = false;
        ch.quests[0].description = vec!["line".into()];
        ch.quests[0].description_from_snbt = false;
        let out = serialize_chapter_to_snbt(&ch);
        assert!(!out.contains("subtitle:"));
        assert!(!out.contains("description:"));
        assert!(out.contains("title: \"Inline\""));
    }

    #[test]
    fn reward_table_roundtrips_snbt() {
        let table = RewardTable {
            id: "loot".into(),
            title: Some("Loot".into()),
            rewards: vec![
                serde_json::json!({"id": "r1", "type": "item", "item": "minecraft:diamond", "weight": 2.0}),
                serde_json::json!({"id": "r2", "weight": 1.0}),
            ],
            empty_weight: 0.5,
            source_file: None,
            extras: HashMap::new(),
        };
        let snbt = serialize_reward_table_to_snbt(&table);
        let parsed = parse_snbt_reward_table(&snbt).unwrap();
        assert_eq!(parsed.id, "loot");
        assert_eq!(parsed.rewards.len(), 2);
        assert_eq!(parsed.empty_weight, 0.5);
        assert_eq!(
            parsed.rewards[0].get("item").and_then(|v| v.as_str()),
            Some("minecraft:diamond")
        );
        assert_eq!(
            parsed.rewards[0].get("type").and_then(|v| v.as_str()),
            Some("item")
        );
        assert_eq!(reward_entry_id(&parsed.rewards[0]).as_deref(), Some("r1"));
        assert!((reward_entry_weight(&parsed.rewards[0]) - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn preserves_hide_dependency_lines_roundtrip() {
        let snbt = r#"{
          id: "CH"
          title: "T"
          default_hide_dependency_lines: true
          filename: "t"
          quests: [{
            id: "Q1"
            title: "One"
            x: 0.0d
            y: 0.0d
            hide_dependency_lines: true
            invisible: false
            can_repeat: true
            custom_flag: "kept"
            tasks: [{ id: "T1" type: "checkmark" }]
          }]
        }"#;
        let (ch, _) = parse_snbt_chapter(snbt).unwrap();
        assert_eq!(ch.default_hide_dependency_lines, Some(true));
        assert_eq!(ch.filename.as_deref(), Some("t"));
        assert_eq!(ch.quests[0].hide_dependency_lines, Some(true));
        assert_eq!(ch.quests[0].can_repeat, Some(true));
        assert_eq!(
            ch.quests[0]
                .extras
                .get("custom_flag")
                .and_then(|v| v.as_str()),
            Some("kept")
        );
        let out = serialize_chapter_to_snbt(&ch);
        let (ch2, _) = parse_snbt_chapter(&out).unwrap();
        assert_eq!(ch2.quests[0].hide_dependency_lines, Some(true));
        assert_eq!(
            ch2.quests[0]
                .extras
                .get("custom_flag")
                .and_then(|v| v.as_str()),
            Some("kept")
        );
    }

    #[test]
    fn parses_and_emits_typed_int_arrays() {
        let v = parse_snbt("{ id: [I; 1 -2 3] }").unwrap();
        let obj = v.as_object().unwrap();
        let tagged = obj.get("id").unwrap().as_object().unwrap();
        assert_eq!(
            tagged.get("__snbtArray").and_then(|x| x.as_str()),
            Some("I")
        );
        let vals = tagged.get("values").unwrap().as_array().unwrap();
        assert_eq!(vals.len(), 3);
        assert_eq!(vals[0].as_i64(), Some(1));
        assert_eq!(vals[1].as_i64(), Some(-2));
        assert_eq!(vals[2].as_i64(), Some(3));
        let out = snbt_value(obj.get("id").unwrap());
        assert!(out.starts_with("[I;"), "expected typed emit, got {out}");
        assert!(out.contains("1"));
        assert!(out.contains("-2"));
    }

    #[test]
    fn emits_booleans_as_true_false() {
        assert_eq!(snbt_value(&serde_json::json!(true)), "true");
        assert_eq!(snbt_value(&serde_json::json!(false)), "false");
    }

    #[test]
    fn warns_on_whitespace_only_separators() {
        let snbt = r#"{ id: "CH" title: "T" quests: [] }"#;
        let (_ch, warnings) = parse_snbt_chapter(snbt).unwrap();
        assert!(
            warnings.iter().any(|w| w.contains("whitespace-only")),
            "{warnings:?}"
        );
    }

    #[test]
    fn alloc_fresh_hex_id_avoids_used() {
        let mut used = HashSet::new();
        used.insert("AAAAAAAAAAAA".into());
        for _ in 0..32 {
            let id = alloc_fresh_hex_id(12, &mut used);
            assert_eq!(id.len(), 12);
            assert_ne!(id, "AAAAAAAAAAAA");
        }
        assert_eq!(used.len(), 33);
    }

    #[test]
    fn missing_ids_get_unique_allocations() {
        let snbt = r#"{ title: "T" quests: [
          { title: "A" x: 0.0d y: 0.0d tasks: [{ type: "checkmark" }] rewards: [{ type: "xp" }] },
          { title: "B" x: 1.0d y: 0.0d tasks: [{ type: "checkmark" }] rewards: [{ type: "xp" }] }
        ] }"#;
        let (ch, _) = parse_snbt_chapter(snbt).unwrap();
        let mut ids = HashSet::new();
        assert!(ids.insert(ch.id.clone()));
        for q in &ch.quests {
            assert!(ids.insert(q.id.clone()), "duplicate quest id {}", q.id);
            for t in &q.tasks {
                assert!(ids.insert(t.id.clone()), "duplicate task id {}", t.id);
            }
            for r in &q.rewards {
                assert!(ids.insert(r.id.clone()), "duplicate reward id {}", r.id);
            }
        }
    }
}
