use std::{
    collections::{HashMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Mutex,
    time::UNIX_EPOCH,
};
use thiserror::Error;

/// How far to search the disk for Java installs.
///
/// `Fast` skips listing the Windows drive root (`C:\`), which hitchs the
/// launcher when Settings asks for a default version. The Java picker uses
/// `Full` so portable `C:\graalvm*` / `C:\jdk*` installs still appear.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JavaScanDepth {
    Fast,
    Full,
}

static RUNTIME_CACHE: Mutex<Option<(JavaScanDepth, Vec<JavaRuntime>)>> = Mutex::new(None);

/// `java -version` is 200–800ms on Windows. Cache by binary size+mtime so
/// repeated Play clicks do not spawn a JVM just to read the version string.
struct JavaPathCacheEntry {
    len: u64,
    mtime_secs: u64,
    runtime: JavaRuntime,
}

static JAVA_PATH_CACHE: Mutex<Option<HashMap<PathBuf, JavaPathCacheEntry>>> = Mutex::new(None);

#[derive(Debug, Error)]
pub enum JreError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse java version")]
    InvalidVersion,
    #[error("java not found")]
    NotFound,
    #[error("failed to download GraalVM: {0}")]
    Download(String),
    #[error("failed to install GraalVM: {0}")]
    Install(String),
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct JavaRuntime {
    pub path: String,
    pub version: String,
    pub major: u32,
}

/// Returns the minimum Java major version Mojang requires for a given
/// Minecraft release, per the version manifest's `javaVersion` field
/// (hardcoded here for the common release boundaries since we don't fetch
/// per-version manifests just for this check).
///
/// This exists because [`find_all_runtimes`] callers previously picked
/// "the newest installed Java" unconditionally, with no regard for what
/// the target Minecraft/loader version actually needs. That silently
/// launches e.g. Forge 1.20.1 (which needs Java 17) on a Java 21 JVM,
/// which fails deep inside Forge's bootstrap launcher with a confusing
/// `InaccessibleObjectException` instead of a clear "wrong Java version"
/// message.
pub fn required_java_major(mc_version: &str) -> u32 {
    let parts: Vec<u32> = mc_version
        .split('.')
        .filter_map(|p| p.split('-').next().and_then(|p| p.parse().ok()))
        .collect();
    let minor = parts.get(1).copied().unwrap_or(0);
    let patch = parts.get(2).copied().unwrap_or(0);

    if minor >= 21 || (minor == 20 && patch >= 5) {
        21
    } else if minor >= 18 {
        17
    } else if minor == 17 {
        16
    } else {
        8
    }
}

/// Picks the best installed runtime for a required Java major version:
/// an exact match if available, otherwise the closest newer version (JVMs
/// are usually backward-compatible enough for vanilla/Fabric, though not
/// always for Forge installers), and only falls back to "newest available"
/// if nothing meets the requirement.
pub fn find_runtime_for(runtimes: &[JavaRuntime], required_major: u32) -> Option<JavaRuntime> {
    runtimes
        .iter()
        .find(|r| r.major == required_major)
        .or_else(|| {
            runtimes
                .iter()
                .filter(|r| r.major > required_major)
                .min_by_key(|r| r.major)
        })
        .or_else(|| runtimes.iter().max_by_key(|r| r.major))
        .cloned()
}

/// Directory where TuffBox stores a managed GraalVM install.
pub fn managed_java_root() -> PathBuf {
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        return PathBuf::from(local)
            .join("TuffBox")
            .join("runtime")
            .join("java");
    }
    if let Some(data) = dirs::data_local_dir() {
        return data.join("TuffBox").join("runtime").join("java");
    }
    std::env::temp_dir()
        .join("tuffbox")
        .join("runtime")
        .join("java")
}

pub fn invalidate_runtime_cache() {
    if let Ok(mut guard) = RUNTIME_CACHE.lock() {
        *guard = None;
    }
    if let Ok(mut guard) = JAVA_PATH_CACHE.lock() {
        *guard = None;
    }
}

/// Cached discovery. Settings / auto-detect use Fast; the Java picker uses Full.
pub fn find_all_runtimes() -> Result<Vec<JavaRuntime>, JreError> {
    cached_runtimes(JavaScanDepth::Fast)
}

pub fn find_all_runtimes_full() -> Result<Vec<JavaRuntime>, JreError> {
    cached_runtimes(JavaScanDepth::Full)
}

