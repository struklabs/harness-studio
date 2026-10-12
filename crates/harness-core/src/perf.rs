//! On-demand resource sampling for the Performance view: CPU, process memory, GPU load.
//!
//! Nothing here runs in the background; the frontend asks once per second while the
//! Performance tab is visible, so the app costs nothing when the tab is closed.

use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::Instant;
use sysinfo::{CpuRefreshKind, Pid, ProcessRefreshKind, ProcessesToUpdate, RefreshKind, System};

/// `nvidia-smi` costs a process spawn, so GPU readings are reused for a few seconds.
const GPU_LIVE_TTL_SECS: u64 = 3;

/// Memory and CPU of a process tree (the process plus everything it spawned).
#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct ProcessStats {
    pub rss_bytes: u64,
    /// Share of the whole machine (0-100); `None` until a second sample exists.
    pub cpu_percent: Option<f64>,
    pub uptime_secs: u64,
    pub process_count: usize,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct GpuLive {
    pub utilization_percent: Option<f64>,
    pub temperature_c: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct PerfDetails {
    /// Whole-machine CPU load; `None` until a second sample exists.
    pub system_cpu_percent: Option<f64>,
    pub cpu_cores: usize,
    /// Harness Studio itself: the app process only.
    pub app: ProcessStats,
    /// The WebView2 processes the app spawned (UI rendering).
    pub app_webview: ProcessStats,
    /// The selected llama-server (with helper processes); `None` when it isn't running.
    pub server: Option<ProcessStats>,
    /// `None` when the GPU exposes no load/temperature counters we can read cheaply.
    pub gpu: Option<GpuLive>,
}

struct Inner {
    sys: System,
    primed: bool,
    gpu_cache: Option<(Instant, Option<GpuLive>)>,
}

pub struct PerfSampler {
    inner: Mutex<Inner>,
}

impl Default for PerfSampler {
    fn default() -> Self {
        let sys = System::new_with_specifics(
            RefreshKind::nothing().with_cpu(CpuRefreshKind::nothing().with_cpu_usage()),
        );
        Self {
            inner: Mutex::new(Inner {
                sys,
                primed: false,
                gpu_cache: None,
            }),
        }
    }
}

/// `root` and all its descendants, given a `pid -> parent` map.
pub fn process_tree(root: u32, parents: &HashMap<u32, u32>) -> HashSet<u32> {
    let mut tree = HashSet::from([root]);
    loop {
        let before = tree.len();
        for (pid, parent) in parents {
            if tree.contains(parent) {
                tree.insert(*pid);
            }
        }
        if tree.len() == before {
            return tree;
        }
    }
}

impl PerfSampler {
    /// Samples the machine, this app, and (when given) the llama-server process tree.
    pub fn sample(
        &self,
        server_pid: Option<u32>,
        gpu: Option<&crate::gpu::GpuAdapter>,
    ) -> PerfDetails {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.sys.refresh_cpu_usage();
        inner.sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_cpu().with_memory(),
        );
        let primed = std::mem::replace(&mut inner.primed, true);
        let cores = inner.sys.cpus().len().max(1);

        let parents: HashMap<u32, u32> = inner
            .sys
            .processes()
            .iter()
            .filter_map(|(pid, p)| Some((pid.as_u32(), p.parent()?.as_u32())))
            .collect();

        let stats_of = |ids: &HashSet<u32>| -> ProcessStats {
            let mut stats = ProcessStats::default();
            let mut cpu = 0.0f64;
            let mut oldest: Option<u64> = None;
            for id in ids {
                if let Some(p) = inner.sys.process(Pid::from_u32(*id)) {
                    stats.rss_bytes += p.memory();
                    stats.process_count += 1;
                    cpu += p.cpu_usage() as f64;
                    oldest = Some(oldest.map_or(p.start_time(), |o| o.min(p.start_time())));
                }
            }
            stats.cpu_percent = primed.then(|| (cpu / cores as f64).clamp(0.0, 100.0));
            stats.uptime_secs = oldest.map_or(0, uptime_since);
            stats
        };

        let own = std::process::id();
        let mut webview_ids = process_tree(own, &parents);
        webview_ids.remove(&own);
        let details = PerfDetails {
            system_cpu_percent: primed.then(|| inner.sys.global_cpu_usage() as f64),
            cpu_cores: cores,
            app: stats_of(&HashSet::from([own])),
            app_webview: stats_of(&webview_ids),
            server: server_pid
                .map(|pid| stats_of(&process_tree(pid, &parents)))
                .filter(|s| s.process_count > 0),
            gpu: None,
        };

        let stale = inner
            .gpu_cache
            .as_ref()
            .is_none_or(|(at, _)| at.elapsed().as_secs() >= GPU_LIVE_TTL_SECS);
        if stale {
            let live = gpu.and_then(query_gpu_live);
            inner.gpu_cache = Some((Instant::now(), live));
        }
        PerfDetails {
            gpu: inner.gpu_cache.as_ref().and_then(|(_, g)| g.clone()),
            ..details
        }
    }
}

