//! CPU affinity pinning for the Minecraft Java process.
//!
//! On hybrid CPUs (Intel 12th–14th gen P/E cores) Windows may schedule the
//! render thread on E-cores. When enabled in Settings, the launcher pins the
//! spawned javaw process to performance cores via SetProcessAffinityMask.
//! Manual mode covers AMD X3D dual-CCD CPUs (no OS-reported efficiency split).

/// Settings mode for CPU affinity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AffinityConfig {
    /// `off` | `performance` | `manual`
    pub mode: String,
    /// Manual mask (hex string), used only when mode == "manual".
    pub mask_raw: String,
}

/// Core topology summary from GetLogicalProcessorInformationEx.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreTopology {
    /// Mask of every logical processor the process may use.
    pub all_mask: u64,
    /// Mask of logical processors in the highest efficiency class.
    /// Equals `all_mask` when the CPU reports no efficiency split.
    pub performance_mask: u64,
    /// True when at least two distinct efficiency classes were reported.
    pub has_efficiency_split: bool,
}

/// Parse a hex bitmask like "0xFF0" / "ff0". Decimal is intentionally NOT
/// accepted — ambiguity between 0x10 and 10 is a footgun.
pub fn parse_affinity_mask(raw: &str) -> Option<u64> {
    let s = raw.trim().trim_start_matches("0x").trim_start_matches("0X");
    if s.is_empty() || !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    u64::from_str_radix(s, 16).ok().filter(|m| *m != 0)
}

/// Resolve the mask to apply for the given config + detected topology.
/// Returns None when nothing should be applied (off / invalid / no split).
pub fn resolve_target_mask(cfg: &AffinityConfig, topo: Option<&CoreTopology>) -> Option<u64> {
    match cfg.mode.as_str() {
        "performance" => {
            let topo = topo?;
            if topo.has_efficiency_split {
                Some(topo.performance_mask)
            } else {
                None
            }
        }
        "manual" => {
            let mask = parse_affinity_mask(&cfg.mask_raw)?;
            let all = topo.map(|t| t.all_mask).unwrap_or(u64::MAX);
            // Must stay inside what the process is allowed to use.
            (mask & !all == 0).then_some(mask)
        }
        _ => None,
    }
}

/// Detect core topology (Windows only; None elsewhere).
#[cfg(windows)]
pub fn detect_core_topology() -> Result<CoreTopology, String> {
    use windows::Win32::System::SystemInformation::{
        GetLogicalProcessorInformationEx, RelationProcessorCore,
        SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
    };

    unsafe {
        // First call with None returns the required buffer size in `len`.
        let mut len: u32 = 0;
        let _ = GetLogicalProcessorInformationEx(RelationProcessorCore, None, &mut len);
        if len == 0 {
            return Err("GetLogicalProcessorInformationEx returned size 0".into());
        }
        let mut buf = vec![0u8; len as usize];
        let info = buf.as_mut_ptr() as *mut SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX;
        if GetLogicalProcessorInformationEx(RelationProcessorCore, Some(info), &mut len).is_err() {
            return Err(format!(
                "GetLogicalProcessorInformationEx failed: {}",
                windows::core::Error::from_win32()
            ));
        }

        let mut all_mask: u64 = 0;
        let mut max_class: u8 = 0;
        // (group, mask, class) per physical core entry
        let mut classes: Vec<u8> = Vec::new();
        let mut offset = 0usize;
        while offset < len as usize {
            let entry =
                &*(buf.as_ptr().add(offset) as *const SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX);
            let size = entry.Size as usize;
            if size == 0 {
                break;
            }
            if entry.Relationship == RelationProcessorCore {
                let p = &entry.Anonymous.Processor;
                let class = p.EfficiencyClass;
                max_class = max_class.max(class);
                for i in 0..p.GroupCount as usize {
                    let gm = &p.GroupMask[i];
                    all_mask |= gm.Mask as u64;
                    classes.push(class);
                }
            }
            offset += size;
        }

        let has_split = classes.iter().any(|c| *c > 0);
        let performance_mask = if has_split {
            // Re-walk groups in lockstep with collected classes.
            let mut perf: u64 = 0;
            let mut idx = 0usize;
            offset = 0usize;
            while offset < len as usize {
                let entry = &*(buf.as_ptr().add(offset) as *const SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX);
                let size = entry.Size as usize;
                if size == 0 {
                    break;
                }
                if entry.Relationship == RelationProcessorCore {
                    let p = &entry.Anonymous.Processor;
                    for i in 0..p.GroupCount as usize {
                        let gm = &p.GroupMask[i];
                        if classes.get(idx) == Some(&max_class) {
                            perf |= gm.Mask as u64;
                        }
                        idx += 1;
                    }
                }
                offset += size;
            }
            perf
        } else {
            all_mask
        };
        if all_mask == 0 {
            return Err("no processor cores reported".into());
        }
        Ok(CoreTopology {
            all_mask,
            performance_mask,
            has_efficiency_split: has_split,
        })
    }
}

