//! Pack RAM advice from a test-run peak. Keep in sync with
//! `apps/tuffbox-desktop/src/lib/testLoad.ts`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MachineFit {
    Comfortable,
    Tight,
    Overloaded,
}

impl MachineFit {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Comfortable => "comfortable",
            Self::Tight => "tight",
            Self::Overloaded => "overloaded",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RamAdvice {
    pub recommended_gb: u32,
    pub machine: MachineFit,
    pub peak_host_pct: f32,
}

const RAM_STEPS_GB: [u32; 5] = [8, 12, 16, 24, 32];
const OS_HEADROOM_MB: u64 = 4096;

pub fn recommend_ram(
    peak_rss_mb: u64,
    xmx_mb: u64,
    peak_host_mb: u64,
    host_total_mb: u64,
) -> RamAdvice {
    let needed_mb = peak_rss_mb.max(xmx_mb).saturating_add(OS_HEADROOM_MB);
    let recommended_gb = RAM_STEPS_GB
        .iter()
        .copied()
        .find(|gb| u64::from(*gb) * 1024 >= needed_mb)
        .unwrap_or(32);

    let peak_host_pct = if host_total_mb == 0 {
        0.0
    } else {
        (peak_host_mb as f64 / host_total_mb as f64 * 100.0) as f32
    };

    let mut machine = if peak_host_pct >= 90.0 {
        MachineFit::Overloaded
    } else if peak_host_pct >= 70.0 {
        MachineFit::Tight
    } else {
        MachineFit::Comfortable
    };

    if host_total_mb > 0 && u64::from(recommended_gb) * 1024 > host_total_mb {
        if machine == MachineFit::Comfortable {
            machine = MachineFit::Tight;
        }
    }

    RamAdvice {
        recommended_gb,
        machine,
        peak_host_pct,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fit(
        peak_rss: u64,
        xmx: u64,
        peak_host: u64,
        host_total: u64,
        rec_gb: u32,
        machine: MachineFit,
    ) {
        let got = recommend_ram(peak_rss, xmx, peak_host, host_total);
        assert_eq!(got.recommended_gb, rec_gb, "rec_gb");
        assert_eq!(got.machine, machine, "machine");
    }

    #[test]
    fn recommend_ram_table() {
        fit(2000, 4096, 8000, 16384, 8, MachineFit::Comfortable);
        fit(6200, 4096, 14000, 16384, 12, MachineFit::Tight);
        fit(8000, 8192, 15000, 16384, 12, MachineFit::Overloaded);
        fit(500, 2048, 3000, 8192, 8, MachineFit::Comfortable);
        fit(20000, 16384, 30000, 32768, 24, MachineFit::Overloaded);
        fit(1000, 4096, 0, 0, 8, MachineFit::Comfortable);
    }

    #[test]
    fn recommend_bumps_comfortable_to_tight_when_pc_is_below_rec() {
        // 8 GB machine, advice is 16 GB → at least Tight.
        let got = recommend_ram(9000, 8192, 4000, 8192);
        assert_eq!(got.recommended_gb, 16);
        assert_eq!(got.machine, MachineFit::Tight);
    }
}
