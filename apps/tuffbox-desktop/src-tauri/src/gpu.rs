//! GPU detection + preference for the Minecraft Java process.
//!
//! Detects discrete vs integrated adapters (DXGI on Windows, sysfs on Linux)
//! and steers the game at its discrete GPU by default:
//! - Linux: PRIME offload env (`DRI_PRIME` / NVIDIA offload trio) on the child.
//! - Windows: per-app `GpuPreference` under `HKCU\...\DirectX\UserGpuPreferences`
//!   for the exact java binary (plus its `javaw.exe` sibling).
//! Everything is best-effort and never blocks the game — same philosophy as
//! `cpu_affinity`.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuInfo {
    /// Stable id: `pci-0000_01_00_0` on Linux, `dxgi-N` on Windows.
    pub id: String,
    pub name: String,
    /// `nvidia` | `amd` | `intel` | `apple` | `unknown`.
    pub vendor: String,
    /// `discrete` | `integrated` | `unknown`.
    pub kind: String,
    pub vram_mb: Option<u64>,
    /// Drives the boot display (boot_vga / attached outputs).
    pub primary: bool,
    /// Linux PCI slot (`0000:01:00.0`) for exact `DRI_PRIME=pci-…`.
    pub pci_slot: Option<String>,
}

/// Classify an adapter from PCI vendor/device ids + advertised name.
/// Pure — unit-tested; platform layers only refine `unknown`.
pub fn classify_gpu(vendor_id: u16, device_id: u16, name: &str) -> &'static str {
    let lname = name.to_lowercase();
    match vendor_id {
        // NVIDIA PCI parts are discrete (GeForce/Quadro/RTX).
        0x10DE => "discrete",
        0x8086 => {
            // Intel: everything is integrated graphics except Arc.
            // DG2/Alchemist (0x569x–0x56xx) and Battlemage (0xE2xx).
            if lname.contains("arc")
                || (0x5690..=0x56ff).contains(&device_id)
                || (0xe200..=0xe2ff).contains(&device_id)
            {
                "discrete"
            } else {
                "integrated"
            }
        }
        0x1002 => {
            // AMD: RX / Pro / big-Vega parts are discrete; APUs report as
            // "…Graphics" (780M Graphics, Vega 8, (TM) Graphics).
            if lname.contains(" rx")
                || lname.starts_with("rx")
                || lname.contains("radeon pro")
                || lname.contains("vega 56")
                || lname.contains("vega 64")
                || lname.contains("radeon vii")
            {
                "discrete"
            } else if lname.contains("graphics")
                || lname.contains("ryzen")
                || lname.contains(" vega ")
                || lname.contains("radeon 6")
                || lname.contains("radeon 7")
                || lname.contains("radeon 8")
                || lname.contains("radeon 9")
            {
                "integrated"
            } else {
                "unknown"
            }
        }
        0x106B => "integrated",
        _ => "unknown",
    }
}

pub fn vendor_label(vendor_id: u16) -> &'static str {
    match vendor_id {
        0x10DE => "nvidia",
        0x1002 => "amd",
        0x8086 => "intel",
        0x106B => "apple",
        _ => "unknown",
    }
}

/// Resolve the launch target: `auto` prefers discrete, then the primary
/// adapter, then whatever was detected first. `integrated` falls back the
/// same way when no iGPU is reported (single-dGPU desktop).
pub fn resolve_target_gpu<'a>(mode: &str, gpus: &'a [GpuInfo]) -> Option<&'a GpuInfo> {
    if gpus.is_empty() {
        return None;
    }
    let want = match mode {
        "discrete" | "integrated" => Some(mode),
        _ => None, // "auto" and anything unknown
    };
    if want == Some("discrete") {
        if let Some(g) = gpus.iter().find(|g| g.kind == "discrete") {
            return Some(g);
        }
    }
    if want == Some("integrated") || want.is_none() {
        if want.is_none() {
            if let Some(g) = gpus.iter().find(|g| g.kind == "discrete") {
                return Some(g);
            }
        } else if let Some(g) = gpus.iter().find(|g| g.kind == "integrated") {
            return Some(g);
        }
    }
    gpus.iter().find(|g| g.primary).or_else(|| gpus.first())
}