fn cached_runtimes(depth: JavaScanDepth) -> Result<Vec<JavaRuntime>, JreError> {
    if let Ok(guard) = RUNTIME_CACHE.lock() {
        if let Some((cached_depth, runtimes)) = guard.as_ref() {
            if *cached_depth == JavaScanDepth::Full || *cached_depth == depth {
                return Ok(runtimes.clone());
            }
        }
    }
    let runtimes = discover_runtimes(depth)?;
    if let Ok(mut guard) = RUNTIME_CACHE.lock() {
        *guard = Some((depth, runtimes.clone()));
    }
    Ok(runtimes)
}

/// Directories we `read_dir` while looking for Java `bin` folders.
/// Exported for tests so Fast never includes the Windows drive root.
pub fn java_search_roots(depth: JavaScanDepth) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let managed = managed_java_root();
    if managed.is_dir() {
        roots.push(managed);
    }
    #[cfg(target_os = "windows")]
    {
        roots.extend(
            [
                r"C:\Program Files\Java",
                r"C:\Program Files (x86)\Java",
                r"C:\Program Files\Eclipse Adoptium",
                r"C:\Program Files (x86)\Eclipse Adoptium",
                r"C:\Program Files\Microsoft",
                r"C:\Program Files (x86)\Microsoft",
                r"C:\Program Files\GraalVM",
                r"C:\Program Files\Common Files\Oracle\Java",
            ]
            .iter()
            .map(PathBuf::from),
        );
        if depth == JavaScanDepth::Full {
            roots.push(PathBuf::from(r"C:\"));
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = depth;
        roots.push(PathBuf::from("/usr/lib/jvm"));
        roots.push(PathBuf::from("/Library/Java/JavaVirtualMachines"));
    }
    roots
}

fn discover_runtimes(depth: JavaScanDepth) -> Result<Vec<JavaRuntime>, JreError> {
    let mut paths = HashSet::new();

    // PATH entries.
    if let Ok(path) = std::env::var("PATH") {
        let sep = if cfg!(target_os = "windows") {
            ';'
        } else {
            ':'
        };
        for dir in path.split(sep) {
            paths.insert(PathBuf::from(dir));
        }
    }

    // JAVA_HOME.
    if let Ok(java_home) = std::env::var("JAVA_HOME") {
        paths.insert(PathBuf::from(java_home).join("bin"));
    }

    for root in java_search_roots(depth) {
        let Ok(entries) = fs::read_dir(&root) else {
            continue;
        };
        let is_drive_root = cfg!(windows) && root == Path::new(r"C:\");
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if name == "downloads" {
                continue;
            }
            if is_drive_root
                && !(name.starts_with("graalvm")
                    || name.starts_with("jdk")
                    || name.starts_with("zulu"))
            {
                continue;
            }
            paths.insert(path.join("bin"));
            paths.insert(path.join("Contents").join("Home").join("bin"));
        }
    }

    // Windows registry.
    #[cfg(target_os = "windows")]
    {
        use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ};
        let keys = [
            r"SOFTWARE\JavaSoft\Java Runtime Environment",
            r"SOFTWARE\JavaSoft\Java Development Kit",
            r"SOFTWARE\JavaSoft\JRE",
            r"SOFTWARE\JavaSoft\JDK",
            r"SOFTWARE\Eclipse Foundation\JDK",
            r"SOFTWARE\Eclipse Adoptium\JRE",
            r"SOFTWARE\Microsoft\JDK",
        ];
        for key in &keys {
            for flags in [
                KEY_READ,
                KEY_READ | 0x0100, /* KEY_WOW64_64KEY */
                KEY_READ | 0x0200, /* KEY_WOW64_32KEY */
            ] {
                if let Ok(jre_key) =
                    winreg::RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(key, flags)
                {
                    for subkey in jre_key.enum_keys().flatten() {
                        if let Ok(sk) = jre_key.open_subkey(subkey) {
                            for value_name in ["JavaHome", "InstallationPath"] {
                                if let Ok(path) = sk.get_value::<String, _>(value_name) {
                                    paths.insert(PathBuf::from(path).join("bin"));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let mut runtimes: Vec<JavaRuntime> = paths
        .into_iter()
        .filter_map(|p| check_java_at_path(&p).ok())
        .collect();
    runtimes.sort_by(|a, b| b.major.cmp(&a.major));
    runtimes.dedup_by(|a, b| a.path == b.path);
    Ok(runtimes)
}

/// Returns any installed Java, or downloads the latest GraalVM Community JDK
/// into the TuffBox managed runtime folder when none is found.
pub fn ensure_java() -> Result<JavaRuntime, JreError> {
    ensure_java_with_log(|_| {})
}

pub fn ensure_java_with_log<F>(mut log: F) -> Result<JavaRuntime, JreError>
where
    F: FnMut(&str),
{
    let mut runtimes = find_all_runtimes()?;
    if runtimes.is_empty() {
        runtimes = find_all_runtimes_full()?;
    }
    if let Some(rt) = runtimes.into_iter().next() {
        return Ok(rt);
    }
    log("# No Java found on this PC — downloading latest GraalVM Community JDK…");
    let installed = install_latest_graalvm(&mut log)?;
    invalidate_runtime_cache();
    Ok(installed)
}

/// True when none of the discovered runtimes can run `required_major`, so a
/// managed download is required. A newer JVM counts as compatible (vanilla /
/// Fabric run fine on newer majors; only old Forge installers are strict).
pub fn managed_install_needed(runtimes: &[JavaRuntime], required_major: u32) -> bool {
    !runtimes.iter().any(|r| r.major >= required_major)
}

/// True when a managed JDK download must happen before launching with
/// `required_major`. Legacy majors (Java 8 for MC <=1.16.4, Java 16 for 1.17)
/// require an EXACT match — old Forge breaks on newer JVMs, so a newer
/// installed runtime must not silently satisfy the requirement. Modern
/// majors accept any newer runtime (vanilla / Fabric run fine on newer
/// JVMs) and keep the historical `>=` behavior.
pub fn managed_jdk_needed(runtimes: &[JavaRuntime], required_major: u32) -> bool {
    if required_major <= 16 {
        !runtimes.iter().any(|r| r.major == required_major)
    } else {
        managed_install_needed(runtimes, required_major)
    }
}

/// Like [`ensure_java`], then picks the best match for `mc_version`.
/// If no installed runtime satisfies the Minecraft requirement, the matching
/// GraalVM Community JDK major is downloaded automatically — instead of
/// launching on a JVM that cannot run the game.
pub fn ensure_java_for_minecraft(mc_version: &str) -> Result<JavaRuntime, JreError> {
    ensure_java_for_minecraft_with_log(mc_version, |_| {})
}

pub fn ensure_java_for_minecraft_with_log<F>(
    mc_version: &str,
    mut log: F,
) -> Result<JavaRuntime, JreError>
where
    F: FnMut(&str),
{
    let mut runtimes = find_all_runtimes()?;
    if runtimes.is_empty() {
        runtimes = find_all_runtimes_full()?;
    }
    let required = required_java_major(mc_version);
    if managed_jdk_needed(&runtimes, required) {
        log(&format!(
            "# Minecraft {mc_version} needs Java {required} and no matching runtime is installed — downloading a managed JDK {required}…"
        ));
        let installed = install_managed_jdk_major(required, &mut log)?;
        invalidate_runtime_cache();
        runtimes.push(installed);
        runtimes.sort_by(|a, b| b.major.cmp(&a.major));
    }
    find_runtime_for(&runtimes, required).ok_or(JreError::NotFound)
}

fn java_bin_fingerprint(path: &Path) -> Option<(u64, u64)> {
    let meta = fs::metadata(path).ok()?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Some((meta.len(), mtime))
}

/// How long `java -version` may run before we give up on it.
///
/// A healthy JVM answers in well under a second, but a broken wrapper, an
/// antivirus scan or a JVM that deadlocks during startup would hang the
/// probe forever — and with it the whole crash-diagnosis preparation
/// (`check_java_at_path` runs inside `prepare_ai_crash_context`), which
/// surfaced as Diagnose stuck on "Analyzing…" indefinitely. 15s is ~20× the
/// observed p99 on Windows while still bounding a sick binary quickly.
const JAVA_VERSION_PROBE_TIMEOUT_SECS: u64 = 15;

/// `Command::output()` with a hard deadline: spawns, polls `try_wait`, and
/// kills the child when it exceeds [`JAVA_VERSION_PROBE_TIMEOUT_SECS`].
fn run_with_probe_timeout(mut c: Command) -> io::Result<std::process::Output> {
    c.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = c.spawn()?;
    let deadline =
        std::time::Instant::now() + std::time::Duration::from_secs(JAVA_VERSION_PROBE_TIMEOUT_SECS);
    loop {
        match child.try_wait()? {
            Some(_) => {
                // try_wait does not drain the pipes; wait_with_output reads
                // them to EOF now that the child has exited.
                return child.wait_with_output();
            }
            None if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("java -version did not answer within {JAVA_VERSION_PROBE_TIMEOUT_SECS}s"),
                ));
            }
            None => std::thread::sleep(std::time::Duration::from_millis(25)),
        }
    }
}

pub fn check_java_at_path(path: &Path) -> Result<JavaRuntime, JreError> {
    let bin = path.to_path_buf();
    let java_bin = if bin
        .file_name()
        .map(|f| f == java_binary_name())
        .unwrap_or(false)
    {
        bin
    } else {
        bin.join(java_binary_name())
    };

    if !java_bin.exists() {
        return Err(JreError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("java binary not found at {}", java_bin.display()),
        )));
    }

    if let Some((len, mtime_secs)) = java_bin_fingerprint(&java_bin) {
        if let Ok(guard) = JAVA_PATH_CACHE.lock() {
            if let Some(entry) = guard.as_ref().and_then(|m| m.get(&java_bin)) {
                if entry.len == len && entry.mtime_secs == mtime_secs {
                    return Ok(entry.runtime.clone());
                }
            }
        }
    }

    let output = {
        let mut c = Command::new(&java_bin);
        c.arg("-version").stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        // Bounded probe — a hung JVM must not pin diagnosis prep forever.
        run_with_probe_timeout(c)?
    };
    let stderr = String::from_utf8_lossy(&output.stderr);
    let first_line = stderr.lines().next().unwrap_or("").to_string();
    let major = parse_java_major(&first_line).ok_or(JreError::InvalidVersion)?;

    let runtime = JavaRuntime {
        path: java_bin.to_string_lossy().to_string(),
        version: first_line,
        major,
    };
    if let Some((len, mtime_secs)) = java_bin_fingerprint(&java_bin) {
        if let Ok(mut guard) = JAVA_PATH_CACHE.lock() {
            guard.get_or_insert_with(HashMap::new).insert(
                java_bin,
                JavaPathCacheEntry {
                    len,
                    mtime_secs,
                    runtime: runtime.clone(),
                },
            );
        }
    }
    Ok(runtime)
}

fn java_binary_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "java.exe"
    } else {
        "java"
    }
}

fn parse_java_major(version_string: &str) -> Option<u32> {
    let start = version_string.find('"').map(|i| i + 1)?;
    let end = version_string[start..].find('"').map(|i| start + i)?;
    let version = &version_string[start..end];
    let mut parts = version.split('.');
    let first = parts.next()?.parse::<u32>().ok()?;
    if first == 1 {
        parts.next()?.parse::<u32>().ok()
    } else {
        Some(first)
    }
}

#[derive(Debug, serde::Deserialize)]
struct GhRelease {
    tag_name: String,
    assets: Vec<GhAsset>,
}

#[derive(Debug, serde::Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    digest: Option<String>,
}

fn platform_asset_suffix() -> &'static str {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        return "windows-x64_bin.zip";
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        return "linux-x64_bin.tar.gz";
    }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        return "linux-aarch64_bin.tar.gz";
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        return "macos-aarch64_bin.tar.gz";
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        return "macos-x64_bin.tar.gz";
    }
    #[cfg(not(any(
        all(target_os = "windows", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
    )))]
    {
        "unsupported"
    }
}

