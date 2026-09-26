//! Launch the original Querz MCA Selector against a project world.
//!
//! The upstream JAR treats any CLI args without `--mode` as a fatal CLI error
//! (`missing mode` → exit). GUI launch therefore uses no `--world` flag; instead
//! we seed `%LOCALAPPDATA%/mcaselector/settings.json` so the world appears under
//! File → Open Recent and Open World starts in the pack's `saves/` folder.
//!
//! The JAR + OpenJFX libs are bundled with the launcher (`mca-selector/` resources).
//! No network download is required at runtime. When the host JRE lacks JavaFX
//! (e.g. GraalVM), we pass `--module-path` to the bundled OpenJFX `lib` folder.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};

const MCA_VERSION: &str = "2.8";
const MCA_JAR_NAME: &str = "mcaselector-2.8.jar";
const MIN_JAVA_MAJOR: u32 = 21;
/// Minimum plausible size for the bundled MCA Selector JAR (rejects stubs).
const MIN_JAR_BYTES: u64 = 1_000_000;

/// Cached JAR path so repeated opens skip filesystem root scanning.
static CACHED_JAR: OnceLock<PathBuf> = OnceLock::new();
/// Cached Java binary after first successful resolve (skips `find_all_runtimes` / `C:\` scan).
static CACHED_JAVA: Mutex<Option<String>> = Mutex::new(None);

/// Tauri entry: resolve bundled JAR + JavaFX, patch settings, spawn the GUI.
///
/// Heavy work (Java scan, settings I/O, process spawn) runs on a blocking pool so
/// the UI/main thread stays responsive. We only wait until the child is spawned,
/// not until MCA Selector exits.
#[tauri::command]
pub async fn open_mca_selector(path: String, world_name: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || open_mca_selector_blocking(path, world_name))
        .await
        .map_err(|e| format!("MCA Selector launch task failed: {e}"))?
}

fn open_mca_selector_blocking(path: String, world_name: String) -> Result<(), String> {
    let project_dir = crate::manifest_parent(&path)?;
    let saves_dir = project_dir.join("saves");
    let world_dir = saves_dir.join(&world_name);

    if !world_dir.is_dir() {
        return Err(format!(
            "world folder not found: {}",
            world_dir.display()
        ));
    }

    let jar = resolve_mca_jar()?;
    let java = resolve_java_for_mca()?;
    let javafx_lib = resolve_javafx_lib(&java)?;
    seed_mca_settings(&saves_dir, &world_dir)?;

    // Prefer javaw.exe on Windows so the console subsystem of java.exe never flashes.
    let launch_bin = gui_java_binary(&java);

    let mut cmd = Command::new(&launch_bin);
    cmd.arg("-Xmx4G");
    if let Some(ref fx) = javafx_lib {
        cmd.arg("--module-path")
            .arg(fx)
            .arg("--add-modules")
            .arg("ALL-MODULE-PATH");
    }
    cmd.arg("-jar")
        .arg(&jar)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    // Independent GUI child: do NOT use DETACHED_PROCESS — that flag can make the
    // parent briefly unresponsive and reset the Windows taskbar icon to the default.
    // CREATE_BREAKAWAY_FROM_JOB keeps the JavaFX window alive if TuffBox is in a job.
    // CREATE_NO_WINDOW hides the java.exe console if javaw is unavailable.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(
            CREATE_BREAKAWAY_FROM_JOB | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW,
        );
    }

    cmd.spawn()
        .map_err(|e| format!("failed to launch MCA Selector ({launch_bin}): {e}"))?;

    Ok(())
}

fn tools_cache_dir() -> Result<PathBuf, String> {
    let root = if let Ok(local) = std::env::var("LOCALAPPDATA") {
        PathBuf::from(local).join("TuffBox").join("tools").join("mca-selector")
    } else if let Some(cache) = dirs::cache_dir() {
        cache.join("TuffBox").join("tools").join("mca-selector")
    } else {
        std::env::temp_dir()
            .join("tuffbox")
            .join("tools")
            .join("mca-selector")
    };
    fs::create_dir_all(&root).map_err(|e| format!("create MCA cache dir: {e}"))?;
    Ok(root)
}

