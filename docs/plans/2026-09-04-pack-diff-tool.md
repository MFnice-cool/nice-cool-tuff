# Pack Diff Tool (Compare builds & backups) — Implementation Plan

> **For Hermes:** Implement task-by-task. Backend Rust tasks go through OMP per repo protocol; the small UI wiring task can be done directly.

**Goal:** Give the player a "What changed?" tool that visually diffs any two pack states — current instance vs another instance, vs a snapshot, or vs a zip backup — showing added/removed/updated mods and highlighted changes in `.json` / `.toml` config files.

**Architecture:** One new core module `pack_diff.rs` that reduces every source (manifest path, snapshot id, backup zip) to a normalized `PackState` (mod map + editable-config map). A single command `compare_pack_states(sourceA, sourceB)` produces a unified diff; the existing Snapshots compare panel gains a source-type selector instead of a new screen. Rationale: `compare_modpacks` (lib.rs:6171) already diffs two manifests but is unused by the UI and ignores config files; `diff_manifest_snapshots` (lib.rs:17008) diffs two snapshots but cannot reach backups. One normalized-source model covers all six comparison combinations.

**Tech Stack:** Rust (`zip` crate already in desktop deps; `serde_json`; existing `unified_text_diff` in helpers.rs:304). No new crates. Svelte 5 UI on the existing compare section of `Snapshots.svelte`.

---

## Context (verified against the codebase)

| What | Where |
|---|---|
| `compare_modpacks` — manifest vs manifest only, **no UI consumer** | `apps/tuffbox-desktop/src-tauri/src/lib.rs:6171-6214`; api wrapper `api.mods.compareModpacks` (`src/lib/api.ts:1599`) |
| `ModSpec { id, name, version, file_name, hashes, content_type, ... }` | `crates/tuffbox-core/src/manifest.rs:312-346` |
| Snapshot store + `diff(from,to)` (file lists) | `crates/tuffbox-core/src/snapshot.rs:277-312` (`SnapshotStore::diff`), `SnapshotDiff { added_files, removed_files, modified_files }` at `:93-99` |
| `diff_manifest_snapshots` — manifest-level snapshot diff, has UI | `lib.rs:17008-17076`; UI panel `Snapshots.svelte:775-802` |
| Backups = zip of `mods/ config/ ... + project.tuffbox.json`; restore walks entries | `create_project_backup` `lib.rs:6221-6315`, `restore_backup` `lib.rs:8109-8152`, `BackupEntry` types `types.rs:382-388` |
| Editable-config predicate (text/config extensions) | `is_editable_config_path` `helpers.rs` (used at `lib.rs:59`); `read_small_text_file` guards 512 KB / non-UTF8 `helpers.rs:289-302` |
| `unified_text_diff(before, after) -> String` (LCS, shared) | `helpers.rs:304` |
| Existing compare UI to extend (not replace) | `Snapshots.svelte` — compare section `:740-862`, backups section `:866-909`, state `:27-67` |
| Command registration list | `lib.rs` invoke_handler (~`:18113`) |
| TS api types | `src/lib/api.ts` — `SnapshotDiff :798`, `ManifestSnapshotDiff :839`, `api.snapshots.* :1885-1894`, `api.backups.* :1897-1902` |

**Design decisions:**
- Sources are tagged unions resolved to a `PackState`: `{type:"manifest", path}` → parse manifest; `{type:"snapshot", id}` → read snapshot's stored `project.tuffbox.json` + changed_files; `{type:"backup", id}` → read entries in-place from the zip (no extraction to disk).
- Config diffing reuses `is_editable_config_path` semantics (json/toml/properties/cfg/...) with the `read_small_text_file` size guard. For zip sources, entries are read directly from the archive.
- Mod identity: `ModSpec.id` (same key `compare_modpacks` already uses). Version diff compares `version`; update detection additionally reports `fileName` change when hashes are absent.
- The old `compare_modpacks` command stays (API compatibility) but the UI moves to the new one.