fn install_latest_graalvm<F>(log: &mut F) -> Result<JavaRuntime, JreError>
where
    F: FnMut(&str),
{
    install_graalvm_matching(
        log,
        "https://api.github.com/repos/graalvm/graalvm-ce-builds/releases/latest",
        "latest",
        &|_tag| true,
    )
}

/// Downloads the newest GraalVM Community release whose major Java version
/// equals `major` (e.g. a `jdk-17.0.x` release) into the managed runtime.
/// Used when installed JVMs do not satisfy what the target Minecraft version
/// requires — the old behavior ("newest GraalVM or nothing") could not fix
/// a machine that only has an older Java than the game needs.
pub fn install_graalvm_major<F>(major: u32, log: &mut F) -> Result<JavaRuntime, JreError>
where
    F: FnMut(&str),
{
    install_graalvm_matching(
        log,
        "https://api.github.com/repos/graalvm/graalvm-ce-builds/releases?per_page=100",
        &format!("jdk-{major}"),
        &|tag| {
            // Release tags look like `jdk-17.0.12` (new scheme) — match the
            // major component exactly. Older tag schemes (`vm-22.3.1/jdk-17…`)
            // still contain `jdk-<major>.` so this covers both.
            let prefix = format!("jdk-{major}.");
            tag.contains(&prefix)
        },
    )
}