fn uptime_since(start_secs: u64) -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().saturating_sub(start_secs))
        .unwrap_or(0)
}

/// GPU load and temperature: `nvidia-smi` for NVIDIA, DRM sysfs/hwmon on Linux for AMD
/// (`gpu_busy_percent` + hwmon) and Intel (hwmon temperature only; i915/xe expose no cheap
/// load counter). Other cases report nothing rather than made-up values.
fn query_gpu_live(adapter: &crate::gpu::GpuAdapter) -> Option<GpuLive> {
    if adapter.vendor_id != Some(crate::gpu::VENDOR_NVIDIA) {
        let (utilization_percent, temperature_c) = crate::gpu::sysfs_gpu_live(adapter.sysfs_dir()?);
        let live = GpuLive {
            utilization_percent,
            temperature_c,
        };
        return (live.utilization_percent.is_some() || live.temperature_c.is_some())
            .then_some(live);
    }
    let out = crate::process::quiet_command("nvidia-smi")
        .args([
            "--query-gpu=utilization.gpu,temperature.gpu",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_nvidia_smi_live(&String::from_utf8_lossy(&out.stdout))
}

/// Parses `utilization.gpu,temperature.gpu` (percent, Celsius) of the first GPU;
/// `[N/A]` fields become `None`, and a line with no usable number yields nothing.
pub fn parse_nvidia_smi_live(text: &str) -> Option<GpuLive> {
    let line = text.lines().find(|l| !l.trim().is_empty())?;
    let mut parts = line.split(',').map(|p| p.trim().parse::<f64>().ok());
    let live = GpuLive {
        utilization_percent: parts.next()?,
        temperature_c: parts.next()?,
    };
    (live.utilization_percent.is_some() || live.temperature_c.is_some()).then_some(live)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_tree_collects_descendants() {
        let parents = HashMap::from([(2, 1), (3, 2), (4, 3), (9, 8), (10, 1)]);
        let tree = process_tree(1, &parents);
        assert_eq!(tree, HashSet::from([1, 2, 3, 4, 10]));
        assert_eq!(process_tree(7, &parents), HashSet::from([7]));
    }

    #[test]
    fn test_parse_nvidia_smi_live() {
        assert_eq!(
            parse_nvidia_smi_live("37, 61\n"),
            Some(GpuLive {
                utilization_percent: Some(37.0),
                temperature_c: Some(61.0)
            })
        );
        let partial = parse_nvidia_smi_live("12, [N/A]").unwrap();
        assert_eq!(partial.utilization_percent, Some(12.0));
        assert_eq!(partial.temperature_c, None);
        assert_eq!(parse_nvidia_smi_live("[N/A], [N/A]"), None);
        assert_eq!(parse_nvidia_smi_live(""), None);
    }

    #[test]
    fn test_sample_reports_own_process() {
        let sampler = PerfSampler::default();
        let first = sampler.sample(None, None);
        assert!(first.app.rss_bytes > 0);
        assert_eq!(first.app.cpu_percent, None, "needs a second sample");
        assert!(first.server.is_none());
        assert!(first.gpu.is_none());
        let second = sampler.sample(None, None);
        assert!(second.app.cpu_percent.is_some());
        assert!(second.cpu_cores >= 1);
    }
}
