//! FTB Quests ↔ KubeJS bridge: index `FTBQuestsEvents` handlers, render stubs,
//! and manage `kubejs/server_scripts/tuffbox_ftb_quests.js`.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const MANAGED_RELATIVE: &str = "kubejs/server_scripts/tuffbox_ftb_quests.js";

const MANAGED_HEADER: &str = r#"// Generated / managed by TuffBox Quests · KubeJS
// FTB XMod Compat: FTBQuestsEvents.customTask / customReward / completed / started
// After editing: /reload then /ftbquests reload (or restart) so custom handlers register.
"#;

const HANDLER_KINDS: &[&str] = &["customTask", "customReward", "completed", "started"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QuestKubeJsHandler {
    pub kind: String,
    pub id: String,
    pub relative_path: String,
    pub line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestKubeJsScriptFile {
    pub relative_path: String,
    pub name: String,
    pub managed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestKubeJsBookObject {
    pub kind: String,
    pub id: String,
    pub quest_id: String,
    pub quest_title: String,
    pub chapter_id: String,
    pub title: Option<String>,
    pub max_progress: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestKubeJsBinding {
    pub kind: String,
    pub id: String,
    pub quest_id: String,
    pub quest_title: String,
    pub chapter_id: String,
    pub title: Option<String>,
    pub status: String,
    pub handlers: Vec<QuestKubeJsHandler>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestKubeJsAudit {
    pub linked: usize,
    pub missing: usize,
    pub orphan: usize,
    pub bindings: Vec<QuestKubeJsBinding>,
    pub orphan_handlers: Vec<QuestKubeJsHandler>,
    pub scripts: Vec<QuestKubeJsScriptFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestKubeJsTemplateParams {
    pub kind: String,
    pub id: String,
    #[serde(default)]
    pub max_progress: Option<u64>,
    #[serde(default)]
    pub block_id: Option<String>,
    #[serde(default)]
    pub item_id: Option<String>,
    #[serde(default)]
    pub count: Option<u64>,
    #[serde(default)]
    pub title: Option<String>,
}

fn line_at(content: &str, byte_idx: usize) -> usize {
    content[..byte_idx.min(content.len())]
        .bytes()
        .filter(|&b| b == b'\n')
        .count()
        + 1
}

fn is_hex_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 32 && s.chars().all(|c| c.is_ascii_hexdigit())
}

fn scan_ftb_events(content: &str, relative_path: &str, out: &mut Vec<QuestKubeJsHandler>) {
    let needle = "FTBQuestsEvents.";
    let mut search_from = 0;
    while let Some(rel) = content[search_from..].find(needle) {
        let start = search_from + rel;
        let after = start + needle.len();
        let rest = &content[after..];
        let Some(kind) = HANDLER_KINDS.iter().find(|k| rest.starts_with(*k)) else {
            search_from = after;
            continue;
        };
        let after_kind = after + kind.len();
        let mut i = after_kind;
        let bytes = content.as_bytes();
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'(' {
            search_from = after_kind;
            continue;
        }
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let quote = bytes[i];
        if quote != b'\'' && quote != b'"' {
            search_from = i;
            continue;
        }
        i += 1;
        let id_start = i;
        while i < bytes.len() && bytes[i] != quote {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let id = &content[id_start..i];
        if is_hex_id(id) {
            out.push(QuestKubeJsHandler {
                kind: (*kind).to_string(),
                id: id.to_string(),
                relative_path: relative_path.replace('\\', "/"),
                line: line_at(content, start),
            });
        }
        search_from = i + 1;
    }
}

fn scan_add_progress(content: &str, relative_path: &str, out: &mut Vec<QuestKubeJsHandler>) {
    let needle = "addProgress(";
    let mut search_from = 0;
    while let Some(rel) = content[search_from..].find(needle) {
        let start = search_from + rel;
        let mut i = start + needle.len();
        let bytes = content.as_bytes();
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let quote = bytes[i];
        if quote != b'\'' && quote != b'"' {
            search_from = i + 1;
            continue;
        }
        i += 1;
        let id_start = i;
        while i < bytes.len() && bytes[i] != quote {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let id = &content[id_start..i];
        if is_hex_id(id)
            && !out
                .iter()
                .any(|h| h.id.eq_ignore_ascii_case(id) && h.kind == "customTask")
        {
            out.push(QuestKubeJsHandler {
                kind: "customTask".into(),
                id: id.to_string(),
                relative_path: relative_path.replace('\\', "/"),
                line: line_at(content, start),
            });
        }
        search_from = i + 1;
    }
}

pub fn managed_path(project_dir: &Path) -> PathBuf {
    project_dir.join(MANAGED_RELATIVE.replace('/', std::path::MAIN_SEPARATOR_STR))
}

pub fn ensure_managed_script(project_dir: &Path) -> Result<String, String> {
    let path = managed_path(project_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    if !path.exists() {
        std::fs::write(&path, format!("{MANAGED_HEADER}\n")).map_err(|e| e.to_string())?;
    }
    Ok(MANAGED_RELATIVE.to_string())
}

pub fn index_handlers_in_content(content: &str, relative_path: &str) -> Vec<QuestKubeJsHandler> {
    let mut out = Vec::new();
    scan_ftb_events(content, relative_path, &mut out);
    scan_add_progress(content, relative_path, &mut out);
    out
}

fn walk_js_files(dir: &Path, cb: &mut dyn FnMut(&Path)) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            walk_js_files(&p, cb);
        } else if p
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("js") || e.eq_ignore_ascii_case("ts"))
        {
            cb(&p);
        }
    }
}

pub fn list_quest_scripts(project_dir: &Path) -> Vec<QuestKubeJsScriptFile> {
    let root = project_dir.join("kubejs").join("server_scripts");
    let mut files = Vec::new();
    walk_js_files(&root, &mut |p| {
        let Ok(rel) = p.strip_prefix(project_dir) else {
            return;
        };
        let relative = rel.to_string_lossy().replace('\\', "/");
        let name = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("script.js")
            .to_string();
        let lower = relative.to_lowercase();
        let name_l = name.to_lowercase();
        let content_hint = std::fs::read_to_string(p).unwrap_or_default();
        let quest_related = name_l.contains("quest")
            || lower.contains("quest")
            || content_hint.contains("FTBQuests")
            || content_hint.contains("ftbquests")
            || relative == MANAGED_RELATIVE;
        if !quest_related {
            return;
        }
        files.push(QuestKubeJsScriptFile {
            managed: relative == MANAGED_RELATIVE,
            relative_path: relative,
            name,
        });
    });
    files.sort_by(|a, b| {
        b.managed
            .cmp(&a.managed)
            .then_with(|| a.relative_path.cmp(&b.relative_path))
    });
    if !files.iter().any(|f| f.relative_path == MANAGED_RELATIVE) {
        files.insert(
            0,
            QuestKubeJsScriptFile {
                relative_path: MANAGED_RELATIVE.to_string(),
                name: "tuffbox_ftb_quests.js".into(),
                managed: true,
            },
        );
    }
    files
}

pub fn index_all_handlers(project_dir: &Path) -> Vec<QuestKubeJsHandler> {
    let mut all = Vec::new();
    for script in list_quest_scripts(project_dir) {
        let path = project_dir.join(
            script
                .relative_path
                .replace('/', std::path::MAIN_SEPARATOR_STR),
        );
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        all.extend(index_handlers_in_content(&content, &script.relative_path));
    }
    all
}

pub fn book_custom_objects(book: &serde_json::Value) -> Vec<QuestKubeJsBookObject> {
    let mut out = Vec::new();
    let Some(chapters) = book.get("chapters").and_then(|c| c.as_array()) else {
        return out;
    };
    for ch in chapters {
        let chapter_id = ch
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let Some(quests) = ch.get("quests").and_then(|q| q.as_array()) else {
            continue;
        };
        for q in quests {
            let quest_id = q
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let quest_title = q
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("(untitled)")
                .to_string();
            if let Some(tasks) = q.get("tasks").and_then(|t| t.as_array()) {
                for t in tasks {
                    let ty = t.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    if ty != "custom" {
                        continue;
                    }
                    let id = t
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    if id.is_empty() {
                        continue;
                    }
                    let title = t
                        .get("title")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let max_progress = t
                        .get("properties")
                        .and_then(|p| p.get("max_progress"))
                        .and_then(|v| v.as_u64().or_else(|| v.as_i64().map(|n| n as u64)));
                    out.push(QuestKubeJsBookObject {
                        kind: "customTask".into(),
                        id,
                        quest_id: quest_id.clone(),
                        quest_title: quest_title.clone(),
                        chapter_id: chapter_id.clone(),
                        title,
                        max_progress,
                    });
                }
            }
            if let Some(rewards) = q.get("rewards").and_then(|r| r.as_array()) {
                for r in rewards {
                    let ty = r.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    if ty != "custom" {
                        continue;
                    }
                    let id = r
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    if id.is_empty() {
                        continue;
                    }
                    let title = r
                        .get("title")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    out.push(QuestKubeJsBookObject {
                        kind: "customReward".into(),
                        id,
                        quest_id: quest_id.clone(),
                        quest_title: quest_title.clone(),
                        chapter_id: chapter_id.clone(),
                        title,
                        max_progress: None,
                    });
                }
            }
        }
    }
    out
}

pub fn audit_bindings(project_dir: &Path, book: &serde_json::Value) -> QuestKubeJsAudit {
    let scripts = list_quest_scripts(project_dir);
    let handlers = index_all_handlers(project_dir);
    let objects = book_custom_objects(book);

    let mut bindings = Vec::new();
    let mut linked = 0usize;
    let mut missing = 0usize;

    for obj in &objects {
        let matched: Vec<_> = handlers
            .iter()
            .filter(|h| h.id.eq_ignore_ascii_case(&obj.id) && h.kind == obj.kind)
            .cloned()
            .collect();
        let status = if matched.is_empty() {
            missing += 1;
            "missing"
        } else {
            linked += 1;
            "linked"
        };
        bindings.push(QuestKubeJsBinding {
            kind: obj.kind.clone(),
            id: obj.id.clone(),
            quest_id: obj.quest_id.clone(),
            quest_title: obj.quest_title.clone(),
            chapter_id: obj.chapter_id.clone(),
            title: obj.title.clone(),
            status: status.into(),
            handlers: matched,
        });
    }

    let book_ids: std::collections::HashSet<String> =
        objects.iter().map(|o| o.id.to_ascii_uppercase()).collect();
    let mut orphan_handlers = Vec::new();
    for h in &handlers {
        if h.kind == "completed" || h.kind == "started" {
            continue;
        }
        if !book_ids.contains(&h.id.to_ascii_uppercase()) {
            orphan_handlers.push(h.clone());
        }
    }
    orphan_handlers.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.id.cmp(&b.id)));
    orphan_handlers.dedup_by(|a, b| a.kind == b.kind && a.id.eq_ignore_ascii_case(&b.id));

    QuestKubeJsAudit {
        linked,
        missing,
        orphan: orphan_handlers.len(),
        bindings,
        orphan_handlers,
        scripts,
    }
}

fn escape_js_single(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'")
}

pub fn render_template(params: &QuestKubeJsTemplateParams) -> Result<String, String> {
    let id = params.id.trim();
    if id.is_empty() {
        return Err("id is required".into());
    }
    let id_esc = escape_js_single(id);
    let max_progress = params.max_progress.unwrap_or(1).max(1);
    let block = params
        .block_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("minecraft:stone");
    let item = params
        .item_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("minecraft:diamond");
    let count = params.count.unwrap_or(1).max(1);
    let title_comment = params
        .title
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|t| format!(" // {t}"))
        .unwrap_or_default();
    let block_esc = escape_js_single(block);
    let item_esc = escape_js_single(item);

    let body = match params.kind.as_str() {
        "customTask" | "custom_task" => format!(
            r#"
FTBQuestsEvents.customTask('{id_esc}', event => {{{title_comment}
  event.maxProgress = {max_progress}
  event.setCheckTimer(20)
  event.setCheck((task, player) => {{
    // TODO: set completion condition
    // task.progress++
  }})
}})
"#
        ),
        "breakBlock" | "break_block" | "mineBlock" => format!(
            r#"
// Break / mine block → progress on custom task {id_esc}
FTBQuestsEvents.customTask('{id_esc}', event => {{{title_comment}
  event.maxProgress = {max_progress}
}})

BlockEvents.broken('{block_esc}', event => {{
  const player = event.player
  if (!player) return
  const data = FTBQuests.getData(player)
  if (!data) return
  data.addProgress('{id_esc}', 1)
}})
"#
        ),
        "customReward" | "custom_reward" => format!(
            r#"
FTBQuestsEvents.customReward('{id_esc}', event => {{{title_comment}
  event.player.give(Item.of('{item_esc}', {count}))
  // event.player.tell('Reward claimed!')
}})
"#
        ),
        "completed" => format!(
            r#"
FTBQuestsEvents.completed('{id_esc}', event => {{{title_comment}
  if (event.player) {{
    event.player.tell('Quest completed!')
  }}
}})
"#
        ),
        "started" => format!(
            r#"
FTBQuestsEvents.started('{id_esc}', event => {{{title_comment}
  if (event.player) {{
    event.player.tell('Quest started!')
  }}
}})
"#
        ),
        other => return Err(format!("unknown template kind: {other}")),
    };
    Ok(body.trim_start().to_string())
}

pub fn append_handler(project_dir: &Path, snippet: &str) -> Result<String, String> {
    ensure_managed_script(project_dir)?;
    let path = managed_path(project_dir);
    let mut existing = if path.exists() {
        std::fs::read_to_string(&path).unwrap_or_default()
    } else {
        format!("{MANAGED_HEADER}\n")
    };
    if existing.trim().is_empty() {
        existing = format!("{MANAGED_HEADER}\n");
    }
    if !existing.ends_with('\n') {
        existing.push('\n');
    }
    existing.push('\n');
    existing.push_str(snippet.trim_end());
    existing.push('\n');
    std::fs::write(&path, existing).map_err(|e| e.to_string())?;
    Ok(MANAGED_RELATIVE.to_string())
}

pub fn read_script(project_dir: &Path, relative_path: &str) -> Result<String, String> {
    let rel = relative_path.replace('\\', "/");
    if !rel.starts_with("kubejs/") {
        return Err("script must be under kubejs/".into());
    }
    let path = project_dir.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
    if !path.exists() && rel == MANAGED_RELATIVE {
        ensure_managed_script(project_dir)?;
    }
    std::fs::read_to_string(&path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn index_handlers_finds_events() {
        let src = r#"
FTBQuestsEvents.customTask('AABBCCDDEEFF0011', event => {})
FTBQuestsEvents.customReward("1122334455667788", event => {})
FTBQuestsEvents.completed('99AABBCCDDEEFF00', event => {})
"#;
        let hs = index_handlers_in_content(src, "kubejs/server_scripts/t.js");
        assert_eq!(hs.len(), 3);
        assert_eq!(hs[0].kind, "customTask");
        assert_eq!(hs[0].id, "AABBCCDDEEFF0011");
        assert_eq!(hs[0].line, 2);
    }

    #[test]
    fn render_break_block() {
        let s = render_template(&QuestKubeJsTemplateParams {
            kind: "breakBlock".into(),
            id: "ABCDEF0123456789".into(),
            max_progress: Some(10),
            block_id: Some("minecraft:iron_ore".into()),
            item_id: None,
            count: None,
            title: Some("Mine iron".into()),
        })
        .unwrap();
        assert!(s.contains("BlockEvents.broken('minecraft:iron_ore'"));
        assert!(s.contains("addProgress('ABCDEF0123456789'"));
        assert!(s.contains("event.maxProgress = 10"));
    }

    #[test]
    fn audit_missing_and_linked() {
        let dir = tempfile::tempdir().unwrap();
        let scripts = dir.path().join("kubejs/server_scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        std::fs::write(
            scripts.join("tuffbox_ftb_quests.js"),
            "FTBQuestsEvents.customTask('AAAABBBBCCCCDDDD', event => {})\n",
        )
        .unwrap();
        let book = json!({
            "chapters": [{
                "id": "ch1",
                "quests": [{
                    "id": "q1",
                    "title": "Test",
                    "tasks": [
                        { "id": "AAAABBBBCCCCDDDD", "type": "custom", "title": "Linked" },
                        { "id": "1111222233334444", "type": "custom", "title": "Missing" }
                    ],
                    "rewards": []
                }]
            }]
        });
        let audit = audit_bindings(dir.path(), &book);
        assert_eq!(audit.linked, 1);
        assert_eq!(audit.missing, 1);
    }
}