fn install_graalvm_matching<F>(
    log: &mut F,
    releases_url: &str,
    release_label: &str,
    tag_matches: &dyn Fn(&str) -> bool,
) -> Result<JavaRuntime, JreError>
where
    F: FnMut(&str),
{
    let suffix = platform_asset_suffix();
    if suffix == "unsupported" {
        return Err(JreError::Install(
            "this platform is not supported for automatic GraalVM download".into(),
        ));
    }

    let root = managed_java_root();
    fs::create_dir_all(&root).map_err(JreError::Io)?;
    let downloads = root.join("downloads");
    fs::create_dir_all(&downloads).map_err(JreError::Io)?;

    log("# Fetching GraalVM Community release metadata…");
    let releases: Vec<GhRelease> = crate::http::get_json(releases_url)
        .map_err(|e| JreError::Download(format!("GitHub releases API: {e}")))?;

    // First release (newest first) that has a platform asset with a matching tag.
    let (release, asset) = releases
        .iter()
        .filter(|r| tag_matches(&r.tag_name))
        .find_map(|r| {
            r.assets
                .iter()
                .find(|a| a.name.ends_with(suffix) && !a.name.ends_with(".sha256"))
                .map(|a| (r, a))
        })
        .ok_or_else(|| {
            JreError::Download(format!(
                "no GraalVM release matching '{release_label}' with asset '*{suffix}'"
            ))
        })?;

    let install_dir = root.join(&release.tag_name);
    let marker = install_dir.join(".tuffbox-java-ok");
    if marker.is_file() {
        if let Ok(rt) = find_java_under(&install_dir) {
            log(&format!(
                "# Reusing managed GraalVM {} at {}",
                release.tag_name, rt.path
            ));
            return Ok(rt);
        }
    }

    let archive_path = downloads.join(&asset.name);
    let sha256 = asset
        .digest
        .as_deref()
        .and_then(|d| d.strip_prefix("sha256:"))
        .map(|s| s.trim().to_string());

    log(&format!(
        "# Downloading {} ({})…",
        asset.name, release.tag_name
    ));
    crate::download_engine::download_resumable(
        &asset.browser_download_url,
        &archive_path,
        sha256
            .as_deref()
            .map(|s| (s, crate::download_engine::ChecksumKind::Sha256)),
        None,
        Some(std::time::Duration::from_secs(300)),
    )
    .map_err(|e| JreError::Download(e.to_string()))?;

    // Fresh extract target.
    if install_dir.exists() {
        let _ = fs::remove_dir_all(&install_dir);
    }
    fs::create_dir_all(&install_dir).map_err(JreError::Io)?;

    log(&format!("# Extracting to {}…", install_dir.display()));
    extract_archive(&archive_path, &install_dir)?;

    // GraalVM zips usually contain a single top-level folder; flatten if needed.
    flatten_single_child_dir(&install_dir)?;

    let runtime = find_java_under(&install_dir).map_err(|e| {
        JreError::Install(format!(
            "extracted GraalVM but java binary not found under {}: {e}",
            install_dir.display()
        ))
    })?;

    fs::write(&marker, release.tag_name.as_bytes()).map_err(JreError::Io)?;
    log(&format!(
        "# GraalVM {} ready: {} (Java {})",
        release.tag_name, runtime.path, runtime.major
    ));
    Ok(runtime)
}