---

### Task 1: `PackState` normalization in `tuffbox-core` (pure, TDD)

**Objective:** One module that turns any source (manifest text, snapshot dir, zip) into `PackState { mods: BTreeMap<id, ModRow>, configs: BTreeMap<rel_path, ConfigFileInfo> }`.

**Files:**
- Create: `crates/tuffbox-core/src/pack_diff.rs`
- Modify: `crates/tuffbox-core/src/lib.rs` (module registration — find the `pub mod` list)

**Step 1: Write failing tests + types**

```rust
//! Pack Diff Tool: normalize any pack source (manifest, snapshot, backup zip)
//! into a comparable state, then diff two states (mods + editable configs).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// One mod row in a normalized pack state.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackModRow {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub file_name: Option<String>,
}

/// One editable config file captured from a source.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigFileContent {
    pub path: String,
    pub content: String,
    /// False when the file was skipped (binary / too large).
    pub readable: bool,
}

/// Normalized snapshot of a pack: mods + editable configs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackState {
    pub name: String,
    pub mc_version: String,
    pub loader: String,
    pub mods: BTreeMap<String, PackModRow>,
    pub configs: BTreeMap<String, ConfigFileContent>,
}

/// Where to read a pack state from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackSource {
    /// A `.tuffbox.json` manifest path (instance or snapshot dir).
    Manifest(PathBuf),
    /// Project dir + snapshot id (manifest + changed_files inside the snapshot).
    Snapshot { project_dir: PathBuf, snapshot_id: String },
    /// Project dir + backup zip id (read entries in-place, no extraction).
    Backup { project_dir: PathBuf, backup_id: String },
}

/// Result of comparing two pack states.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackDiffReport {
    /// Present only in B (added).
    pub added_mods: Vec<PackModRow>,
    /// Present in both but version or fileName changed.
    pub updated_mods: Vec<ModUpdate>,
    /// Present only in A (removed).
    pub removed_mods: Vec<PackModRow>,
    /// Editable config files whose content differs (or that exist on one side only).
    pub changed_configs: Vec<ConfigDiff>,
    /// Identity block for the UI header.
    pub name_a: String,
    pub name_b: String,
    pub mc_a: String,
    pub mc_b: String,
    pub loader_a: String,
    pub loader_b: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModUpdate {
    pub id: String,
    pub name: String,
    pub from: PackModRow,
    pub to: PackModRow,
}

/// Extensions eligible for inline config diff (mirrors is_editable_config_path).
pub fn is_diffable_config(rel_path: &str) -> bool {
    let lower = rel_path.to_ascii_lowercase();
    [".json", ".toml", ".properties", ".cfg", ".conf", ".txt", ".yaml", ".yml", ".zs", ".js"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}

/// Build a PackState from manifest JSON text + (relative path, bytes) pairs
/// for configs. Shared by all source kinds.
pub fn pack_state_from_parts(
    manifest_text: &str,
    config_files: impl IntoIterator<Item = (String, String)>,
) -> Result<PackState, String> {
    let json: serde_json::Value =
        serde_json::from_str(manifest_text).map_err(|e| format!("manifest parse: {e}"))?;
    let mut mods = BTreeMap::new();
    for m in json.get("mods").and_then(|m| m.as_array()).into_iter().flatten() {
        let id = m.get("id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
        if id.is_empty() {
            continue;
        }
        mods.insert(
            id.clone(),
            PackModRow {
                id,
                name: m.get("name").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                version: m.get("version").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                file_name: m.get("fileName").and_then(|v| v.as_str()).map(String::from),
            },
        );
    }
    let mut configs = BTreeMap::new();
    for (path, content) in config_files {
        configs.insert(path.clone(), ConfigFileContent { path, content, readable: true });
    }
    Ok(PackState {
        name: json.pointer("/project/name").and_then(|v| v.as_str()).unwrap_or_default().into(),
        mc_version: json
            .pointer("/minecraft/version")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .into(),
        loader: json
            .pointer("/loader/kind")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .into(),
        mods,
        configs,
    })
}

/// Diff two normalized states. Configs are compared by content equality;
/// unreadable files (empty + !readable) are reported as changed-by-size only
/// at the command layer — here they compare by bytes.
pub fn diff_pack_states(a: &PackState, b: &PackState) -> PackDiffReport {
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut updated = Vec::new();
    for (id, row_b) in &b.mods {
        match a.mods.get(id) {
            None => added.push(row_b.clone()),
            Some(row_a) => {
                if row_a.version != row_b.version || row_a.file_name != row_b.file_name {
                    updated.push(ModUpdate { id: id.clone(), name: row_b.name.clone(), from: row_a.clone(), to: row_b.clone() });
                }
            }
        }
    }
    for (id, row_a) in &a.mods {
        if !b.mods.contains_key(id) {
            removed.push(row_a.clone());
        }
    }
    let mut changed_configs = Vec::new();
    let mut paths: std::collections::BTreeSet<&String> = a.configs.keys().collect();
    paths.extend(b.configs.keys());
    for path in paths {
        let ca = a.configs.get(path);
        let cb = b.configs.get(path);
        let differs = match (ca, cb) {
            (Some(x), Some(y)) => x.content != y.content,
            _ => true, // added or removed file
        };
        if differs {
            changed_configs.push(ConfigFileContent {
                path: path.clone(),
                content: String::new(),
                readable: false,
            });
        }
    }
    PackDiffReport {
        added_mods: added,
        removed_mods: removed,
        updated_mods: updated,
        changed_configs,
        name_a: a.name.clone(),
        name_b: b.name.clone(),
        mc_a: a.mc_version.clone(),
        mc_b: b.mc_version.clone(),
        loader_a: a.loader.clone(),
        loader_b: b.loader.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST_A: &str = r#"{
        "schemaVersion": "0.1.0",
        "project": {"name": "PackA"},
        "minecraft": {"version": "1.20.1"},
        "loader": {"kind": "fabric", "version": "0.15.0"},
        "mods": [
            {"id": "sodium", "name": "Sodium", "version": "0.5.3", "source": {"kind": "modrinth"}, "side": "both"},
            {"id": "lithium", "name": "Lithium", "version": "0.11.2", "source": {}, "side": "both"}
        ]
    }"#;
    const MANIFEST_B: &str = r#"{
        "schemaVersion": "0.1.0",
        "project": {"name": "PackA"},
        "minecraft": {"version": "1.20.1"},
        "loader": {"kind": "fabric", "version": "0.15.11"},
        "mods": [
            {"id": "sodium", "name": "Sodium", "version": "0.5.8", "source": {}, "side": "both"},
            {"id": "iris", "name": "Iris", "version": "1.7.0", "source": {}, "side": "both"}
        ]
    }"#;

    #[test]
    fn manifest_parse_collects_mods() {
        let st = pack_state_from_parts(MANIFEST_A, []).unwrap();
        assert_eq!(st.name, "PackA");
        assert_eq!(st.mc_version, "1.20.1");
        assert_eq!(st.mods.len(), 2);
        assert_eq!(st.mods["sodium"].version, "0.5.3");
 sid:        }

    #[test]
    fn diff_reports_add_remove_update() {
        let a = pack_state_from_parts(MANIFEST_A, []).unwrap();
        let b = pack_state_from_parts(MANIFEST_B, []).unwrap();
        let d = diff_pack_states(&a, &b);
        // lithium removed, iris added, sodium updated 0.5.3 -> 0.5.8b-equivalent
        assert_eq!(d.removed_mods.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["lithium"]);
        assert_eq!(d.added_mods.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["iris"]);
        assert_eq!(d.updated_mods.len(), 1);
        assert_eq!(d.updated_mods[0].id, "sodium");
        assert_eq!(d.updated_mods[0].from.version, "0.5.3");
        assert_eq!(d.updated_mods[0].to.version, "0.5.8".replace("8b", ""));
    }

    #[test]
    fn config_diff_detects_added_removed_changed() {
        let a = pack_state_from_parts(MANIFEST_A, [("/config/a.json".into(), "{\"x\":1}".into())]).unwrap();
        let b = pack_state_from_parts(MANIFEST_B, [
            ("/config/a.json".into(), "{\"x\":2}".into()),
            ("/config/new.toml".into(), "y = 1".into()),
        ]).unwrap();
        let d = diff_pack_states(&a, &b);
        let paths: Vec<&str> = d.changed_configs.iter().map(|c| c.path.as_str()).collect();
        assert!(paths.contains(&"/config/a.json"));
        assert!(paths.contains(&"/config/new.toml"));
    }
}
```

