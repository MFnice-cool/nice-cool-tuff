use crate::{LoaderKind, ProjectManifest};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct RecipeBridgeLaunch {
    pub jvm_args: Vec<String>,
    pub cleanup_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipeRuntimeStatus {
    pub connected: bool,
    pub supported: bool,
    pub message: String,
    pub minecraft_version: Option<String>,
    pub pid: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecipeBridgeHandshake {
    protocol_version: u32,
    host: String,
    port: u16,
    token: String,
    pid: u64,
    minecraft_version: String,
}

pub fn prepare_recipe_bridge(
    manifest: &ProjectManifest,
    game_dir: &Path,
) -> Result<Option<RecipeBridgeLaunch>, String> {
    if manifest.minecraft.version != "1.21.1"
        || !matches!(
            manifest.loader.kind,
            LoaderKind::Fabric | LoaderKind::Neoforge
        )
    {
        return Ok(None);
    }
    let mods_dir = game_dir.join("mods");
    let jei_present = manifest.mods.iter().any(|module| module.id == "jei")
        || std::fs::read_dir(&mods_dir)
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .starts_with("jei-")
            });
    if !jei_present {
        return Ok(None);
    }

    let Some(source) = find_bridge_jar(&manifest.loader.kind) else {
        return Ok(None);
    };
    std::fs::create_dir_all(&mods_dir).map_err(|error| error.to_string())?;
    let installed = mods_dir.join("tuffbox-jei-bridge.runtime.jar");
    let skip_copy = installed.is_file()
        && std::fs::metadata(&source)
            .ok()
            .zip(std::fs::metadata(&installed).ok())
            .is_some_and(|(s, d)| s.len() == d.len());
    if !skip_copy {
        std::fs::copy(&source, &installed).map_err(|error| {
            format!(
                "failed to install JEI runtime bridge {}: {error}",
                source.display()
            )
        })?;
    }

    let runtime_dir = game_dir.join(".tuffbox");
    std::fs::create_dir_all(&runtime_dir).map_err(|error| error.to_string())?;
    let handshake = runtime_dir.join("jei-bridge.json");
    let _ = std::fs::remove_file(&handshake);
    let random_file = tempfile::NamedTempFile::new_in(&runtime_dir).map_err(|e| e.to_string())?;
    let token = format!(
        "{:x}",
        Sha256::digest(random_file.path().to_string_lossy().as_bytes())
    );

    Ok(Some(RecipeBridgeLaunch {
        jvm_args: vec![
            format!("-Dtuffbox.bridge.token={token}"),
            format!("-Dtuffbox.bridge.handshake={}", handshake.to_string_lossy()),
        ],
        cleanup_paths: vec![installed, handshake],
    }))
}

pub fn recipe_runtime_status(manifest_path: &Path) -> RecipeRuntimeStatus {
    let supported = ProjectManifest::load_from_path(manifest_path)
        .map(|manifest| {
            manifest.minecraft.version == "1.21.1"
                && matches!(
                    manifest.loader.kind,
                    LoaderKind::Fabric | LoaderKind::Neoforge
                )
        })
        .unwrap_or(false);
    match load_handshake(manifest_path) {
        Ok(handshake) => match request(&handshake, "/health") {
            Ok(_) => RecipeRuntimeStatus {
                connected: true,
                supported,
                message: "Connected to JEI runtime".to_string(),
                minecraft_version: Some(handshake.minecraft_version),
                pid: Some(handshake.pid),
            },
            Err(error) => RecipeRuntimeStatus {
                connected: false,
                supported,
                message: format!("JEI bridge is not responding: {error}"),
                minecraft_version: Some(handshake.minecraft_version),
                pid: Some(handshake.pid),
            },
        },
        Err(error) => RecipeRuntimeStatus {
            connected: false,
            supported,
            message: if supported {
                format!("Launch the client with JEI to enable live recipes ({error})")
            } else {
                "Live JEI supports Fabric and NeoForge 1.21.1; offline recipes remain available"
                    .to_string()
            },
            minecraft_version: None,
            pid: None,
        },
    }
}

pub fn fetch_recipe_runtime_snapshot(manifest_path: &Path) -> Result<serde_json::Value, String> {
    let handshake = load_handshake(manifest_path)?;
    request(&handshake, "/v1/snapshot")
}

fn load_handshake(manifest_path: &Path) -> Result<RecipeBridgeHandshake, String> {
    let project_dir = manifest_path
        .parent()
        .ok_or_else(|| "manifest has no parent directory".to_string())?;
    let path = project_dir.join(".tuffbox").join("jei-bridge.json");
    let raw = std::fs::read_to_string(&path)
        .map_err(|_| format!("no active bridge at {}", path.display()))?;
    let handshake: RecipeBridgeHandshake =
        serde_json::from_str(&raw).map_err(|error| error.to_string())?;
    if handshake.protocol_version != 1 || handshake.host != "127.0.0.1" {
        return Err("unsupported or unsafe JEI bridge handshake".to_string());
    }
    Ok(handshake)
}

fn request(handshake: &RecipeBridgeHandshake, endpoint: &str) -> Result<serde_json::Value, String> {
    let url = format!("http://127.0.0.1:{}{endpoint}", handshake.port);
    let response = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(35))
        .build()
        .map_err(|error| error.to_string())?
        .get(url)
        .header("X-TuffBox-Token", &handshake.token)
        .send()
        .map_err(|error| error.to_string())?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("bridge returned HTTP {status}"));
    }
    response.json().map_err(|error| error.to_string())
}

fn find_bridge_jar(loader: &LoaderKind) -> Option<PathBuf> {
    let loader_name = match loader {
        LoaderKind::Fabric => "fabric",
        LoaderKind::Neoforge => "neoforge",
        _ => return None,
    };
    let expected_prefix = format!("tuffbox-jei-bridge-1.21.1-{loader_name}-");
    let mut roots = Vec::new();
    if let Some(path) = std::env::var_os("TUFFBOX_JEI_BRIDGE_DIR") {
        roots.push(PathBuf::from(path));
    }
    roots.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("bridges")
            .join("jei-runtime")
            .join(loader_name)
            .join("build")
            .join("libs"),
    );
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            roots.push(parent.join("resources").join("jei-bridge"));
            roots.push(parent.join("jei-bridge"));
        }
    }
    roots.into_iter().find_map(|root| {
        std::fs::read_dir(root)
            .ok()?
            .flatten()
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        name.starts_with(&expected_prefix) && name.ends_with(".jar")
                    })
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_loopback_handshake() {
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = dir.path().join("project.tuffbox.json");
        std::fs::create_dir_all(dir.path().join(".tuffbox")).unwrap();
        std::fs::write(
            dir.path().join(".tuffbox/jei-bridge.json"),
            r#"{"protocolVersion":1,"host":"0.0.0.0","port":1,"token":"x","pid":1,"minecraftVersion":"1.21.1"}"#,
        )
        .unwrap();
        assert!(load_handshake(&manifest_path).is_err());
    }
}