/// Candidate roots that may contain `mcaselector-2.8.jar` and/or `javafx-lib/`.
fn bundled_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Ok(dir) = std::env::var("TUFFBOX_MCA_SELECTOR_DIR") {
        let p = PathBuf::from(dir);
        if p.is_dir() {
            roots.push(p);
        }
    }

    // Packaged / tauri-dev resources: …/mca-selector/
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            for rel in [
                PathBuf::from("mca-selector"),
                PathBuf::from("resources").join("mca-selector"),
                PathBuf::from("../Resources/mca-selector"),
            ] {
                let p = exe_dir.join(rel);
                if p.is_dir() {
                    roots.push(p);
                }
            }
        }
    }

    // Dev checkout: bridges/mca-selector/prebuilt next to the repo.
    for ancestor in [
        std::env::current_dir().ok(),
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf())),
    ]
    .into_iter()
    .flatten()
    {
        let mut dir = ancestor;
        for _ in 0..8 {
            let candidate = dir.join("bridges").join("mca-selector").join("prebuilt");
            if candidate.is_dir() {
                roots.push(candidate);
                break;
            }
            if !dir.pop() {
                break;
            }
        }
    }

    if let Ok(cache) = tools_cache_dir() {
        roots.push(cache);
    }

    roots
}

fn jar_size_ok(path: &Path) -> bool {
    path.is_file()
        && path
            .metadata()
            .map(|m| m.len() > MIN_JAR_BYTES)
            .unwrap_or(false)
}

/// Resolve the bundled JAR with a cheap size gate (no full-file SHA on the open path).
fn resolve_mca_jar() -> Result<PathBuf, String> {
    if let Some(cached) = CACHED_JAR.get() {
        if jar_size_ok(cached) {
            return Ok(cached.clone());
        }
    }

    for root in bundled_roots() {
        let jar = root.join(MCA_JAR_NAME);
        if jar_size_ok(&jar) {
            let _ = CACHED_JAR.set(jar.clone());
            return Ok(jar);
        }
    }

    Err(format!(
        "Bundled MCA Selector {MCA_VERSION} not found. Expected `{MCA_JAR_NAME}` under \
         TUFFBOX_MCA_SELECTOR_DIR, app resources/mca-selector, or bridges/mca-selector/prebuilt. \
         Run bridges/mca-selector/fetch-prebuilt.ps1 to populate the prebuilt folder."
    ))
}

fn javafx_lib_looks_valid(dir: &Path) -> bool {
    if !dir.is_dir() {
        return false;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|e| {
        let name = e.file_name().to_string_lossy().to_lowercase();
        name.contains("javafx.base") || name.contains("javafx-base")
    })
}

