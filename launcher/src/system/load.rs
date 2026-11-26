//! How hard the processor and GPU work: their clocks and the CPU's busy share,
//! for the Battery page, since they decide much of the power draw.

/// What is known; each value is absent where the system does not report it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Load {
    /// The fastest CPU core's clock.
    pub cpu_mhz: Option<u32>,
    /// Share of time all CPU cores were busy since the previous reading, 0–100.
    pub cpu_busy: Option<u8>,
    pub gpu_mhz: Option<u32>,
}

/// Reads [`Load`]; the busy share needs two readings, so the first has none.
#[derive(Default)]
pub struct LoadMeter {
    /// Busy and total jiffies at the previous reading.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    last: Option<(u64, u64)>,
}

impl LoadMeter {
    pub fn read(&mut self, simulated: bool) -> Load {
        if simulated {
            return Load { cpu_mhz: Some(816), cpu_busy: Some(12), gpu_mhz: Some(200) };
        }
        #[cfg(target_os = "linux")]
        return self.read_linux();
        #[cfg(not(target_os = "linux"))]
        Load::default()
    }

    #[cfg(target_os = "linux")]
    fn read_linux(&mut self) -> Load {
        use std::fs;
        use std::path::Path;
        let number = |p: &Path| fs::read_to_string(p).ok().and_then(|s| s.trim().parse::<u64>().ok());
        let dirs = |d: &str| -> Vec<std::path::PathBuf> {
            fs::read_dir(d).map(|it| it.flatten().map(|e| e.path()).collect()).unwrap_or_default()
        };

        // One policy per cluster; scaling_cur_freq is in kHz.
        let cpu_mhz = dirs("/sys/devices/system/cpu/cpufreq")
            .iter()
            .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("policy")))
            .filter_map(|p| number(&p.join("scaling_cur_freq")))
            .max()
            .map(|khz| (khz / 1000) as u32);

        // A devfreq device named after the GPU, such as ff400000.gpu; cur_freq is in Hz.
        let gpu_mhz = dirs("/sys/class/devfreq")
            .iter()
            .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().contains("gpu")))
            .find_map(|p| number(&p.join("cur_freq")))
            .map(|hz| (hz / 1_000_000) as u32);

        // The first line of /proc/stat: cpu user nice system idle iowait irq softirq steal …
        let times: Option<(u64, u64)> = fs::read_to_string("/proc/stat").ok().and_then(|s| {
            let fields: Vec<u64> = s.lines().next()?.split_whitespace().skip(1).take(8).filter_map(|f| f.parse().ok()).collect();
            let total: u64 = fields.iter().sum();
            let idle = fields.get(3)? + fields.get(4).unwrap_or(&0);
            Some((total - idle, total))
        });
        let cpu_busy = match (self.last, times) {
            (Some((b0, t0)), Some((b1, t1))) if t1 > t0 => Some(((b1.saturating_sub(b0)) * 100 / (t1 - t0)).min(100) as u8),
            _ => None,
        };
        self.last = times;
        Load { cpu_mhz, cpu_busy, gpu_mhz }
    }
}
