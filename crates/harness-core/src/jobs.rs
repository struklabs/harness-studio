//! Process-wide GPU admission. Existing gateway (including audio) slot and token
//! limits remain in charge of requests; this queue owns process memory lifetimes.
use serde::Serialize;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::{watch, Notify};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", schema(as = GpuJobPriority))]
pub enum Priority {
    Interactive,
    Agent,
    Background,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", schema(as = GpuJobState))]
pub enum Status {
    Queued,
    Loading,
    Running,
    Cancelling,
}

/// Why a queued job has not started yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", schema(as = GpuJobWaitReason))]
pub enum WaitReason {
    /// A job of higher priority, or one that came first, goes before it.
    Queue,
    /// Its memory need is above what is free now.
    Memory,
    /// It runs alone (unknown footprint, or above the whole budget) and others still hold
    /// memory.
    Alone,
}

/// What a queued job waits for, with the numbers of the last admission check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "openapi", schema(as = GpuJobWait))]
pub struct JobWait {
    pub reason: WaitReason,
    /// GPU (or unified) memory free for it now, after the reserve and loads in progress.
    pub free_bytes: u64,
    /// The budget for managed engines, after the reserve.
    pub capacity_bytes: u64,
}

/// Free memory and the budget as admission sees them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Memory {
    pub free: u64,
    pub capacity: u64,
    /// Unified memory (Apple silicon, integrated GPUs): the OS can page a model that only
    /// just fits, where a discrete GPU would fail to allocate.
    pub unified: bool,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct JobStatus {
    pub id: u64,
    pub label: String,
    pub priority: Priority,
    pub status: Status,
    /// None means the GPU footprint is unknown; admission waits.
    pub memory_bytes: Option<u64>,
    pub queue_wait_ms: u64,
    /// Time to engine readiness; one-shot binaries use their first output as the
    /// observable startup boundary when they have no readiness protocol.
    pub load_ms: Option<u64>,
    /// Why it is still queued; absent once it has started (or before the first check).
    pub wait: Option<JobWait>,
}

struct Entry {
    view: JobStatus,
    arrived: Instant,
    admitted: Option<Instant>,
    cancel: watch::Sender<bool>,
}

#[derive(Default)]
struct State {
    next: u64,
    entries: Vec<Entry>,
}

#[derive(Default)]
pub struct JobScheduler {
    state: Mutex<State>,
    changed: Notify,
}

pub fn global() -> &'static Arc<JobScheduler> {
    static SCHEDULER: OnceLock<Arc<JobScheduler>> = OnceLock::new();
    SCHEDULER.get_or_init(|| Arc::new(JobScheduler::default()))
}

tokio::task_local! { pub static JOB_PRIORITY: Priority; }

pub fn current_priority() -> Priority {
    JOB_PRIORITY
        .try_with(|priority| *priority)
        .unwrap_or(Priority::Interactive)
}

/// Use the engine's estimate, including weights, KV/context and runtime overhead.
pub fn estimate_launch(config: &crate::process::LaunchConfig) -> u64 {
    let engine = crate::engine::server_for(config);
    let hw = crate::hardware::detect_hardware();
    if engine.family() != crate::engine::LLAMA_CPP {
        if crate::vram::budget(&hw).vram_bytes == 0
            || crate::engine::non_llama_runs_on_cpu(config)
            || (config.gpu_layers == 0 && engine.family() == crate::engine::MISTRALRS)
            || (engine.family() == crate::engine::PYTHON_HF
                && crate::engine::python_hf::gpu_device(config).is_none())
        {
            return 0;
        }
        let need = crate::engine::non_llama_vram_need(config);
        // An unknown GPU footprint must wait, never become a zero-byte reservation.
        return if need > 0 { need } else { u64::MAX };
    }
    if config.router_mode && config.gpu_layers > 0 {
        // Router children choose models after startup, so their footprint cannot
        // safely be represented by the parent process's empty model path.
        return u64::MAX;
    }
    let input = crate::vram::complete_input(crate::vram::input_from_launch(config));
    let budget = crate::vram::budget_for_model(&hw, input.model_path.as_deref());
    let meta = crate::gguf::read_gguf_metadata(&config.model_path).ok();
    let measured = input
        .model_path
        .as_deref()
        .map(crate::vram::measurements_for)
        .unwrap_or_default();
    crate::engine::server_for(config)
        .estimate_memory(&input, meta.as_ref(), &budget, &measured)
        .estimate
        .vram_bytes
}