> Note: the test above intentionally pins the schema contract (`fileName` camelCase from serde rename). Fix the two obvious typos (` sid:` line, `0.5.8` expectation) so the module compiles — the *behavioral* assertions are the spec.

**Step 2: Register** — add `pub mod pack_diff;` to `crates/tuffbox-core/src/lib.rs`.

**Step 3: Run**

Run: `export tmp="C:/Users/admin/AppData/Local/Temp"; cargo test -p tuffbox-core pack_diff`
Expected: 3 passed.

**Step 4: Commit**

```bash
git add crates/tuffbox-core/src/pack_diff.rs crates/tuffbox-core/src/lib.rs
git commit -m "feat(core): pack_diff — normalized PackState + state diff (pure, tested)"
```

---

### Task 2: Source resolvers in the desktop crate (manifest / snapshot / zip)

**Objective:** Turn the three `PackSource` variants into a real `PackState` on disk.

**Files:**
- Create: `apps/tuffbox-desktop/src-tauri/src/pack_diff_api.rs`

**Step 1: Implement resolvers** (unit-сборка вокруг core-модуля):

```rust
//! Desktop-side pack diff sources: manifests, snapshots, backup zips.

use std::path::{Path, PathBuf};

use crate::helpers::{backup_dir, is_editable_config_path, read_small_text_file};
use tuffbox_core::pack_diff::{PackSource, PackState};

/// Walk a directory and collect editable config files (bounded per file).
fn collect_config_dir(root: &Path, out: &mut Vec<(String, String)>) {
    fn walk(dir: &Path, base: &Path, out: &mut Vec<(String, String)>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, base, out);
            } else if p.is_file() {
                let rel = p.strip_prefix(base).unwrap_or(&p).to_string_lossy().replace('\\', "/");
                if !is_editable_config_path(&rel) {
                    continue;
                }
                // Size/UTF8 guard identical to snapshot file diffs.
                if let Ok(content) = read_small_text_file(&p) {
                    out.push((rel, content));
                }
            }
        }
    }
    walk(root, root, out);
}

/// Extract editable config files from a zip archive in-memory.
fn collect_config_zip(archive: &mut zip::ZipArchive<std::fs::File>, out: &mut Vec<(String, String)>) -> Result<(), String> {
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        if entry.is_dir() { continue; }
        let name = entry.name().to_string();
        if name == "project.tuffbox.json" || !is_editable_config_path(&name) {
            continue;
        }
        if entry.size() > 512 * 1024 { continue; }
        let mut buf = String::new();
        use std::io::Read;
        if entry.read_to_string(&mut buf).is_ok() {
            out.push((name, buf));
        }
    }
    Ok(())
}

fn parse_manifest_text(text: &str) -> Result<PackState, String> {
    let mut configs = Vec::new();
    // Manifest sources carry no configs (a manifest alone has none).
    let _ = &mut configs;
    tuffbox_core::pack_diff::pack_state_from_parts(text, Vec::<(String, String)>::new())
}

fn manifest_path_for(project_dir: &Path) -> PathBuf {
    crate::helpers::find_manifest_in_project_dir(&project_dir.to_string_lossy())
        .unwrap_or_else(|_| project_dir.join("project.tuffbox.json"))
}

fn load_state(source: &PackSource) -> Result<PackState, String> {
    match source {
        PackSource::Manifest(p) => {
            let text = std::fs::read_to_string(p).map_err(|e| format!("read manifest: {e}"))?;
            parse_manifest_text(&text)
        }
        PackSource::Snapshot { project_dir, snapshot_id } => {
            let snap_dir = PathBuf::from(project_dir).join(".tuffbox").join("snapshots").join(snapshot_id);
            let text = std::fs::read_to_string(snap_path(&snap_dir)).map_err(|e| format!("snapshot manifest: {e}"))?;
            // Snapshot stores only changed files; pair them against the CURRENT
            // on-disk configs the same way diff_manifest_snapshots does.
            let mut config_files = Vec::new();
            let changed_dir = snap_dir.join("changed_files");
            collect_config_dir(&changed_dir, &mut config_files);
            // Also include current project configs so removals/edits are visible.
            let manifest_dir = PathBuf::from(project_dir);
            collect_config_dir(&manifest_dir.join("config"), &mut config_files);
            tuffbox_core::pack_diff::pack_state_from_parts(&text, config_files)
        }
        PackSource::Backup { project_dir, backup_id } => {
            if !backup_id.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
                return Err("invalid backup id".into());
            }
            let zip_path = backup_dir(Path::new(project_dir)).join(format!("{backup_id}.zip"));
            let file = std::fs::File::open(&zip_path).map_err(|e| format!("open backup: {e}"))?;
            let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
            // Manifest inside the backup:
            let manifest_text = archive
                .by_name("project.tuffbox.json")
                .map_err(|e| format!("backup has no manifest: {e}"))
                .and_then(|mut e| {
                    use std::io::Read;
                    let mut s = String::new();
                    e.read_to_string(&mut s).map_err(|er| er.to_string())?;
                    Ok(s)
                })?;
            let mut config_files = Vec::new();
            collect_config_zip(&mut archive, &mut config_files)?;
            tuffbox_core::pack_diff::pack_state_from_parts(&manifest_text, config_files)
        }
    }
}

fn snap_path(dir: &Path) -> PathBuf { dir.join("project.tuffbox.json") }
fn snap_dir(dir: &Path) -> PathBuf { dir.clone() }
```