/// Linux PRIME offload env for the resolved target. Empty when there is
/// nothing to steer (single GPU, or an integrated target on a hybrid box
/// where the iGPU already renders by default).
pub fn linux_prime_env(target: &GpuInfo, gpu_count: usize) -> Vec<(String, String)> {
    if target.kind != "discrete" || gpu_count < 2 {
        return Vec::new();
    }
    if target.vendor == "nvidia" {
        return vec![
            ("__NV_PRIME_RENDER_OFFLOAD".into(), "1".into()),
            ("__GLX_VENDOR_LIBRARY_NAME".into(), "nvidia".into()),
            ("__VK_LAYER_NV_optimus".into(), "NVIDIA_only".into()),
        ];
    }
    // Mesa offload: exact PCI slot when known, otherwise "second GPU".
    let selector = target
        .pci_slot
        .as_ref()
        .map(|s| format!("pci-{}", s.replace([':', '.'], "_")))
        .unwrap_or_else(|| "1".into());
    vec![("DRI_PRIME".into(), selector)]
}

/// Windows `UserGpuPreferences` value for the target kind.
/// None = leave the OS default (unknown adapter).
pub fn windows_gpu_preference_value(kind: &str) -> Option<&'static str> {
    match kind {
        "discrete" => Some("GpuPreference=2;"),
        "integrated" => Some("GpuPreference=1;"),
        _ => None,
    }
}

fn sort_key(kind: &str) -> u8 {
    match kind {
        "discrete" => 0,
        "integrated" => 1,
        _ => 2,
    }
}

// ── Windows: DXGI enumeration ────────────────────────────────────────────

#[cfg(target_os = "windows")]
pub fn detect_platform_gpus() -> Result<Vec<GpuInfo>, String> {
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIFactory1,
    };

    let factory: IDXGIFactory1 =
        unsafe { CreateDXGIFactory1().map_err(|e| format!("CreateDXGIFactory1: {e}"))? };
    let mut gpus = Vec::new();
    let mut index = 0u32;
    loop {
        let adapter = match unsafe { factory.EnumAdapters1(index) } {
            Ok(a) => a,
            Err(_) => break,
        };
        index += 1;
        let desc = match unsafe { adapter.GetDesc1() } {
            Ok(d) => d,
            Err(_) => continue,
        };
        if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
            continue; // WARP / Basic Render Driver — not a real GPU
        }
        let end = desc
            .Description
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(desc.Description.len());
        let name = String::from_utf16_lossy(&desc.Description[..end])
            .trim()
            .to_string();
        let name = if name.is_empty() {
            format!("GPU {index}")
        } else {
            name
        };
        let vendor_id = desc.VendorId as u16;
        let gpus_len = gpus.len();
        gpus.push(GpuInfo {
            id: format!("dxgi-{gpus_len}"),
            kind: classify_gpu(vendor_id, desc.DeviceId as u16, &name).to_string(),
            vendor: vendor_label(vendor_id).to_string(),
            name,
            vram_mb: Some(desc.DedicatedVideoMemory as u64 / 1024 / 1024),
            primary: unsafe { adapter.EnumOutputs(0) }.is_ok(),
            pci_slot: None,
        });
    }
    gpus.sort_by(|a, b| {
        sort_key(&a.kind)
            .cmp(&sort_key(&b.kind))
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(gpus)
}

/// Write the per-app Windows graphics preference for the java binary.
/// HKCU — no elevation needed. Best-effort: errors are logged, not fatal.
#[cfg(target_os = "windows")]
pub fn apply_windows_gpu_preference(java_path: &str, preference_value: &str) -> Result<(), String> {
    use winreg::enums::HKEY_CURRENT_USER;
    let hkcu = winreg::RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey(r"Software\Microsoft\DirectX\UserGpuPreferences")
        .map_err(|e| format!("open UserGpuPreferences: {e}"))?;
    key.set_value(java_path, &preference_value.to_string())
        .map_err(|e| format!("set {java_path}: {e}"))?;
    // Sibling javaw.exe — whichever binary creates the GL context gets the hint.
    if let Some(dir) = std::path::Path::new(java_path).parent() {
        let javaw = dir.join("javaw.exe");
        if javaw.is_file() {
            if let Some(s) = javaw.to_str() {
                let _ = key.set_value(s, &preference_value.to_string());
            }
        }
    }
    Ok(())
}

// ── Linux: sysfs DRM enumeration ─────────────────────────────────────────

/// `card0`, `card1`, … — but not `card0-DP-1` connectors or `renderD128`.
fn is_drm_card(name: &str) -> bool {
    name.strip_prefix("card").map(|rest| {
        !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit())
    }) == Some(true)
}