fn find_bundled_javafx_lib() -> Option<PathBuf> {
    for root in bundled_roots() {
        for rel in ["javafx-lib", "javafx/lib", "lib"] {
            let candidate = root.join(rel);
            if javafx_lib_looks_valid(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

/// Returns `Some(module_path)` when we must inject OpenJFX; `None` if Java already has it.
fn resolve_javafx_lib(java_path: &str) -> Result<Option<PathBuf>, String> {
    // Disk probe only — avoid spawning `java --list-modules` on the open path.
    if java_has_javafx_on_disk(java_path) {
        return Ok(None);
    }

    if let Some(fx) = find_bundled_javafx_lib() {
        return Ok(Some(fx));
    }

    Err(format!(
        "Java at '{java_path}' has no JavaFX modules, and no bundled OpenJFX was found. \
         Populate bridges/mca-selector/prebuilt/javafx-lib (see fetch-prebuilt.ps1) or install \
         a JRE with JavaFX (Azul Zulu JRE-FX)."
    ))
}

fn java_bin_ok(path: &str) -> bool {
    let p = Path::new(path);
    p.is_file()
}

/// On Windows, swap `java.exe` → sibling `javaw.exe` when present (no console window).
fn gui_java_binary(java_path: &str) -> String {
    #[cfg(windows)]
    {
        let path = Path::new(java_path);
        let file = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if file.eq_ignore_ascii_case("java.exe") || file.eq_ignore_ascii_case("java") {
            let javaw = path.with_file_name("javaw.exe");
            if javaw.is_file() {
                return javaw.to_string_lossy().into_owned();
            }
        }
        return java_path.to_string();
    }
    #[cfg(not(windows))]
    {
        java_path.to_string()
    }
}

fn cache_java_path(path: String) -> String {
    if let Ok(mut guard) = CACHED_JAVA.lock() {
        *guard = Some(path.clone());
    }
    path
}

fn resolve_java_for_mca() -> Result<String, String> {
    // Fast path: reuse last successful Java 21+ binary when it still exists.
    if let Ok(mut guard) = CACHED_JAVA.lock() {
        if let Some(cached) = guard.as_ref() {
            if java_bin_ok(cached) {
                return Ok(cached.clone());
            }
        }
        // Stale cache (binary moved/uninstalled) — force a rescan.
        *guard = None;
    }

    // Prefer any already-installed Java 21+ (with or without JavaFX —
    // bundled OpenJFX covers the latter). If nothing is on the machine,
    // download the latest GraalVM Community JDK into the managed folder.
    let runtimes = tuffbox_core::jre::find_all_runtimes()
        .map_err(|e| format!("scan Java runtimes: {e}"))?;
    let candidates: Vec<_> = runtimes
        .into_iter()
        .filter(|r| r.major >= MIN_JAVA_MAJOR)
        .collect();

    let bundled_fx = find_bundled_javafx_lib().is_some();

    // With bundled OpenJFX, any Java 21+ works — skip per-runtime `--list-modules`.
    if bundled_fx {
        if let Some(rt) = candidates
            .iter()
            .find(|rt| java_has_javafx_on_disk(&rt.path))
            .or_else(|| candidates.first())
        {
            return Ok(cache_java_path(rt.path.clone()));
        }
    } else {
        for rt in &candidates {
            if java_has_javafx(&rt.path) {
                return Ok(cache_java_path(rt.path.clone()));
            }
        }
        if let Some(rt) = candidates.first() {
            return Ok(cache_java_path(rt.path.clone()));
        }
    }

    let installed = tuffbox_core::jre::ensure_java().map_err(|e| e.to_string())?;
    if installed.major < MIN_JAVA_MAJOR {
        return Err(format!(
            "MCA Selector requires Java {MIN_JAVA_MAJOR}+, but managed runtime is Java {}",
            installed.major
        ));
    }
    Ok(cache_java_path(installed.path))
}

fn java_home_from_bin(java_path: &str) -> Option<PathBuf> {
    let bin = Path::new(java_path);
    let parent = bin.parent()?;
    // …/bin/java(.exe) → JAVA_HOME; also accept a bare java path.
    if parent
        .file_name()
        .map(|n| n.eq_ignore_ascii_case("bin"))
        .unwrap_or(false)
    {
        parent.parent().map(|p| p.to_path_buf())
    } else {
        Some(parent.to_path_buf())
    }
}

/// Fast path: detect JavaFX on disk next to the JRE (no process spawn).
fn java_has_javafx_on_disk(java_path: &str) -> bool {
    let Some(home) = java_home_from_bin(java_path) else {
        return false;
    };
    let jmod = home.join("jmods").join("javafx.base.jmod");
    if jmod.is_file() {
        return true;
    }
    for rel in ["lib", "jmods"] {
        let dir = home.join(rel);
        if javafx_lib_looks_valid(&dir) {
            return true;
        }
    }
    false
}

fn java_has_javafx(java_path: &str) -> bool {
    if java_has_javafx_on_disk(java_path) {
        return true;
    }

    let mut c = Command::new(java_path);
    c.arg("--list-modules")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let Ok(output) = c.output() else {
        return false;
    };
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    text.lines().any(|line| {
        let trimmed = line.trim();
        trimmed.starts_with("javafx.") || trimmed.contains("@javafx.")
    })
}

fn mca_settings_path() -> Result<PathBuf, String> {
    #[cfg(target_os = "windows")]
    {
        let local = std::env::var("LOCALAPPDATA").map_err(|_| {
            "LOCALAPPDATA is not set; cannot locate MCA Selector settings".to_string()
        })?;
        let dir = PathBuf::from(local).join("mcaselector");
        fs::create_dir_all(&dir).map_err(|e| format!("create mcaselector settings dir: {e}"))?;
        return Ok(dir.join("settings.json"));
    }
    #[cfg(target_os = "macos")]
    {
        let home = dirs::home_dir().ok_or_else(|| "home directory not found".to_string())?;
        let dir = home.join("Library/Application Support/mcaselector");
        fs::create_dir_all(&dir).map_err(|e| format!("create mcaselector settings dir: {e}"))?;
        return Ok(dir.join("settings.json"));
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let dir = if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            PathBuf::from(xdg).join("mcaselector")
        } else {
            let home = dirs::home_dir().ok_or_else(|| "home directory not found".to_string())?;
            home.join(".mcaselector").join("mcaselector")
        };
        fs::create_dir_all(&dir).map_err(|e| format!("create mcaselector settings dir: {e}"))?;
        Ok(dir.join("settings.json"))
    }
}

fn detect_dimension_dirs(world_dir: &Path) -> Vec<PathBuf> {
    let mut dims = Vec::new();
    // Overworld is the world root when `region/` exists (or always as primary).
    if world_dir.join("region").is_dir() || world_dir.join("level.dat").is_file() {
        dims.push(world_dir.to_path_buf());
    }
    for name in ["DIM-1", "DIM1"] {
        let p = world_dir.join(name);
        if p.is_dir() {
            dims.push(p);
        }
    }
    // Modded dimensions folder: dimensions/<ns>/<path>
    let dimensions_root = world_dir.join("dimensions");
    if dimensions_root.is_dir() {
        if let Ok(namespaces) = fs::read_dir(&dimensions_root) {
            for ns in namespaces.flatten() {
                if !ns.path().is_dir() {
                    continue;
                }
                if let Ok(children) = fs::read_dir(ns.path()) {
                    for child in children.flatten() {
                        let p = child.path();
                        if p.is_dir() && (p.join("region").is_dir() || p.join("data").is_dir()) {
                            dims.push(p);
                        }
                    }
                }
            }
        }
    }
    if dims.is_empty() {
        dims.push(world_dir.to_path_buf());
    }
    dims
}

fn seed_mca_settings(saves_dir: &Path, world_dir: &Path) -> Result<(), String> {
    let settings_path = mca_settings_path()?;
    let mut root: serde_json::Value = if settings_path.is_file() {
        let raw = fs::read_to_string(&settings_path)
            .map_err(|e| format!("read MCA settings: {e}"))?;
        serde_json::from_str(&raw).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    if !root.is_object() {
        root = serde_json::json!({});
    }

    let obj = root
        .as_object_mut()
        .ok_or_else(|| "MCA settings JSON root must be an object".to_string())?;

    let saves_str = saves_dir
        .canonicalize()
        .unwrap_or_else(|_| saves_dir.to_path_buf())
        .to_string_lossy()
        .to_string();
    let world_str = world_dir
        .canonicalize()
        .unwrap_or_else(|_| world_dir.to_path_buf())
        .to_string_lossy()
        .to_string();

    obj.insert("mcSavesDir".into(), serde_json::Value::String(saves_str));

    let dim_dirs: Vec<serde_json::Value> = detect_dimension_dirs(world_dir)
        .into_iter()
        .map(|p| {
            serde_json::Value::String(
                p.canonicalize()
                    .unwrap_or(p)
                    .to_string_lossy()
                    .to_string(),
            )
        })
        .collect();

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
        .to_string();

    let entry = serde_json::json!({
        "recentWorld": world_str,
        "dimensionDirectories": dim_dirs,
    });

    let recent = obj
        .entry("recentWorlds")
        .or_insert_with(|| serde_json::json!({}));
    if let Some(map) = recent.as_object_mut() {
        // Drop any prior entry pointing at the same world (by path).
        map.retain(|_, v| {
            v.get("recentWorld")
                .and_then(|x| x.as_str())
                .map(|p| !paths_equal(Path::new(p), world_dir))
                .unwrap_or(true)
        });
        // Cap at 16 like MCA Selector.
        while map.len() >= 16 {
            if let Some(oldest) = map.keys().min().cloned() {
                map.remove(&oldest);
            } else {
                break;
            }
        }
        map.insert(now_ms, entry);
    }

    if let Some(parent) = settings_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create settings parent: {e}"))?;
    }
    let pretty = serde_json::to_string_pretty(&root)
        .map_err(|e| format!("serialize MCA settings: {e}"))?;
    let tmp = settings_path.with_extension("json.tmp");
    {
        let mut f = fs::File::create(&tmp).map_err(|e| format!("write MCA settings tmp: {e}"))?;
        f.write_all(pretty.as_bytes())
            .map_err(|e| format!("write MCA settings: {e}"))?;
        f.sync_all().ok();
    }
    fs::rename(&tmp, &settings_path).or_else(|_| {
        fs::copy(&tmp, &settings_path)
            .map(|_| {
                let _ = fs::remove_file(&tmp);
            })
            .map_err(|e| format!("commit MCA settings: {e}"))
    })?;

    Ok(())
}

fn paths_equal(a: &Path, b: &Path) -> bool {
    let ca = fs::canonicalize(a).unwrap_or_else(|_| a.to_path_buf());
    let cb = fs::canonicalize(b).unwrap_or_else(|_| b.to_path_buf());
    ca == cb
}
