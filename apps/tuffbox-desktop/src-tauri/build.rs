use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    // Best-effort: copy workspace-built tuffswarm-node into src-tauri/binaries/
    // so Tauri can bundle it next to the desktop app (Fog L2 spawn).
    copy_tuffswarm_node_sidecar();
    tauri_build::build()
}

fn copy_tuffswarm_node_sidecar() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest
        .join("../../..")
        .canonicalize()
        .unwrap_or_else(|_| manifest.join("../../.."));
    let target_dir = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root.join("target"));

    let bin_name = if cfg!(windows) {
        "tuffswarm-node.exe"
    } else {
        "tuffswarm-node"
    };

    let dest_dir = manifest.join("binaries");
    let _ = fs::create_dir_all(&dest_dir);
    let dest = dest_dir.join(bin_name);

    for profile in ["release", "debug"] {
        let src = target_dir.join(profile).join(bin_name);
        if !src.is_file() {
            continue;
        }
        match fs::copy(&src, &dest) {
            Ok(_) => {
                println!(
                    "cargo:warning=copied {} → binaries/{}",
                    src.display(),
                    bin_name
                );
                println!("cargo:rerun-if-changed={}", src.display());
            }
            Err(e) => {
                println!(
                    "cargo:warning=could not copy tuffswarm-node sidecar ({}): {e}",
                    src.display()
                );
            }
        }
        break;
    }

    println!(
        "cargo:rerun-if-changed={}",
        dest_dir.join(".gitkeep").display()
    );
}