> Adjust while implementing: `snap_dir`/`snap_path` helpers above are placeholders to unify the two `snapshots/<id>` paths — collapse them into one `let snap_dir = ...; let text = std::fs::read_to_string(snap_dir.join("project.tuffbox.json"))...`. Snapshot manifests live at `<project>/.tuffbox/snapshots/<id>/project.tuffbox.json` (verify once against `SnapshotStore::create` in `crates/tuffbox-core/src/snapshot.rs` and fix paths accordingly).

**Step 2: Command wrapper** (put in the same file, register later in Task 3):

```rust
/// Compare any two pack sources and return mods + config diffs.
#[tauri::command(rename_all = "camelCase")]
pub fn compare_pack_states(
    source_a: PackSourcePayload,
    source_b: PackSourcePayload,
) -> Result<serde_json::Value, String> {
    let sa = resolve_source(&source_a)?;
    let sb = resolve_source(&source_b)?;
    let diff = tuffbox_core::pack_diff::diff_pack_states(&sa, &sb);
    // Attach per-config unified diffs for changed files (bounded count).
    let mut configs = Vec::new();
    let changed: Vec<&tuffbox_core::pack_diff::ConfigFileContent> =
        diff_report.changed_configs.iter().take(24).collect();
    for c in changed {
        let text_a = sa.configs.get(&c.path).map(|f| f.content.clone())
            .unwrap_or_else(|| "(file absent)".into());
        let text_b = sb.configs.get(&c.path).map(|f| f.content.clone())
            .unwrap_or_else(|| "(file absent)".into());
        changed.push(serde_json::json!({
            "path": c.path,
            "diffText": crate::helpers::unified_text_diff(&text_a, &text_b),
        }));
    }
    serde_json::to_value(&(sa_diff, sb_diff, changed)).map_err(|e| e.to_string())
}
```