/// Which distribution a managed JDK download uses for a given major.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JdkSource {
    /// Modern LTS majors (17/21) — the existing default runtime.
    GraalVM,
    /// Legacy majors (8 for MC <=1.16.4, 16 for 1.17): GraalVM CE publishes
    /// no jdk-8 / jdk-16 builds, so Adoptium Temurin — the community-standard
    /// runtime for old Forge — fills the gap.
    Temurin,
}

fn jdk_source_for_major(major: u32) -> JdkSource {
    match major {
        8 | 16 => JdkSource::Temurin,
        _ => JdkSource::GraalVM,
    }
}

/// Download a managed JDK for `major` from whichever distribution has a
/// build for it (see [`JdkSource`]).
pub fn install_managed_jdk_major<F>(major: u32, log: &mut F) -> Result<JavaRuntime, JreError>
where
    F: FnMut(&str),
{
    match jdk_source_for_major(major) {
        JdkSource::Temurin => install_temurin_major(major, log),
        JdkSource::GraalVM => install_graalvm_major(major, log),
    }
}

/// Adoptium Temurin release metadata (`/v3/assets/latest/{major}/hotspot`).
#[derive(Debug, serde::Deserialize)]
struct TemurinAsset {
    binary: TemurinBinary,
}

#[derive(Debug, serde::Deserialize)]
struct TemurinBinary {
    /// Archive file name, e.g. `OpenJDK8U-jdk_x64_windows_hotspot_8u422b05.zip`.
    name: String,
    /// Direct download link (GitHub `adoptium/temurinN-binaries`).
    link: String,
    /// sha256 of the archive (hex, no `sha256:` prefix).
    #[serde(default)]
    checksum: Option<String>,
}

