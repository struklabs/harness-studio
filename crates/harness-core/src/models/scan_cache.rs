use super::*;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};

struct Snapshot {
    dirs: Vec<PathBuf>,
    generation: u64,
    completed: Instant,
    models: Arc<Vec<LocalModel>>,
}

/// The lock is held through discovery: concurrent page commands join one scan.
/// Invalidation uses an atomic so a mutator never waits for a scan's disk I/O.
#[derive(Default)]
pub(super) struct ScanCache {
    generation: AtomicU64,
    snapshot: Mutex<Option<Snapshot>>,
}

impl ScanCache {
    pub fn invalidate(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    /// Rescans at most `MAX_RESCANS` times when a mutation lands during discovery. After
    /// that the newest result is returned without being published, so a long-running
    /// import cannot keep waiters on the lock; the next call simply scans again.
    pub fn get(
        &self,
        dirs: &[PathBuf],
        mut scan: impl FnMut() -> Vec<LocalModel>,
    ) -> Arc<Vec<LocalModel>> {
        const MAX_RESCANS: u32 = 2;
        let mut cache = self.snapshot.lock().unwrap_or_else(|e| e.into_inner());
        let mut rescans = 0;
        loop {
            let generation = self.generation.load(Ordering::SeqCst);
            if let Some(hit) = cache.as_ref().filter(|s| {
                s.dirs == dirs
                    && s.generation == generation
                    && s.completed.elapsed() < Duration::from_secs(1)
            }) {
                return hit.models.clone();
            }
            let models = Arc::new(scan());
            // A completed mutation during discovery must not publish an older result.
            if self.generation.load(Ordering::SeqCst) != generation {
                if rescans >= MAX_RESCANS {
                    return models;
                }
                rescans += 1;
                continue;
            }
            *cache = Some(Snapshot {
                dirs: dirs.to_vec(),
                generation,
                completed: Instant::now(),
                models: models.clone(),
            });
            return models;
        }
    }
}

static CACHE: std::sync::LazyLock<ScanCache> = std::sync::LazyLock::new(ScanCache::default);

pub(super) fn scan(dirs: &[PathBuf]) -> Arc<Vec<LocalModel>> {
    CACHE.get(dirs, || {
        let mut models = ModelManager.scan_directories(dirs);
        models.extend(ModelManager::external_models(&models));
        models.sort_by(|a, b| a.name.cmp(&b.name));
        models
    })
}

/// Invalidates all shared library discovery snapshots. Unobserved filesystem changes (for
/// example another application's downloads) expire within one second, without a watcher.
pub fn invalidate_scan() {
    CACHE.invalidate();
}

/// Invalidate before and after a filesystem mutation, including partially failed operations.
pub(crate) struct LibraryMutation;
impl LibraryMutation {
    pub fn begin() -> Self {
        invalidate_scan();
        Self
    }
}
impl Drop for LibraryMutation {
    fn drop(&mut self) {
        invalidate_scan();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};

    #[test]
    fn concurrent_calls_share_one_scan() {
        let cache = ScanCache::default();
        let count = AtomicU64::new(0);
        let start = Arc::new(Barrier::new(4));
        std::thread::scope(|scope| {
            for _ in 0..4 {
                let start = start.clone();
                let cache = &cache;
                let count = &count;
                scope.spawn(move || {
                    start.wait();
                    cache.get(&[], || {
                        count.fetch_add(1, Ordering::SeqCst);
                        Vec::new()
                    });
                });
            }
        });
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn expiry_and_invalidation_rescan_including_a_racing_mutation() {
        let cache = ScanCache::default();
        let mut calls = 0;
        cache.get(&[], || {
            calls += 1;
            Vec::new()
        });
        cache.get(&[], || panic!("should reuse snapshot"));
        cache.snapshot.lock().unwrap().as_mut().unwrap().completed =
            Instant::now() - Duration::from_secs(2);
        cache.get(&[], || {
            calls += 1;
            Vec::new()
        });
        assert_eq!(calls, 2);
        cache.invalidate();
        cache.get(&[], || {
            calls += 1;
            if calls == 3 {
                cache.invalidate();
            }
            Vec::new()
        });
        assert_eq!(calls, 4, "discard a result invalidated during discovery");
        cache.get(&[PathBuf::from("added")], || {
            calls += 1;
            Vec::new()
        });
        assert_eq!(calls, 5, "directory list is part of the key");
    }

    #[test]
    fn continuous_invalidation_is_bounded_and_unpublished() {
        let cache = ScanCache::default();
        let mut scans = 0;
        let models = cache.get(&[], || {
            scans += 1;
            cache.invalidate();
            Vec::new()
        });
        assert!(models.is_empty());
        assert_eq!(scans, 3, "one scan plus two retries");
        assert!(
            cache.snapshot.lock().unwrap().is_none(),
            "nothing published"
        );
        let mut after = 0;
        cache.get(&[], || {
            after += 1;
            Vec::new()
        });
        assert_eq!(after, 1);
        assert!(cache.snapshot.lock().unwrap().is_some());
    }

    #[test]
    fn panicking_scan_does_not_poison_the_cache() {
        let cache = ScanCache::default();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            cache.get(&[], || panic!("scan failed"));
        }));
        assert!(result.is_err());
        assert!(cache.snapshot.lock().is_err(), "the mutex was poisoned");
        let mut calls = 0;
        cache.get(&[], || {
            calls += 1;
            Vec::new()
        });
        assert_eq!(calls, 1, "next get recovers and scans");
        cache.get(&[], || panic!("should reuse snapshot"));
    }
}