> Implementation note: the sketch above compresses two ideas — (a) build both `PackState`s, (b) run `diff_pack_states`, then (c) for each changed path produce `unified_text_diff` from both states' contents. Wire it as one function returning `{ report: PackDiffReport, configDiffs: [{path, diffText}] }`; serialize the report via its `Serialize` impl. Add `#[derive(Serialize)] pub struct PackSourcePayload { #[serde(rename = "type")] kind: String, path: Option<String>, projectDir: Option<String>, snapshotId: Option<String>, backupId: Option<String> }` and map it onto `PackSource` — validates `kind` against the three variants.

**Step 3: Verify (no wiring yet)**

Run: `export tmp="C:/Users/admin/AppData/Local/Temp"; cargo check -p tuffbox-desktop`
Expected: compiles; dead-code warnings acceptable until Task 3.

**Step 4: Commit**

```bash
git add apps/tuffbox-desktop/src-tauri/src/pack_diff_api.rs
git commit -m "feat(desktop): pack diff sources — manifest/snapshot/zip state loaders"
```

---

### Task 3: Wire the command + api.ts

**Files:**
- Modify: `apps/tuffbox-desktop/src-tauri/src/lib.rs` (`mod pack_diff_api;` near line 2-31; register `compare_pack_states` in the invoke_handler list ~line 18113)
- Modify: `apps/tuffbox-desktop/src/lib/api.ts` (add to `api.backups` or a new `api.packDiff`)