fn adoptium_os() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "mac"
    } else {
        "linux"
    }
}

fn adoptium_arch() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "x64"
    }
}

fn adoptium_assets_url(major: u32, os: &str, arch: &str) -> String {
    format!(
        "https://api.adoptium.net/v3/assets/latest/{major}/hotspot?architecture={arch}&image_type=jdk&os={os}&project=jdk"
    )
}

/// Install the latest Adoptium Temurin JDK for `major` into the managed
/// runtime root. Mirrors [`install_graalvm_matching`]: checksum-verified
/// download, extract (zip / tar.gz), flatten the single top-level folder,
/// mark with `.tuffbox-java-ok` so re-runs reuse the install.
pub fn install_temurin_major<F>(major: u32, log: &mut F) -> Result<JavaRuntime, JreError>
where
    F: FnMut(&str),
{
    let os = adoptium_os();
    let arch = adoptium_arch();
    let url = adoptium_assets_url(major, os, arch);

    let root = managed_java_root();
    fs::create_dir_all(&root).map_err(JreError::Io)?;
    let downloads = root.join("downloads");
    fs::create_dir_all(&downloads).map_err(JreError::Io)?;

    log("# Fetching Adoptium Temurin release metadata…");
    let assets: Vec<TemurinAsset> = crate::http::get_json(&url)
        .map_err(|e| JreError::Download(format!("Adoptium API: {e}")))?;
    let binary = assets.first().map(|a| &a.binary).ok_or_else(|| {
        JreError::Download(format!(
            "Adoptium has no Temurin JDK {major} build for {os}/{arch}"
        ))
    })?;

    // Per-release directory (e.g. `temurin-OpenJDK8U-jdk_x64_windows_hotspot_8u422b05`)
    // so updating the same major installs beside, never over, the old one.
    let release_name = binary
        .name
        .trim_end_matches(".zip")
        .trim_end_matches(".tar.gz");
    let install_dir = root.join(format!("temurin-{release_name}"));
    let marker = install_dir.join(".tuffbox-java-ok");
    if marker.is_file() {
        if let Ok(rt) = find_java_under(&install_dir) {
            log(&format!(
                "# Reusing managed Temurin {release_name} at {}",
                rt.path
            ));
            return Ok(rt);
        }
    }

    let archive_path = downloads.join(&binary.name);
    let checksum = binary
        .checksum
        .as_deref()
        .map(|c| c.trim())
        .filter(|c| !c.is_empty());

    log(&format!(
        "# Downloading {} (Temurin JDK {major})…",
        binary.name
    ));
    crate::download_engine::download_resumable(
        &binary.link,
        &archive_path,
        checksum.map(|c| (c, crate::download_engine::ChecksumKind::Sha256)),
        None,
        Some(std::time::Duration::from_secs(600)),
    )
    .map_err(|e| JreError::Download(e.to_string()))?;

    // Fresh extract target.
    if install_dir.exists() {
        let _ = fs::remove_dir_all(&install_dir);
    }
    fs::create_dir_all(&install_dir).map_err(JreError::Io)?;

    log(&format!("# Extracting to {}…", install_dir.display()));
    extract_archive(&archive_path, &install_dir)?;

    // Temurin archives wrap a single top-level `jdk-…` / `jdk8u…` folder.
    flatten_single_child_dir(&install_dir)?;

    let runtime = find_java_under(&install_dir).map_err(|e| {
        JreError::Install(format!(
            "extracted Temurin but java binary not found under {}: {e}",
            install_dir.display()
        ))
    })?;

    fs::write(&marker, release_name.as_bytes()).map_err(JreError::Io)?;
    log(&format!(
        "# Temurin {release_name} ready: {} (Java {})",
        runtime.path, runtime.major
    ));
    Ok(runtime)
}

fn find_java_under(dir: &Path) -> Result<JavaRuntime, JreError> {
    let candidates = [
        dir.join("bin"),
        dir.join("Contents").join("Home").join("bin"),
    ];
    for bin in &candidates {
        if let Ok(rt) = check_java_at_path(bin) {
            return Ok(rt);
        }
    }
    // Shallow walk one extra level (zip root folder rename edge cases).
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if !p.is_dir() {
                continue;
            }
            for rel in ["bin", "Contents/Home/bin"] {
                if let Ok(rt) = check_java_at_path(&p.join(rel)) {
                    return Ok(rt);
                }
            }
        }
    }
    Err(JreError::NotFound)
}

