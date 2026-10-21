//! Measures what an indexing run costs this machine: the CPU and the memory of Harness and of
//! the processes it started (the embedding, speech and OCR engines), sampled once a second on a
//! thread of its own.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Figures of one measured stretch.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Usage {
    /// Average and peak CPU use in percent of one core (all cores busy = cores x 100).
    pub cpu_avg_pct: f32,
    pub cpu_peak_pct: f32,
    /// Largest resident memory of the process tree.
    pub rss_peak_bytes: u64,
}

#[derive(Default)]
struct Acc {
    samples: u32,
    cpu_sum: f64,
    cpu_peak: f32,
    rss_peak: u64,
}

/// A running measurement; [`Meter::stop`] ends it.
pub struct Meter {
    stop: Arc<AtomicBool>,
    acc: Arc<Mutex<Acc>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

const INTERVAL: Duration = Duration::from_millis(1000);

/// Pids of `root` and of every process below it.
fn tree(parents: &HashMap<u32, u32>, root: u32) -> HashSet<u32> {
    let mut set = HashSet::from([root]);
    loop {
        let before = set.len();
        for (pid, parent) in parents {
            if set.contains(parent) {
                set.insert(*pid);
            }
        }
        if set.len() == before {
            return set;
        }
    }
}

impl Meter {
    pub fn start() -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let acc = Arc::new(Mutex::new(Acc::default()));
        let (s, a) = (stop.clone(), acc.clone());
        let thread = std::thread::Builder::new()
            .name("rag-usage".into())
            .spawn(move || {
                use sysinfo::{ProcessesToUpdate, System};
                let me = std::process::id();
                let mut sys = System::new();
                // The first refresh only sets the baseline CPU time.
                sys.refresh_processes(ProcessesToUpdate::All, true);
                let mut first = true;
                while !s.load(Ordering::SeqCst) {
                    // Sleep in short steps so a stop is quick.
                    let mut waited = Duration::ZERO;
                    while waited < INTERVAL && !s.load(Ordering::SeqCst) {
                        std::thread::sleep(Duration::from_millis(100));
                        waited += Duration::from_millis(100);
                    }
                    sys.refresh_processes(ProcessesToUpdate::All, true);
                    if first {
                        first = false;
                        continue;
                    }
                    let parents: HashMap<u32, u32> = sys
                        .processes()
                        .iter()
                        .filter_map(|(pid, p)| Some((pid.as_u32(), p.parent()?.as_u32())))
                        .collect();
                    let members = tree(&parents, me);
                    let (mut cpu, mut rss) = (0f32, 0u64);
                    for (pid, p) in sys.processes() {
                        if members.contains(&pid.as_u32()) {
                            cpu += p.cpu_usage();
                            rss += p.memory();
                        }
                    }
                    let mut acc = a.lock().unwrap_or_else(|e| e.into_inner());
                    acc.samples += 1;
                    acc.cpu_sum += cpu as f64;
                    acc.cpu_peak = acc.cpu_peak.max(cpu);
                    acc.rss_peak = acc.rss_peak.max(rss);
                }
            })
            .ok();
        Self { stop, acc, thread }
    }

    /// Ends the measurement and returns its figures (zeros when the run was shorter than a
    /// sampling step).
    pub fn stop(mut self) -> Usage {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        let acc = self.acc.lock().unwrap_or_else(|e| e.into_inner());
        Usage {
            cpu_avg_pct: if acc.samples > 0 {
                (acc.cpu_sum / acc.samples as f64) as f32
            } else {
                0.0
            },
            cpu_peak_pct: acc.cpu_peak,
            rss_peak_bytes: acc.rss_peak,
        }
    }
}

impl Drop for Meter {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tree_holds_a_process_and_its_descendants() {
        let parents = HashMap::from([(2, 1), (3, 2), (4, 9), (5, 3)]);
        let t = tree(&parents, 1);
        assert_eq!(t, HashSet::from([1, 2, 3, 5]));
    }

    #[test]
    fn a_short_measurement_reports_the_memory_floor_of_zero_samples() {
        let m = Meter::start();
        let u = m.stop();
        assert_eq!(u.cpu_avg_pct, 0.0);
    }
}