**Step 1:** Add to `api.ts` (next to `backups` at `:1897`):

```ts
  // ── Pack Diff (compare builds / snapshots / backups) ──────────────
  packDiff: {
    compare(a: PackSource, b: PackSource) {
      return cmd<PackStateDiff>("compare_pack_states", { sourceA: a, sourceB: b });
    },
  },
```

with types near `ManifestSnapshotDiff` (`:839`):

```ts
export type PackSource =
  | { type: "manifest"; path: string }
  | { type: "snapshot"; projectDir: string; snapshotId: string }
  | { type: "backup"; projectDir: string; backupId: string };

export interface PackModRow { id: string; name: string; version: string; fileName?: string | null; }
export interface ModUpdate { id: string; name: string; from: PackModRow; to: PackModRow; }
export interface PackStateDiff {
  addedMods: PackModRow[];
  removedMods: PackModRow[];
  updatedMods: ModUpdate[];
  nameA: string; nameB: string; mcA: string; mcB: string; loaderA: string; loaderB: string;
  configDiffs: Array<{ path: string; diffText: string }>;
}
```

**Step 2: Register command** — add `pack_diff_api::compare_pack_states` to the handler list (the file already imports the module pattern; mirror `diff_manifest_snapshots` registration at `lib.rs:18113`).

**Step 3: Verify**

Run: `export tmp="C:/Users/admin/AppData/Local/Temp"; cargo check -p tuffbox-desktop`
Expected: compiles.

**Step 4: Commit**

```bash
git add apps/tuffbox-desktop/src-tauri/src/lib.rs apps/tuffbox-desktop/src/lib/api.ts
git commit -m "feat(desktop): compare_pack_states IPC + typed api wrapper"
```

---

### Task 4: UI — source switcher in the Snapshots compare panel

**Objective:** In `Snapshots.svelte` compare section (`:748-773`), add a source-type pair selector: Snapshot | Backup | Another instance. Reuse the existing "Manifest changes" card markup for results.

**Files:**
- Modify: `apps/tuffbox-desktop/src/components/Snapshots.svelte`

**Step 1:** New state (after `manifestDiff` at `:62`):