/// Unknown free memory is deliberately not treated as unlimited GPU memory.
pub fn available_memory() -> Memory {
    let hw = crate::hardware::detect_hardware();
    let budget = crate::vram::budget(&hw);
    let capacity = budget.vram_bytes;
    let free = crate::vram::os_available_vram(&hw)
        .map(|(free, _)| free)
        .unwrap_or_else(|| {
            if budget.unified {
                hw.available_ram_bytes.min(capacity)
            } else {
                // No driver probe (integrated or unsupported adapter): the managed
                // reservations still bound the total, but a missing probe must not
                // block every GPU start forever.
                capacity
            }
        });
    let reserve = crate::vram::fixed_footprint_headroom(capacity, 0);
    Memory {
        free: free.saturating_sub(reserve),
        capacity: capacity.saturating_sub(reserve),
        unified: budget.unified,
    }
}

impl JobScheduler {
    pub fn submit(self: &Arc<Self>, label: String, priority: Priority, bytes: u64) -> Ticket {
        let mut state = self.state.lock().unwrap();
        state.next += 1;
        let id = state.next;
        let (cancel, receiver) = watch::channel(false);
        state.entries.push(Entry {
            view: JobStatus {
                id,
                label,
                priority,
                status: Status::Queued,
                memory_bytes: (bytes != u64::MAX).then_some(bytes),
                queue_wait_ms: 0,
                load_ms: None,
                wait: None,
            },
            arrived: Instant::now(),
            admitted: None,
            cancel,
        });
        drop(state);
        self.changed.notify_waiters();
        Ticket {
            scheduler: self.clone(),
            id,
            cancel: receiver,
        }
    }

    pub fn snapshot(&self) -> Vec<JobStatus> {
        let state = self.state.lock().unwrap();
        let mut jobs: Vec<_> = state
            .entries
            .iter()
            .map(|entry| {
                let mut view = entry.view.clone();
                if entry.admitted.is_none() {
                    view.queue_wait_ms = millis(entry.arrived.elapsed());
                }
                view
            })
            .collect();
        jobs.sort_by_key(|job| (job.priority, job.id));
        jobs
    }

    /// Running reservations are released only by their owner after the child exits.
    pub fn cancel(&self, id: u64) -> bool {
        let mut state = self.state.lock().unwrap();
        let Some(entry) = state.entries.iter_mut().find(|entry| entry.view.id == id) else {
            return false;
        };
        entry.cancel.send_replace(true);
        entry.view.status = Status::Cancelling;
        drop(state);
        self.changed.notify_waiters();
        true
    }