fn parse_hex_u32(raw: &str) -> Option<u32> {
    let s = raw.trim();
    let s = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
    if s.is_empty() {
        return None;
    }
    u32::from_str_radix(s, 16).ok()
}

fn read_trimmed(path: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

/// Parse one pci.ids database text: `VVVV  Vendor` lines, `\tDDDD  Device`.
fn lookup_pci_name_in_text(text: &str, vendor: u16, device: u16) -> Option<String> {
    let want_v = format!("{vendor:04x}");
    let want_d = format!("{device:04x}");
    let mut in_vendor = false;
    for line in text.lines() {
        if line.is_empty() || line.starts_with('#') || line.starts_with("\t\t") {
            continue;
        }
        if let Some(rest) = line.strip_prefix('\t') {
            if !in_vendor {
                continue;
            }
            if rest.get(..4).map(|id| id.eq_ignore_ascii_case(&want_d)) == Some(true) {
                return Some(rest.get(4..).unwrap_or("").trim().to_string());
            }
        } else if line.get(..4).map(|id| id.eq_ignore_ascii_case(&want_v)) == Some(true) {
            in_vendor = true;
        } else if !line.starts_with([' ', '\t']) {
            in_vendor = false;
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn lookup_pci_name(vendor: u16, device: u16) -> Option<String> {
    for path in ["/usr/share/hwdata/pci.ids", "/usr/share/misc/pci.ids"] {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        if let Some(name) = lookup_pci_name_in_text(&text, vendor, device) {
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

#[cfg(target_os = "linux")]
pub fn detect_platform_gpus() -> Result<Vec<GpuInfo>, String> {
    let mut gpus = Vec::new();
    let entries =
        std::fs::read_dir("/sys/class/drm").map_err(|e| format!("read /sys/class/drm: {e}"))?;
    for entry in entries.flatten() {
        let file_name = entry.file_name().to_string_lossy().to_string();
        if !is_drm_card(&file_name) {
            continue;
        }
        let dev = entry.path().join("device");
        let vendor = read_trimmed(&dev.join("vendor"))
            .and_then(|s| parse_hex_u32(&s))
            .unwrap_or(0) as u16;
        if vendor == 0 {
            continue;
        }
        let device = read_trimmed(&dev.join("device"))
            .and_then(|s| parse_hex_u32(&s))
            .unwrap_or(0) as u16;
        let class = read_trimmed(&dev.join("class"))
            .and_then(|s| parse_hex_u32(&s))
            .unwrap_or(0);
        let boot_vga = read_trimmed(&dev.join("boot_vga")).as_deref() == Some("1");
        let driver = std::fs::read_link(dev.join("driver"))
            .ok()
            .and_then(|p| {
                p.file_name()
                    .map(|s| s.to_string_lossy().to_string())
            })
            .unwrap_or_default();
        let pci_slot = std::fs::read_link(&dev)
            .ok()
            .and_then(|p| p.file_name().map(|s| s.to_string_lossy().to_string()));
        let mut name = lookup_pci_name(vendor, device).unwrap_or_default();
        if name.is_empty() {
            let label = match vendor_label(vendor) {
                "unknown" => format!("PCI {vendor:04x}:{device:04x}"),
                v => format!("{} Graphics", capitalize(v)),
            };
            name = if driver.is_empty() {
                label
            } else {
                format!("{label} ({driver})")
            };
        }
        let mut kind = classify_gpu(vendor, device, &name).to_string();
        let vram_mb = read_trimmed(&dev.join("mem_info_vram_total"))
            .and_then(|s| s.parse::<u64>().ok())
            .map(|bytes| bytes / 1024 / 1024);
        if kind == "unknown" {
            // 3D-controller class = render-only discrete part (offload mode).
            // AMD without a decoded name: big VRAM carve-out = discrete;
            // otherwise the boot-VGA adapter is the APU's iGPU.
            if class == 0x030200 {
                kind = "discrete".into();
            } else if vendor == 0x1002 {
                let big_vram = vram_mb.unwrap_or(0) >= 1024;
                kind = if big_vram || !boot_vga {
                    "discrete"
                } else {
                    "integrated"
                }
                .into();
            }
        }
        let id = pci_slot.as_ref().map(|s| {
            format!("pci-{}", s.replace([':', '.'], "_"))
        }).unwrap_or_else(|| format!("drm-{file_name}"));
        gpus.push(GpuInfo {
            id,
            name,
            vendor: vendor_label(vendor).to_string(),
            kind,
            vram_mb,
            primary: boot_vga,
            pci_slot,
        });
    }
    gpus.sort_by(|a, b| {
        sort_key(&a.kind)
            .cmp(&sort_key(&b.kind))
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(gpus)
}

#[cfg(target_os = "linux")]
fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

// ── Other platforms: detection not implemented yet ───────────────────────

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub fn detect_platform_gpus() -> Result<Vec<GpuInfo>, String> {
    Ok(Vec::new())
}

#[tauri::command(rename_all = "camelCase")]
pub fn detect_gpus() -> Result<Vec<GpuInfo>, String> {
    detect_platform_gpus()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gpu(kind: &str, primary: bool) -> GpuInfo {
        GpuInfo {
            id: format!("{kind}-{primary}"),
            name: kind.into(),
            vendor: "test".into(),
            kind: kind.into(),
            vram_mb: None,
            primary,
            pci_slot: None,
        }
    }

    #[test]
    fn classify_matrix() {
        // NVIDIA always discrete.
        assert_eq!(classify_gpu(0x10DE, 0x2484, "NVIDIA GeForce RTX 3070"), "discrete");
        // Intel iGPU vs Arc.
        assert_eq!(classify_gpu(0x8086, 0x9BC4, "Intel UHD Graphics 630"), "integrated");
        assert_eq!(classify_gpu(0x8086, 0x5690, "Intel Arc A750"), "discrete");
        assert_eq!(classify_gpu(0x8086, 0x56A5, "Intel Arc A770"), "discrete");
        assert_eq!(classify_gpu(0x8086, 0xE20B, "Intel Battlemage"), "discrete");
        // AMD discrete vs APU.
        assert_eq!(classify_gpu(0x1002, 0x73DF, "AMD Radeon RX 6700 XT"), "discrete");
        assert_eq!(classify_gpu(0x1002, 0x15BF, "AMD Radeon 780M Graphics"), "integrated");
        assert_eq!(classify_gpu(0x1002, 0x1636, "AMD Radeon(TM) Graphics"), "integrated");
        assert_eq!(classify_gpu(0x1002, 0x15D8, "AMD Radeon Vega 8 Graphics"), "integrated");
        assert_eq!(classify_gpu(0x1002, 0x687F, "AMD Radeon RX Vega 56"), "discrete");
        assert_eq!(classify_gpu(0x1002, 0x7408, "AMD Radeon PRO W6800"), "discrete");
        assert_eq!(classify_gpu(0x106B, 0x0000, "Apple M1"), "integrated");
        assert_eq!(classify_gpu(0x1234, 0x1111, "Whatever"), "unknown");
    }

    #[test]
    fn resolve_prefers_discrete_then_primary() {
        let hybrid = vec![gpu("integrated", true), gpu("discrete", false)];
        assert_eq!(resolve_target_gpu("auto", &hybrid).unwrap().kind, "discrete");
        assert_eq!(resolve_target_gpu("discrete", &hybrid).unwrap().kind, "discrete");
        assert_eq!(resolve_target_gpu("integrated", &hybrid).unwrap().kind, "integrated");
        // No dGPU: auto falls back to primary.
        let solo = vec![gpu("integrated", true)];
        assert_eq!(resolve_target_gpu("auto", &solo).unwrap().kind, "integrated");
        assert_eq!(resolve_target_gpu("discrete", &solo).unwrap().kind, "integrated");
        // Unknown mode behaves like auto; empty list resolves to nothing.
        assert_eq!(resolve_target_gpu("bogus", &hybrid).unwrap().kind, "discrete");
        assert!(resolve_target_gpu("auto", &[]).is_none());
    }

    #[test]
    fn prime_env_cases() {
        let nvidia = GpuInfo {
            vendor: "nvidia".into(),
            kind: "discrete".into(),
            pci_slot: Some("0000:01:00.0".into()),
            ..gpu("discrete", false)
        };
        let env = linux_prime_env(&nvidia, 2);
        assert!(env.iter().any(|(k, v)| k == "__NV_PRIME_RENDER_OFFLOAD" && v == "1"));
        assert!(env.iter().any(|(k, v)| k == "__GLX_VENDOR_LIBRARY_NAME" && v == "nvidia"));

        let amd = GpuInfo {
            vendor: "amd".into(),
            kind: "discrete".into(),
            pci_slot: Some("0000:03:00.0".into()),
            ..gpu("discrete", false)
        };
        assert_eq!(
            linux_prime_env(&amd, 2),
            vec![("DRI_PRIME".to_string(), "pci-0000_03_00_0".to_string())]
        );
        // Single GPU / integrated target: nothing to steer.
        assert!(linux_prime_env(&amd, 1).is_empty());
        assert!(linux_prime_env(&gpu("integrated", true), 2).is_empty());
    }

    #[test]
    fn windows_preference_values() {
        assert_eq!(windows_gpu_preference_value("discrete"), Some("GpuPreference=2;"));
        assert_eq!(windows_gpu_preference_value("integrated"), Some("GpuPreference=1;"));
        assert_eq!(windows_gpu_preference_value("unknown"), None);
    }

    #[test]
    fn drm_card_filter() {
        assert!(is_drm_card("card0"));
        assert!(is_drm_card("card12"));
        assert!(!is_drm_card("card0-DP-1"));
        assert!(!is_drm_card("renderD128"));
        assert!(!is_drm_card("card"));
        assert_eq!(parse_hex_u32("0x8086\n"), Some(0x8086));
        assert_eq!(parse_hex_u32("0X030200"), Some(0x030200));
        assert_eq!(parse_hex_u32(""), None);
    }

    #[test]
    fn pci_ids_lookup() {
        let text = "# comment\n8086  Intel Corporation\n\t9bc4  CometLake-S GT2 [UHD Graphics 630]\n10de  NVIDIA Corporation\n\t2484  GA104 [GeForce RTX 3070]\n";
        assert_eq!(
            lookup_pci_name_in_text(text, 0x10de, 0x2484).as_deref(),
            Some("GA104 [GeForce RTX 3070]")
        );
        assert_eq!(lookup_pci_name_in_text(text, 0x10de, 0x9999), None);
        assert_eq!(lookup_pci_name_in_text(text, 0x1234, 0x0000), None);
    }
}