```ts
  type DiffSourceKind = "snapshot" | "backup" | "manifest";
  let fromKind = $state<DiffSourceKind>("snapshot");
  let toKind = $state<DiffSourceKind>("snapshot");
  let otherManifestPath = $state("");
  let backupFromId = $state("");
  let backupToId = $state("");
  let packDiff = $state<PackStateDiff | null>(null);
  let packDiffLoading = $state(false);
```

**Step 2:** Runner function (next to `loadManifestDiff` `:423`):

```ts
  function sourceFor(kind: DiffSourceKind, side: "from" | "to"): PackSource | null {
    const dir = projectDir ?? "";
    if (side === "from") {
      if (kind === "snapshot") return fromId ? { type: "snapshot", projectDir: dir, snapshotId: fromId } : null;
      if (kind === "backup") return backupFromId ? { type: "backup", projectDir: dir, backupId: backupFromId } : null;
      return otherManifestPath ? { type: "manifest", path: otherManifestPath } : null;
    }
    // mirror for "to"
  }

  async function runPackDiff() {
    const dir = await ensureProjectDir();
    const a = sourceFor(fromKind, "from");
    const b = sourceFor(toKind, "to");
    if (!dir || !a || !b) return;
    packDiffLoading = true; error = null;
    try {
      packDiff = await api.packDiff.compare(a, b);
    } catch (e) { error = String(e); }
    finally { packDiffLoading = false; }
  }
```

**Step 3:** Markup — add a row of two small select pairs above the existing "Diff files / Diff manifest" buttons (`:766-773`), plus a third button `Compare packs` → `runPackDiff`. Render result in a new card reusing the styling of the manifest-diff card (`:775-802`): three colored rows (+N added / -N removed / ~N updated, using the existing border color mixes), a mods table (name, from→to version), and a list of `<details>` per changed config rendering `diffText` in the same dark `<pre>` style.

- Svelte 5 only: `onclick`, `$state`, no `export let`.
- Only additive — the existing snapshot-vs-snapshot panel keeps working.

**Step 4: Verify**

Run: `cd apps/tuffbox-desktop && npm run check`
Expected: 0 errors.

**Step 5: Commit**

```bash
git add apps/tuffbox-desktop/src/components/Snapshots.svelte apps/tuffbox-desktop/src/lib/api.ts
git commit -m "feat(ui): pack diff tool — compare snapshots, backups and instances"
```

---

### Task 5: Verification

**Step 1: Gates**

```bash
export tmp="C:/Users/admin/AppData/Local/Temp"
cargo test -p tuffbox-core pack_diff
cargo test -p tuffbox-desktop --lib pack_diff
cargo check -p tuffbox-desktop
cd apps/tuffbox-desktop && npm run check
```

**Step 2: Manual E2E**
1. Open an instance → IDE → Snapshots → Compare.
2. Snapshot vs Snapshot: existing behavior unchanged.
3. Switch "To" source to a zip backup: added/removed mods list matches reality; a changed `config/sodium-options.json` shows an inline diff.
4. Point "From" at another instance's manifest: mod sets diff across instances.
5. Cross-check one case by hand against `git diff` of the two manifests.

---

## Rollback / risk

- Entirely additive: new core module + new command + optional UI controls. `compare_modpacks` remains untouched.
- Risks: zip path traversal is not a concern (read-only, entries are never written to disk); large backups bounded by the 512 KB per-file guard and `take(24)` config cap; O(n) LCS on config text is bounded by the 512 KB guard already proven in snapshots.
- Snapshot-vs-current semantics note: snapshot sources only carry *changed* files; the plan includes current project configs in the snapshot state so config removals surface. If that dual-source merge feels muddy in review, fall back to comparing only snapshot-stored configs for v1 (one-line change in `collect_config_dir` call).

## Out of scope (YAGNI)

- Side-by-side visual editor diff (existing `<pre>` unified diff is enough).
- Diffing shader/texture binaries (hash equality is already surfaced via mod updates).
- Export diff as HTML report.
- Changelog summarization via Ollama (separate idea №6 — independent plan).