    fn admit(&self, id: u64, memory: Memory) -> bool {
        let Memory {
            free,
            capacity,
            unified,
        } = memory;
        let mut state = self.state.lock().unwrap();
        let cpu = state.entries.iter().any(|entry| {
            entry.view.id == id
                && entry.view.status == Status::Queued
                && entry.view.memory_bytes == Some(0)
        });
        let first = state
            .entries
            .iter()
            .filter(|entry| entry.view.status == Status::Queued)
            .min_by_key(|entry| (entry.view.priority, entry.view.id))
            .map(|entry| entry.view.id);
        // The CPU-only probe admits CPU work; a GPU job's wait is recorded with real numbers.
        if memory == NONE && !cpu {
            return false;
        }
        if first != Some(id) && !cpu {
            Self::waits(&mut state, id, WaitReason::Queue, free, capacity);
            return false;
        }
        // Driver free memory accounts for ready processes; pending loads may not
        // be reflected yet. Also cap the sum of all managed reservations.
        let reserved = state
            .entries
            .iter()
            .filter(|e| e.admitted.is_some())
            .fold(0u64, |sum, e| {
                sum.saturating_add(e.view.memory_bytes.unwrap_or(u64::MAX))
            });
        let pending = state
            .entries
            .iter()
            .filter(|e| e.admitted.is_some() && e.view.load_ms.is_none())
            .fold(0u64, |sum, e| {
                sum.saturating_add(e.view.memory_bytes.unwrap_or(u64::MAX))
            });
        let room = free
            .saturating_sub(pending)
            .min(capacity.saturating_sub(reserved));
        let refused = match state
            .entries
            .iter()
            .find(|entry| entry.view.id == id)
            .unwrap()
            .view
            .memory_bytes
        {
            // An unknown footprint can never fit a byte budget, so it would wait
            // forever; it runs alone, once no other GPU reservation is held.
            None if reserved > 0 => Some(WaitReason::Alone),
            None => None,
            // Neither can one larger than the whole budget (sd.cpp's partial-offload estimate
            // on unified memory is the full budget, above the headroom-reduced capacity): it
            // runs alone too, like the engine's own fit check already allowed.
            Some(need) if capacity > 0 && need > capacity => {
                (reserved > 0).then_some(WaitReason::Alone)
            }
            // On unified memory a load that fits the budget runs once it is alone, even when
            // other apps keep free memory below its need: the OS pages them out, where waiting
            // for free memory that never comes would hold it forever.
            Some(need) if unified && reserved == 0 && need <= capacity => None,
            Some(need) if need > room => Some(WaitReason::Memory),
            Some(_) => None,
        };
        if let Some(reason) = refused {
            Self::waits(&mut state, id, reason, room, capacity);
            return false;
        }
        let entry = state
            .entries
            .iter_mut()
            .find(|entry| entry.view.id == id)
            .unwrap();
        entry.view.wait = None;
        entry.admitted = Some(Instant::now());
        entry.view.status = Status::Loading;
        entry.view.queue_wait_ms = millis(entry.arrived.elapsed());
        eprintln!(
            "[jobs] id={id} queue_wait_ms={} memory_bytes={}",
            entry.view.queue_wait_ms,
            entry.view.memory_bytes.unwrap_or(u64::MAX)
        );
        true
    }

    /// Records why `id` is still queued.
    fn waits(state: &mut State, id: u64, reason: WaitReason, free: u64, capacity: u64) {
        if let Some(entry) = state.entries.iter_mut().find(|entry| entry.view.id == id) {
            entry.view.wait = Some(JobWait {
                reason,
                free_bytes: free,
                capacity_bytes: capacity,
            });
        }
    }
}

fn millis(duration: Duration) -> u64 {
    duration.as_millis().min(u64::MAX as u128) as u64
}

/// No memory: only CPU work (a zero footprint) is admitted.
const NONE: Memory = Memory {
    free: 0,
    capacity: 0,
    unified: false,
};

/// Dropping an unadmitted ticket removes it too (timeout or abandoned request).
pub struct Ticket {
    scheduler: Arc<JobScheduler>,
    id: u64,
    cancel: watch::Receiver<bool>,
}

impl Ticket {
    pub fn request_cancel(&self) {
        self.scheduler.cancel(self.id);
    }
    pub fn is_cancelled(&self) -> bool {
        *self.cancel.borrow()
    }
    pub fn cancellation(&self) -> watch::Receiver<bool> {
        self.cancel.clone()
    }

    /// Wait is the safe fallback: never silently change an engine's backend.
    pub async fn wait(&self) -> Result<(), String> {
        loop {
            let changed = self.scheduler.changed.notified();
            if self.is_cancelled() {
                return Err("jobs.cancelled".into());
            }
            // CPU work needs no GPU probe and is independent of blocked GPU loads.
            if self.scheduler.admit(self.id, NONE) {
                return Ok(());
            }
            let memory = tokio::task::spawn_blocking(available_memory)
                .await
                .unwrap_or(NONE);
            // No GPU detected: nothing to protect, so only the queue order applies.
            let memory = if memory.capacity == 0 {
                Memory {
                    free: u64::MAX,
                    capacity: u64::MAX,
                    unified: false,
                }
            } else {
                memory
            };
            if self.scheduler.admit(self.id, memory) {
                return Ok(());
            }
            tokio::select! {
                _ = changed => {},
                _ = tokio::time::sleep(Duration::from_millis(500)) => {},
            }
        }
    }

    pub fn loaded(&self) {
        let mut state = self.scheduler.state.lock().unwrap();
        if let Some(entry) = state
            .entries
            .iter_mut()
            .find(|entry| entry.view.id == self.id)
        {
            if entry.view.status == Status::Loading {
                entry.view.status = Status::Running;
                entry.view.load_ms = entry.admitted.map(|start| millis(start.elapsed()));
                eprintln!(
                    "[jobs] id={} load_ms={}",
                    self.id,
                    entry.view.load_ms.unwrap_or(0)
                );
            }
        }
    }
}