#[cfg(not(windows))]
pub fn detect_core_topology() -> Result<CoreTopology, String> {
    Err("CPU affinity is only supported on Windows".into())
}

/// Apply `mask` to the process `pid` (post-spawn). Windows only.
#[cfg(windows)]
pub fn apply_mask_to_pid(pid: u32, mask: u64) -> Result<(), String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        GetProcessAffinityMask, OpenProcess, SetProcessAffinityMask, PROCESS_QUERY_INFORMATION,
        PROCESS_SET_INFORMATION,
    };

    if mask == 0 {
        return Err("affinity mask is empty".into());
    }
    unsafe {
        let process = OpenProcess(
            PROCESS_SET_INFORMATION | PROCESS_QUERY_INFORMATION,
            false,
            pid,
        )
        .map_err(|e| format!("OpenProcess({pid}): {e}"))?;
        let result = (|| {
            let mut current: usize = 0;
            let mut system: usize = 0;
            if GetProcessAffinityMask(process, &mut current, &mut system).is_err() {
                return Err(format!(
                    "GetProcessAffinityMask({pid}): {}",
                    windows::core::Error::from_win32()
                ));
            }
            if mask & !(current as u64) != 0 {
                return Err(format!(
                    "mask 0x{mask:X} is outside the process affinity 0x{current:X}"
                ));
            }
            if SetProcessAffinityMask(process, mask as usize).is_err() {
                return Err(format!(
                    "SetProcessAffinityMask({pid}): {}",
                    windows::core::Error::from_win32()
                ));
            }
            Ok(())
        })();
        let _ = CloseHandle(process);
        result
    }
}

#[cfg(not(windows))]
pub fn apply_mask_to_pid(_pid: u32, _mask: u64) -> Result<(), String> {
    Err("CPU affinity is only supported on Windows".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(mode: &str, mask: &str) -> AffinityConfig {
        AffinityConfig {
            mode: mode.into(),
            mask_raw: mask.into(),
        }
    }

    #[test]
    fn parse_hex_mask() {
        assert_eq!(parse_affinity_mask("0xFF0"), Some(0xFF0));
        assert_eq!(parse_affinity_mask("ff0"), Some(0xFF0));
        assert_eq!(parse_affinity_mask(" 0x1 "), Some(1));
        assert_eq!(parse_affinity_mask(""), None);
        assert_eq!(parse_affinity_mask("xyz"), None);
        assert_eq!(parse_affinity_mask("0x0"), None);
    }

    #[test]
    fn off_mode_never_applies() {
        assert_eq!(resolve_target_mask(&cfg("off", ""), None), None);
        assert_eq!(resolve_target_mask(&cfg("off", "0xFF"), None), None);
    }

    #[test]
    fn performance_mode_needs_split() {
        let uniform = CoreTopology {
            all_mask: 0xFF,
            performance_mask: 0xFF,
            has_efficiency_split: false,
        };
        let hybrid = CoreTopology {
            all_mask: 0xFF,
            performance_mask: 0xF0,
            has_efficiency_split: true,
        };
        assert_eq!(resolve_target_mask(&cfg("performance", ""), Some(&uniform)), None);
        assert_eq!(resolve_target_mask(&cfg("performance", ""), Some(&hybrid)), Some(0xF0));
        assert_eq!(resolve_target_mask(&cfg("performance", ""), None), None);
    }

    #[test]
    fn manual_mode_validates_against_topology() {
        let topo = CoreTopology {
            all_mask: 0xFF,
            performance_mask: 0xF0,
            has_efficiency_split: true,
        };
        assert_eq!(resolve_target_mask(&cfg("manual", "0x0F"), Some(&topo)), Some(0x0F));
        // Outside the allowed set → rejected.
        assert_eq!(resolve_target_mask(&cfg("manual", "0xF00"), Some(&topo)), None);
        // No topology available → permissive (mask applied, OS clamps).
        assert_eq!(resolve_target_mask(&cfg("manual", "0xFF0"), None), Some(0xFF0));
    }

    #[cfg(windows)]
    #[test]
    fn topology_smoke() {
        let topo = detect_core_topology().expect("topology detection");
        assert!(topo.all_mask.count_ones() >= 1);
        assert!(topo.performance_mask & topo.all_mask == topo.performance_mask);
    }
}