fn flatten_single_child_dir(install_dir: &Path) -> Result<(), JreError> {
    let mut children: Vec<PathBuf> = fs::read_dir(install_dir)
        .map_err(JreError::Io)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .map(|n| n != ".tuffbox-java-ok")
                .unwrap_or(true)
        })
        .collect();
    if children.len() != 1 || !children[0].is_dir() {
        return Ok(());
    }
    // If the only child already has bin/java, keep nested layout — find_java_under handles it.
    let only = children.remove(0);
    if only.join("bin").join(java_binary_name()).is_file()
        || only
            .join("Contents")
            .join("Home")
            .join("bin")
            .join(java_binary_name())
            .is_file()
    {
        return Ok(());
    }
    Ok(())
}

fn extract_archive(archive: &Path, dest: &Path) -> Result<(), JreError> {
    let name = archive.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name.ends_with(".zip") {
        extract_zip(archive, dest)
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        extract_tar_gz_via_system_tar(archive, dest)
    } else {
        Err(JreError::Install(format!(
            "unsupported archive format: {name}"
        )))
    }
}

fn extract_zip(archive: &Path, dest: &Path) -> Result<(), JreError> {
    let file = fs::File::open(archive).map_err(JreError::Io)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| JreError::Install(e.to_string()))?;
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| JreError::Install(e.to_string()))?;
        let name = entry.name().replace('\\', "/");
        if name.contains("..") || name.starts_with('/') {
            return Err(JreError::Install(format!("path traversal in zip: {name}")));
        }
        let out_path = dest.join(&name);
        if entry.is_dir() || name.ends_with('/') {
            fs::create_dir_all(&out_path).map_err(JreError::Io)?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(JreError::Io)?;
        }
        let mut out = fs::File::create(&out_path).map_err(JreError::Io)?;
        io::copy(&mut entry, &mut out).map_err(JreError::Io)?;
    }
    Ok(())
}