impl Drop for Ticket {
    fn drop(&mut self) {
        self.scheduler
            .state
            .lock()
            .unwrap()
            .entries
            .retain(|entry| entry.view.id != self.id);
        self.scheduler.changed.notify_waiters();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A discrete GPU with `free` of `capacity` bytes free.
    fn discrete(free: u64, capacity: u64) -> Memory {
        Memory {
            free,
            capacity,
            unified: false,
        }
    }

    fn unified(free: u64, capacity: u64) -> Memory {
        Memory {
            free,
            capacity,
            unified: true,
        }
    }

    #[test]
    fn priority_then_fifo() {
        let scheduler = Arc::new(JobScheduler::default());
        let batch = scheduler.submit("batch".into(), Priority::Background, 10);
        let agent = scheduler.submit("agent".into(), Priority::Agent, 10);
        let chat = scheduler.submit("chat".into(), Priority::Interactive, 10);
        let chat2 = scheduler.submit("chat2".into(), Priority::Interactive, 10);
        assert!(!scheduler.admit(batch.id, discrete(100, 100)));
        assert!(!scheduler.admit(agent.id, discrete(100, 100)));
        assert!(!scheduler.admit(chat2.id, discrete(100, 100)));
        assert!(scheduler.admit(chat.id, discrete(100, 100)));
        assert!(scheduler.admit(chat2.id, discrete(100, 100)));
        assert!(scheduler.admit(agent.id, discrete(100, 100)));
        assert!(scheduler.admit(batch.id, discrete(100, 100)));
    }

    #[test]
    fn reservation_wait_fallback_and_release() {
        let scheduler = Arc::new(JobScheduler::default());
        let first = scheduler.submit("image".into(), Priority::Agent, 80);
        let next = scheduler.submit("speech".into(), Priority::Agent, 30);
        assert!(scheduler.admit(first.id, discrete(100, 100)));
        assert!(!scheduler.admit(next.id, discrete(100, 100)));
        assert_eq!(scheduler.snapshot()[1].status, Status::Queued);
        first.loaded();
        assert!(scheduler.snapshot()[0].load_ms.is_some());
        drop(first);
        assert!(scheduler.admit(next.id, discrete(100, 100)));
    }

    #[test]
    fn cancel_queued_and_running_holds_reservation_until_exit() {
        let scheduler = Arc::new(JobScheduler::default());
        let running = scheduler.submit("training".into(), Priority::Background, 80);
        assert!(scheduler.admit(running.id, discrete(100, 100)));
        let queued = scheduler.submit("chat".into(), Priority::Interactive, 30);
        assert!(scheduler.cancel(running.id));
        assert!(running.is_cancelled());
        assert!(!scheduler.admit(queued.id, discrete(100, 100)));
        assert!(scheduler.cancel(queued.id));
        assert!(!scheduler.admit(queued.id, discrete(100, 100)));
        drop((running, queued));
        assert!(scheduler.snapshot().is_empty());
        assert!(!scheduler.cancel(999));
    }

    #[test]
    fn ready_memory_is_not_counted_twice_and_external_usage_is_respected() {
        let scheduler = Arc::new(JobScheduler::default());
        let resident = scheduler.submit("chat".into(), Priority::Interactive, 60);
        assert!(scheduler.admit(resident.id, discrete(100, 100)));
        resident.loaded();
        let next = scheduler.submit("speech".into(), Priority::Agent, 30);
        assert!(!scheduler.admit(next.id, discrete(20, 100)));
        assert!(scheduler.admit(next.id, discrete(40, 100)));
    }

    #[test]
    fn cpu_work_can_run_while_gpu_work_waits() {
        let scheduler = Arc::new(JobScheduler::default());
        let gpu = scheduler.submit("training".into(), Priority::Interactive, 50);
        let cpu = scheduler.submit("index".into(), Priority::Background, 0);
        assert!(!scheduler.admit(gpu.id, discrete(0, 0)));
        assert!(scheduler.admit(cpu.id, discrete(0, 0)));
    }

    #[test]
    fn unknown_footprint_runs_alone_instead_of_waiting_forever() {
        let scheduler = Arc::new(JobScheduler::default());
        let resident = scheduler.submit("chat".into(), Priority::Interactive, 40);
        let unknown = scheduler.submit("router".into(), Priority::Agent, u64::MAX);
        let after = scheduler.submit("speech".into(), Priority::Agent, 10);
        assert!(scheduler.admit(resident.id, discrete(100, 100)));
        assert!(!scheduler.admit(unknown.id, discrete(100, 100)));
        drop(resident);
        assert!(scheduler.admit(unknown.id, discrete(100, 100)));
        // While it runs it holds the whole GPU.
        assert!(!scheduler.admit(after.id, discrete(100, 100)));
        drop(unknown);
        assert!(scheduler.admit(after.id, discrete(100, 100)));
    }

    #[test]
    fn footprint_above_the_whole_budget_runs_alone_instead_of_waiting_forever() {
        let scheduler = Arc::new(JobScheduler::default());
        let resident = scheduler.submit("chat".into(), Priority::Interactive, 40);
        let video = scheduler.submit("video".into(), Priority::Interactive, 120);
        let after = scheduler.submit("speech".into(), Priority::Agent, 10);
        assert!(scheduler.admit(resident.id, discrete(100, 100)));
        assert!(!scheduler.admit(video.id, discrete(100, 100)));
        drop(resident);
        // Free memory below its estimate does not hold it back once it is alone.
        assert!(scheduler.admit(video.id, discrete(30, 100)));
        assert!(!scheduler.admit(after.id, discrete(100, 100)));
        drop(video);
        assert!(scheduler.admit(after.id, discrete(100, 100)));
    }

    #[test]
    fn alone_on_unified_memory_a_load_that_fits_the_budget_runs_despite_low_free_memory() {
        let scheduler = Arc::new(JobScheduler::default());
        // 8 GB Mac: a 4.3 GiB image model, 3.9 GiB free (other apps), a 5.3 GiB budget.
        let image = scheduler.submit("image".into(), Priority::Interactive, 43);
        assert!(
            !scheduler.admit(image.id, discrete(39, 53)),
            "a discrete GPU still waits"
        );
        let wait = scheduler.snapshot()[0].wait.unwrap();
        assert_eq!(wait.reason, WaitReason::Memory);
        assert_eq!((wait.free_bytes, wait.capacity_bytes), (39, 53));
        assert!(scheduler.admit(image.id, unified(39, 53)));
        assert_eq!(scheduler.snapshot()[0].wait, None);
        // Not alone: free memory decides again.
        let speech = scheduler.submit("speech".into(), Priority::Interactive, 10);
        assert!(!scheduler.admit(speech.id, unified(5, 53)));
        assert_eq!(
            scheduler.snapshot()[1].wait.unwrap().reason,
            WaitReason::Memory
        );
        drop(image);
        assert!(scheduler.admit(speech.id, unified(5, 53)));
    }

    #[test]
    fn a_queued_job_says_what_it_waits_for() {
        let scheduler = Arc::new(JobScheduler::default());
        let resident = scheduler.submit("chat".into(), Priority::Interactive, 40);
        let unknown = scheduler.submit("router".into(), Priority::Interactive, u64::MAX);
        let after = scheduler.submit("speech".into(), Priority::Agent, 10);
        assert!(scheduler.admit(resident.id, discrete(100, 100)));
        assert!(!scheduler.admit(unknown.id, discrete(100, 100)));
        assert!(!scheduler.admit(after.id, discrete(100, 100)));
        let jobs = scheduler.snapshot();
        assert_eq!(jobs[0].wait, None, "running");
        assert_eq!(jobs[1].wait.unwrap().reason, WaitReason::Alone);
        assert_eq!(jobs[2].wait.unwrap().reason, WaitReason::Queue);
    }

    #[tokio::test]
    async fn cancelled_waiter_returns_without_probing_hardware() {
        let scheduler = Arc::new(JobScheduler::default());
        let ticket = scheduler.submit("queued".into(), Priority::Agent, u64::MAX);
        ticket.request_cancel();
        assert_eq!(ticket.wait().await, Err("jobs.cancelled".into()));
        drop(ticket);
        assert!(scheduler.snapshot().is_empty());
    }
}