fn extract_tar_gz_via_system_tar(archive: &Path, dest: &Path) -> Result<(), JreError> {
    // Prefer system tar on Unix (always present); avoids adding a tar crate dep.
    let status = Command::new("tar")
        .arg("-xzf")
        .arg(archive)
        .arg("-C")
        .arg(dest)
        .status()
        .map_err(|e| JreError::Install(format!("failed to run tar: {e}")))?;
    if !status.success() {
        return Err(JreError::Install(format!(
            "tar exited with status {status}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_java_major_matches_known_mojang_boundaries() {
        // Forge 1.20.1 famously needs Java 17, not whatever is newest on
        // the system — this is the exact scenario that used to crash with
        // `InaccessibleObjectException` deep inside Forge's bootstrap
        // launcher when TuffBox picked Java 21 instead.
        assert_eq!(required_java_major("1.20.1"), 17);
        assert_eq!(required_java_major("1.16.5"), 8);
        assert_eq!(required_java_major("1.12.2"), 8);
        assert_eq!(required_java_major("1.17"), 16);
        assert_eq!(required_java_major("1.18.2"), 17);
        assert_eq!(required_java_major("1.20.4"), 17);
        assert_eq!(required_java_major("1.20.5"), 21);
        assert_eq!(required_java_major("1.21.1"), 21);
    }

    fn runtime(major: u32) -> JavaRuntime {
        JavaRuntime {
            path: format!("/fake/java{major}"),
            version: format!("{major}.0.0"),
            major,
        }
    }

    #[test]
    fn managed_jdk_needed_legacy_majors_require_exact_match() {
        // Old Forge (MC <=1.16.4 -> Java 8, 1.17 -> Java 16) crashes on
        // newer JVMs: a newer installed runtime must NOT satisfy the
        // requirement — the exact major has to be downloaded (Temurin).
        assert!(managed_jdk_needed(&[], 8));
        assert!(managed_jdk_needed(&[runtime(17)], 8));
        assert!(managed_jdk_needed(&[runtime(21)], 16));
        assert!(!managed_jdk_needed(&[runtime(8)], 8));
        assert!(!managed_jdk_needed(&[runtime(8), runtime(21)], 8));
        assert!(!managed_jdk_needed(&[runtime(16)], 16));
    }

    #[test]
    fn managed_jdk_needed_modern_majors_accept_newer_runtimes() {
        // Modern MC runs fine on newer JVMs — no forced download.
        assert!(!managed_jdk_needed(&[runtime(21)], 17));
        assert!(!managed_jdk_needed(&[runtime(25)], 21));
        assert!(managed_jdk_needed(&[runtime(8)], 17));
        assert!(managed_jdk_needed(&[], 21));
    }

    #[test]
    fn jdk_source_routing_fills_graalvm_gaps_with_temurin() {
        // GraalVM CE publishes no jdk-8 / jdk-16 builds.
        assert_eq!(jdk_source_for_major(8), JdkSource::Temurin);
        assert_eq!(jdk_source_for_major(16), JdkSource::Temurin);
        assert_eq!(jdk_source_for_major(11), JdkSource::GraalVM);
        assert_eq!(jdk_source_for_major(17), JdkSource::GraalVM);
        assert_eq!(jdk_source_for_major(21), JdkSource::GraalVM);
    }

    #[test]
    fn adoptium_url_targets_major_and_platform() {
        let url = adoptium_assets_url(8, "windows", "x64");
        assert!(url.starts_with("https://api.adoptium.net/v3/assets/latest/"));
        assert!(url.contains("/8/hotspot?"));
        assert!(url.contains("architecture=x64"));
        assert!(url.contains("os=windows"));
        assert!(url.contains("project=jdk"));
    }

    #[test]
    fn adoptium_platform_names_are_api_valid() {
        assert!(["windows", "linux", "mac"].contains(&adoptium_os()));
        assert!(["x64", "aarch64"].contains(&adoptium_arch()));
    }

    #[test]
    fn find_runtime_for_prefers_exact_match() {
        let runtimes = vec![runtime(8), runtime(17), runtime(21)];
        let picked = find_runtime_for(&runtimes, 17).unwrap();
        assert_eq!(picked.major, 17);
    }

    #[test]
    fn find_runtime_for_falls_back_to_closest_newer_when_no_exact_match() {
        // Regression test: previously the launcher always picked the
        // single newest installed JVM regardless of what was needed,
        // which breaks old Forge installers built against pre-module-system
        // Java. This verifies the *closest* compatible version is chosen
        // instead of always jumping to the newest.
        let runtimes = vec![runtime(8), runtime(11), runtime(21)];
        let picked = find_runtime_for(&runtimes, 17).unwrap();
        assert_eq!(
            picked.major, 21,
            "should pick the closest newer runtime, not skip past it"
        );
    }

    #[test]
    fn find_runtime_for_falls_back_to_newest_when_nothing_meets_requirement() {
        let runtimes = vec![runtime(8), runtime(11)];
        let picked = find_runtime_for(&runtimes, 21).unwrap();
        assert_eq!(picked.major, 11);
    }

    #[test]
    fn find_runtime_for_empty_list_returns_none() {
        assert!(find_runtime_for(&[], 17).is_none());
    }

    #[test]
    fn managed_install_needed_when_all_runtimes_too_old() {
        // Only Java 8 + 11 installed, MC needs 17 → must download, not
        // silently fall back to an old JVM that will refuse to boot MC.
        let runtimes = vec![runtime(8), runtime(11)];
        assert!(managed_install_needed(&runtimes, 17));
    }

    #[test]
    fn managed_install_not_needed_when_compatible_runtime_exists() {
        let runtimes = vec![runtime(8), runtime(17), runtime(21)];
        assert!(!managed_install_needed(&runtimes, 17));
        // Newer-than-required JVM is usable for vanilla/Fabric.
        let runtimes = vec![runtime(8), runtime(21)];
        assert!(!managed_install_needed(&runtimes, 17));
    }

    #[test]
    fn managed_install_needed_when_nothing_installed() {
        assert!(managed_install_needed(&[], 17));
    }

    #[test]
    fn platform_suffix_is_known() {
        assert_ne!(platform_asset_suffix(), "unsupported");
    }

    #[test]
    fn java_bin_fingerprint_reads_size() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("java.exe");
        std::fs::write(&bin, b"not-a-real-jvm").unwrap();
        let (len, _mtime) = java_bin_fingerprint(&bin).expect("fingerprint");
        assert_eq!(len, 14);
    }

    #[test]
    fn check_java_at_path_errors_when_binary_missing() {
        let dir = tempfile::tempdir().unwrap();
        let err = check_java_at_path(&dir.path().join("missing-java")).unwrap_err();
        match err {
            JreError::Io(e) => assert_eq!(e.kind(), std::io::ErrorKind::NotFound),
            other => panic!("expected Io, got {other}"),
        }
    }

    #[test]
    fn fast_search_roots_skip_windows_drive_root() {
        let roots = java_search_roots(JavaScanDepth::Fast);
        assert!(
            !roots.iter().any(|p| p == Path::new(r"C:\")),
            "Fast scan must not list C:\\"
        );
        let full = java_search_roots(JavaScanDepth::Full);
        #[cfg(target_os = "windows")]
        assert!(full.iter().any(|p| p == Path::new(r"C:\")));
        #[cfg(not(target_os = "windows"))]
        assert_eq!(roots, full);
    }
}
