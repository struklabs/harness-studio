//! Why a llama-server run failed: classification of the run's log and exit status into a
//! stable error code with an i18n message key and parameters.
//!
//! Every class was checked against the real output of llama-server b11236 (Windows, Vulkan);
//! the captured texts are the test data in `tests/data/failures/`. Markers of the other
//! backends (CUDA, ROCm/HIP, Metal, CPU) and of the Linux/macOS loaders come from their
//! sources and are covered by fixture tests only.
//!
//! * `code` is the machine code (`servers.out_of_memory`...): the Control API returns it as
//!   `error.code`, the CLI maps it to its exit code.
//! * `message_key` is the GUI locale key (`failure.out_of_memory`...); `params` fill it.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Failure classes, most specific first (the order [`classify`] tries them).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    /// A device (or host) allocation failed: VRAM/RAM does not fit the model + context.
    OutOfMemory,
    /// The runtime could not load a shared library (DLL / .so / dylib).
    MissingLibrary,
    /// The GGUF file is truncated, not a GGUF, or its tensors are out of bounds.
    CorruptModel,
    /// This runtime does not know the model architecture.
    UnsupportedArchitecture,
    /// The model file does not exist or cannot be opened.
    ModelNotFound,
    /// The HTTP port could not be bound.
    PortInUse,
    /// This runtime rejected a command-line argument (unknown flag or bad value).
    InvalidArgument,
    /// No healthy `/health` within the load timeout.
    LoadTimeout,
    /// The server exited after it was running (no known cause in the log).
    Crashed,
    /// The server exited while loading (no known cause in the log), or could not be spawned.
    StartFailed,
    /// The GPU stopped the work: the driver reset the device (Windows TDR, Vulkan device
    /// lost, Metal GPU timeout) or a kernel faulted (CUDA/HIP launch failure, illegal access).
    DeviceLost,
    /// The target loaded but the draft model of speculative decoding did not (`-md`): it did
    /// not fit (`params.cause = memory`), is missing (`not_found`) or broken (`corrupt`).
    DraftLoadFailed,
}

impl FailureKind {
    pub const ALL: [FailureKind; 12] = [
        FailureKind::OutOfMemory,
        FailureKind::MissingLibrary,
        FailureKind::CorruptModel,
        FailureKind::UnsupportedArchitecture,
        FailureKind::ModelNotFound,
        FailureKind::PortInUse,
        FailureKind::InvalidArgument,
        FailureKind::LoadTimeout,
        FailureKind::Crashed,
        FailureKind::StartFailed,
        FailureKind::DeviceLost,
        FailureKind::DraftLoadFailed,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            FailureKind::OutOfMemory => "out_of_memory",
            FailureKind::MissingLibrary => "missing_library",
            FailureKind::CorruptModel => "corrupt_model",
            FailureKind::UnsupportedArchitecture => "unsupported_architecture",
            FailureKind::ModelNotFound => "model_not_found",
            FailureKind::PortInUse => "port_in_use",
            FailureKind::InvalidArgument => "invalid_argument",
            FailureKind::LoadTimeout => "load_timeout",
            FailureKind::Crashed => "crashed",
            FailureKind::StartFailed => "start_failed",
            FailureKind::DeviceLost => "device_lost",
            FailureKind::DraftLoadFailed => "draft_load_failed",
        }
    }

    /// Stable machine code (`servers.out_of_memory`).
    pub fn code(self) -> String {
        format!("servers.{}", self.as_str())
    }

    /// GUI locale key (`failure.out_of_memory`).
    pub fn message_key(self) -> String {
        format!("failure.{}", self.as_str())
    }
}

/// When the run failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum FailurePhase {
    /// Before `/health` answered (spawn, model load, bind).
    Loading,
    /// After the server was ready.
    Running,
}

/// A classified failure of one run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ServerFailure {
    /// Stable machine code (`servers.out_of_memory`, `servers.port_in_use`...).
    pub code: String,
    pub kind: FailureKind,
    /// i18n key of the message (`failure.out_of_memory`...).
    pub message_key: String,
    /// Values for the message (`flag`, `arch`, `port`, `library`, `exit`, `seconds`...).
    pub params: BTreeMap<String, String>,
    pub phase: FailurePhase,
    /// Process exit code (Windows NTSTATUS values are negative here; `params.exit` has hex).
    pub exit_code: Option<i32>,
    /// Log lines the classification is based on (at most 4).
    pub evidence: Vec<String>,
    /// Unix seconds.
    pub at: u64,
}

impl ServerFailure {
    pub fn new(kind: FailureKind, phase: FailurePhase) -> Self {
        Self {
            code: kind.code(),
            kind,
            message_key: kind.message_key(),
            params: BTreeMap::new(),
            phase,
            exit_code: None,
            evidence: Vec::new(),
            at: crate::capabilities::now_secs(),
        }
    }

    pub fn with(mut self, key: &str, value: impl ToString) -> Self {
        self.params.insert(key.to_string(), value.to_string());
        self
    }

    /// `true` for the classes the OOM recovery options apply to.
    pub fn is_out_of_memory(&self) -> bool {
        self.kind == FailureKind::OutOfMemory
    }
}

/// Windows NTSTATUS exit codes of a process that could not start its image.
const STATUS_DLL_NOT_FOUND: u32 = 0xC000_0135;
const STATUS_ENTRYPOINT_NOT_FOUND: u32 = 0xC000_0139;
const STATUS_INVALID_IMAGE_FORMAT: u32 = 0xC000_007B;
const STATUS_ACCESS_VIOLATION: u32 = 0xC000_0005;
const STATUS_STACK_BUFFER_OVERRUN: u32 = 0xC000_0409;

/// Case-insensitive markers of an allocation failure (CUDA, HIP/ROCm, Vulkan, Metal, SYCL,
/// CPU). `failed to allocate` / `unable to allocate` cover the ggml backend buffer messages
/// (`ggml_backend_*_buffer_type_alloc_buffer: ... failed to allocate`).
const OOM_MARKERS: &[&str] = &[
    "erroroutofdevicememory",
    "erroroutofhostmemory",
    "outofdevicememory",
    "out of memory",
    "cudamalloc failed",
    "hipmalloc failed",
    "failed to allocate",
    "unable to allocate",
    "insufficient memory",
    "not enough memory",
    "std::bad_alloc",
    "bad_alloc",
    "kiogpucommandbuffercallbackerroroutofmemory",
    // A Python worker (MLX, Hugging Face) that could not allocate.
    "memoryerror",
];

/// Case-insensitive markers of a lost or faulted GPU. The CUDA texts are `cudaGetErrorString`
/// results (found in cudart64_12.dll of b11327; ggml prints them as `CUDA error: <text>`, HIP
/// uses the same texts); a Windows TDR during a kernel shows up as the launch timeout or the
/// unspecified launch failure. Vulkan: `vk::DeviceLostError` (`ErrorDeviceLost`). Metal: the
/// command buffer timeout / hang errors.
const DEVICE_LOST_MARKERS: &[&str] = &[
    "the launch timed out and was terminated",
    "unspecified launch failure",
    "an illegal memory access was encountered",
    "an illegal instruction was encountered",
    "errordevicelost",
    "vk_error_device_lost",
    "kiogpucommandbuffercallbackerrortimeout",
    "kiogpucommandbuffercallbackerrorhang",
    "kiogpucommandbuffercallbackerrorimpactinginteractivity",
];

/// The subset that means the driver reset the device (TDR / timeout), not a kernel bug.
const DEVICE_RESET_MARKERS: &[&str] = &[
    "the launch timed out and was terminated",
    "errordevicelost",
    "vk_error_device_lost",
    "kiogpucommandbuffercallbackerrortimeout",
    "kiogpucommandbuffercallbackerrorimpactinginteractivity",
];

/// Markers of a truncated / invalid GGUF.
const CORRUPT_MARKERS: &[&str] = &[
    "not within the file bounds",
    "model is corrupted or incomplete",
    "invalid magic characters",
    "failed to read key-value pairs",
    "failed to read tensor info",
    "failed to read tensor data",
    "gguf_init_from_file_impl: failed to read",
    "invalid gguf",
    "tensor data is not within",
    "unexpectedly reached end of file",
    "failed to read magic",
    // whisper.cpp: `whisper_model_load: invalid model data (bad magic)`
    "invalid model data",
];

/// Whether a text reports an out-of-memory failure.
pub fn mentions_out_of_memory(text: &str) -> bool {
    let lower = text.to_lowercase();
    OOM_MARKERS.iter().any(|m| lower.contains(m)) || is_unallocated_tensor(&lower)
}

/// Whether a log line is ggml-metal's command buffer out-of-memory
/// (`kIOGPUCommandBufferCallbackErrorOutOfMemory`). The request then fails with
/// "Compute error." and llama-server keeps running, so the exit-time [`classify`] never sees
/// it; the supervisor watches for it while the server loads and shortly after.
pub fn metal_out_of_memory(line: &str) -> bool {
    line.to_lowercase()
        .contains("kiogpucommandbuffercallbackerroroutofmemory")
}

/// ggml-backend.cpp's scheduler assert for a tensor whose buffer was never allocated:
/// `pre-allocated tensor (cache_k_l0) in a buffer (Vulkan0) that cannot run the operation (NONE)`.
/// A far too large context (`-c 20000000`) makes the KV cache allocation come back empty and
/// llama-server aborts on this line (Windows: exit 0xC0000409) instead of logging
/// "failed to allocate". The operation is `NONE` (no op at all, the buffer is missing); a real
/// unsupported op names the op (`MUL_MAT`) and is not a memory problem. `lower` is lowercase.
fn is_unallocated_tensor(lower: &str) -> bool {
    lower.contains("pre-allocated tensor") && lower.contains("cannot run the operation (none)")
}

/// Log text of a line without the `[stderr]` / `[stdout]` prefix the ring buffer adds.
fn body(line: &str) -> &str {
    line.strip_prefix("[stderr] ")
        .or_else(|| line.strip_prefix("[stdout] "))
        .unwrap_or(line)
}

/// Harness' own lines are not evidence (they may quote earlier errors).
fn is_harness_line(line: &str) -> bool {
    line.starts_with("[Harness Studio")
}

/// Text between the first pair of `'` or `"` after `after`.
fn quoted_after(line: &str, after: &str) -> Option<String> {
    let lower = line.to_lowercase();
    let pos = lower.find(after)? + after.len();
    let rest = &line[pos..];
    let start = rest.find(['\'', '"'])?;
    let quote = rest[start..].chars().next()?;
    let inner = &rest[start + 1..];
    let end = inner.find(quote)?;
    Some(inner[..end].to_string())
}

/// `port: 18702` / `port 18702` at the end of a bind error.
fn port_in(line: &str) -> Option<String> {
    let lower = line.to_lowercase();
    let pos = lower.rfind("port")? + 4;
    let digits: String = line[pos..]
        .trim_start_matches([':', ' ', '='])
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    (!digits.is_empty()).then_some(digits)
}

/// Library name in a loader error (`libcuda.so.1: cannot open shared object file`,
/// `Library not loaded: @rpath/libggml.dylib`, `LoadLibrary failed for ggml-cuda.dll`).
fn library_in(line: &str) -> Option<String> {
    let lower = line.to_lowercase();
    if let Some(pos) = lower.find("error while loading shared libraries:") {
        let rest = line[pos + "error while loading shared libraries:".len()..].trim();
        return rest.split(':').next().map(|s| s.trim().to_string());
    }
    if let Some(pos) = lower.find("library not loaded:") {
        let rest = line[pos + "library not loaded:".len()..].trim();
        return rest.split_whitespace().next().map(str::to_string);
    }
    line.split(|c: char| c.is_whitespace() || c == '\'' || c == '"' || c == ',')
        .find(|t| {
            let t = t.to_lowercase();
            t.ends_with(".dll") || t.contains(".so") || t.ends_with(".dylib")
        })
        .map(|t| {
            t.trim_matches(|c| c == ':' || c == '(' || c == ')')
                .to_string()
        })
}

/// `0xC0000135` for Windows status codes, the decimal code otherwise.
pub fn exit_text(code: i32) -> String {
    if code < 0 || (code as u32) >= 0xC000_0000 {
        format!("0x{:08X}", code as u32)
    } else {
        code.to_string()
    }
}

/// Adds the terminating signal of a Unix process to `f` (`params.exit` = "SIGABRT (6)"):
/// a signal-killed child has no exit code, so [`classify`] left `exit` empty. SIGSEGV and
/// SIGBUS of an otherwise unexplained crash also set `params.signal`, as the Windows access
/// violation does. SIGABRT does not: an abort is not an invalid memory access. No-op elsewhere.
pub fn note_signal(f: &mut ServerFailure, status: &std::process::ExitStatus) {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if status.code().is_some() {
            return;
        }
        let Some(sig) = status.signal() else {
            return;
        };
        let text = signal_text(sig);
        f.params.insert("exit".into(), text);
        // A SIGKILL that left no explanation in the log came from outside (our own stop never
        // reaches the classification): the system when memory ran low (macOS jetsam, the Linux
        // OOM killer), or another program. Its own message says both;
        // [`confirm_memory_kill`] makes it a memory failure when the system's log says so.
        if sig == libc_signal::KILL
            && matches!(f.kind, FailureKind::Crashed | FailureKind::StartFailed)
        {
            f.message_key = "failure.killed".into();
            return;
        }
        let access = sig == libc_signal::SEGV || sig == libc_signal::BUS;
        if access
            && !f.params.contains_key("signal")
            && matches!(f.kind, FailureKind::Crashed | FailureKind::StartFailed)
        {
            f.params.insert("signal".into(), "access_violation".into());
        }
    }
    #[cfg(not(unix))]
    let _ = (f, status);
}

/// A run that [`note_signal`] found killed from outside (`failure.killed`) becomes an
/// out-of-memory failure (`failure.killed_by_system`, with the recovery options and no automatic
/// restart) when the system's log shows it ended `pid` for memory: on macOS the kernel's
/// memorystatus (jetsam) line of the last two minutes. Elsewhere, or without such a line, it stays
/// as it is.
///
/// Blocking: `log show` takes about 1.5 s, longer under memory pressure, so it gets
/// [`JETSAM_LOG_DEADLINE`] and is killed after it (the failure then stays `failure.killed`).
/// Call it off the async runtime. The kernel's line can also reach the log a moment after the
/// exit we report: a kill that is not found yet stays `failure.killed` (a false negative, never
/// a false "memory ran out").
pub fn confirm_memory_kill(f: &mut ServerFailure, pid: u32) {
    if f.message_key != "failure.killed" {
        return;
    }
    #[cfg(target_os = "macos")]
    {
        let mut cmd = crate::process::hidden_command("/usr/bin/log");
        cmd.args([
            "show",
            "--last",
            "2m",
            "--style",
            "compact",
            "--predicate",
            "sender == \"kernel\" AND eventMessage CONTAINS \"memorystatus: kill\"",
        ]);
        if let Some(log) = output_within(cmd, JETSAM_LOG_DEADLINE) {
            apply_memory_kill(f, &log, pid);
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (f, pid);
}

/// How long [`confirm_memory_kill`] waits for `log show`.
pub const JETSAM_LOG_DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);

/// `failure.killed` -> `failure.killed_by_system` (out of memory) when `log` names `pid` in a
/// jetsam kill ([`jetsam_killed`]); anything else stays as it is.
#[cfg(any(target_os = "macos", test))]
fn apply_memory_kill(f: &mut ServerFailure, log: &str, pid: u32) {
    if f.message_key == "failure.killed" && jetsam_killed(log, pid) {
        f.kind = FailureKind::OutOfMemory;
        f.code = f.kind.code();
        f.message_key = "failure.killed_by_system".into();
    }
}

/// The stdout of `cmd`, or `None` when it cannot start or does not finish within `deadline`
/// (then it is killed). Stdout is read on its own thread, so a full pipe cannot stall the wait.
#[cfg(unix)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn output_within(mut cmd: std::process::Command, deadline: std::time::Duration) -> Option<String> {
    use std::io::Read;
    use std::process::Stdio;
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stdout.read_to_string(&mut text);
        let _ = tx.send(text);
    });
    match rx.recv_timeout(deadline) {
        Ok(text) => {
            let _ = child.wait();
            Some(text)
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            None
        }
    }
}

/// Whether macOS memorystatus lines (`log show`) say the kernel killed `pid` for memory: a
/// `memorystatus: killing_<reason>` line naming `pid N ` (`killing_top_process pid 1755
/// [python3.12] (vm-pageshortage ...)`). Idle exits of daemons (`idle-exit`) are no memory kills.
#[cfg(any(target_os = "macos", test))]
fn jetsam_killed(log: &str, pid: u32) -> bool {
    let by_pid = format!(" pid {pid} ");
    log.lines().any(|l| {
        l.split_once("memorystatus: killing_")
            .is_some_and(|(_, rest)| rest.contains(&by_pid))
            && !l.contains("idle-exit")
    })
}

/// Signal numbers [`note_signal`] names, from libc (SIGBUS is 10 on macOS, 7 on Linux).
#[cfg(unix)]
mod libc_signal {
    pub const ABRT: i32 = libc::SIGABRT;
    pub const KILL: i32 = libc::SIGKILL;
    pub const BUS: i32 = libc::SIGBUS;
    pub const SEGV: i32 = libc::SIGSEGV;
    pub const TERM: i32 = libc::SIGTERM;
}

/// `6` → "SIGABRT (6)"; unnamed signals → "signal N".
#[cfg(unix)]
fn signal_text(sig: i32) -> String {
    let name = match sig {
        libc_signal::ABRT => "SIGABRT",
        libc_signal::KILL => "SIGKILL",
        libc_signal::BUS => "SIGBUS",
        libc_signal::SEGV => "SIGSEGV",
        libc_signal::TERM => "SIGTERM",
        _ => return format!("signal {sig}"),
    };
    format!("{name} ({sig})")
}

fn push_evidence(evidence: &mut Vec<String>, line: &str) {
    if evidence.len() < 4 && !evidence.iter().any(|e| e == line) {
        evidence.push(line.to_string());
    }
}

/// Classifies a failed run from its log lines (this run only) and exit code.
pub fn classify(lines: &[String], exit_code: Option<i32>, phase: FailurePhase) -> ServerFailure {
    let lines: Vec<&str> = lines
        .iter()
        .map(String::as_str)
        .filter(|l| !is_harness_line(l))
        .map(body)
        .collect();
    let find = |pred: &dyn Fn(&str) -> bool| -> Vec<&str> {
        lines.iter().copied().filter(|l| pred(l)).collect()
    };
    let finish = |mut f: ServerFailure, hits: &[&str]| {
        for h in hits {
            push_evidence(&mut f.evidence, h.trim());
        }
        f.exit_code = exit_code;
        if let Some(code) = exit_code {
            f.params.insert("exit".into(), exit_text(code));
        }
        f
    };

    // 0. The draft model failed to load (the target had loaded). Checked before everything
    //    else: its errors look like the target's ("failed to open GGUF file" is not the
    //    target missing, an allocation failure needs a smaller draft, not a smaller context).
    //    A draft that does not fit in VRAM fails in b11333 with "invalid vector subscript"
    //    (CUDA, Windows; tests/data/failures/b11333-cuda-draft_no_fit.log).
    let draft_hits = find(&|l| l.to_lowercase().contains("failed to load draft model"));
    if !draft_hits.is_empty() {
        let lower: Vec<String> = lines.iter().map(|l| l.to_lowercase()).collect();
        let any = |pred: &dyn Fn(&str) -> bool| lower.iter().any(|l| pred(l));
        let cause = if any(&|l| {
            l.contains("failed to open gguf file") || l.contains("no such file")
        }) {
            "not_found"
        } else if any(&|l| CORRUPT_MARKERS.iter().any(|m| l.contains(m))) {
            "corrupt"
        } else if any(&|l| l.contains("invalid vector subscript") || mentions_out_of_memory(l)) {
            "memory"
        } else {
            "unknown"
        };
        let draft = draft_hits
            .iter()
            .find_map(|l| quoted_after(l, "failed to load draft model"))
            .unwrap_or_default();
        let mut evidence = find(&|l| l.to_lowercase().contains("error loading model"));
        evidence.truncate(1);
        evidence.extend(draft_hits.iter().take(1));
        return finish(
            ServerFailure::new(FailureKind::DraftLoadFailed, phase)
                .with("cause", cause)
                .with("draft", draft),
            &evidence,
        );
    }

    // 1. Out of memory (any backend). Checked first: an allocation failure is often followed
    //    by generic "failed to load model" lines.
    let hits = find(&|l| mentions_out_of_memory(l));
    if !hits.is_empty() {
        let mut f = ServerFailure::new(FailureKind::OutOfMemory, phase);
        if let Some(mib) = hits.iter().find_map(|l| requested_mib(l)) {
            f = f.with("requested_mib", mib);
        }
        // What did not fit: the KV cache, the weights or the compute buffers. Decides which
        // recovery option comes first.
        let all = lines
            .join(
                "
",
            )
            .to_lowercase();
        let stage = if all.contains("buffer for kv cache")
            || all.contains("llama_kv_cache: failed")
            || all.contains("pre-allocated tensor (cache_")
        {
            Some("kv_cache")
        } else if all.contains("compute buffer") || all.contains("ggml_gallocr_reserve") {
            Some("compute")
        } else if all.contains("load_tensors") || all.contains("model buffer") {
            Some("weights")
        } else {
            None
        };
        if let Some(stage) = stage {
            f = f.with("stage", stage);
        }
        return finish(f, &hits);
    }

    // 2. The GPU was lost or a kernel faulted. After OOM: `CUDA error: out of memory` is an
    //    allocation failure, not a lost device.
    let hits = find(&|l| {
        let l = l.to_lowercase();
        DEVICE_LOST_MARKERS.iter().any(|m| l.contains(m))
    });
    if !hits.is_empty() {
        let lower = hits[0].to_lowercase();
        let mut f = ServerFailure::new(FailureKind::DeviceLost, phase);
        if let Some(detail) = DEVICE_LOST_MARKERS.iter().find(|m| lower.contains(*m)) {
            f = f.with("detail", detail);
        }
        let reset = hits.iter().any(|l| {
            let l = l.to_lowercase();
            DEVICE_RESET_MARKERS.iter().any(|m| l.contains(m))
        });
        f = f.with(
            "cause",
            if reset {
                "driver_reset"
            } else {
                "kernel_fault"
            },
        );
        // ggml's `current device: 0, in function ... at file:line` names the failed call.
        let mut evidence = hits;
        evidence.extend(
            find(&|l| l.trim_start().starts_with("current device:"))
                .into_iter()
                .take(1),
        );
        return finish(f, &evidence);
    }

    // 3. The Windows loader could not start the image (a DLL is missing or incompatible).
    let status = exit_code.map(|c| c as u32);
    if matches!(
        status,
        Some(STATUS_DLL_NOT_FOUND | STATUS_ENTRYPOINT_NOT_FOUND | STATUS_INVALID_IMAGE_FORMAT)
    ) {
        return finish(ServerFailure::new(FailureKind::MissingLibrary, phase), &[]);
    }

    // 4. Unknown architecture.
    let hits = find(&|l| l.to_lowercase().contains("unknown model architecture"));
    if !hits.is_empty() {
        let arch = hits
            .iter()
            .find_map(|l| quoted_after(l, "unknown model architecture"))
            .unwrap_or_default();
        return finish(
            ServerFailure::new(FailureKind::UnsupportedArchitecture, phase).with("arch", arch),
            &hits,
        );
    }

    // 5. Corrupt / truncated GGUF.
    let hits = find(&|l| {
        let l = l.to_lowercase();
        CORRUPT_MARKERS.iter().any(|m| l.contains(m))
    });
    if !hits.is_empty() {
        return finish(ServerFailure::new(FailureKind::CorruptModel, phase), &hits);
    }

    // 6. Model file missing / unreadable.
    let hits = find(&|l| {
        let l = l.to_lowercase();
        l.contains("failed to open gguf file")
            || (l.contains("failed to load model") && l.contains("no such file"))
    });
    if !hits.is_empty() {
        return finish(ServerFailure::new(FailureKind::ModelNotFound, phase), &hits);
    }

    // 7. Port in use (llama-server: "couldn't bind HTTP server socket, hostname: h, port: p").
    let hits = find(&|l| {
        let l = l.to_lowercase();
        l.contains("couldn't bind")
            || l.contains("could not bind")
            || l.contains("address already in use")
    });
    if !hits.is_empty() {
        let mut f = ServerFailure::new(FailureKind::PortInUse, phase);
        if let Some(port) = hits.iter().find_map(|l| port_in(l)) {
            f = f.with("port", port);
        }
        return finish(f, &hits);
    }

    // 8. Invalid argument: "error: invalid argument: --x" or
    //    "error while handling argument \"-ngl\": invalid stoi argument".
    let hits = find(&|l| {
        let l = l.to_lowercase();
        l.starts_with("error: invalid argument")
            || l.contains("error while handling argument")
            || l.starts_with("error: unknown argument")
    });
    if !hits.is_empty() {
        let line = hits[0];
        let lower = line.to_lowercase();
        let mut f = ServerFailure::new(FailureKind::InvalidArgument, phase);
        if lower.contains("error while handling argument") {
            if let Some(flag) = quoted_after(line, "error while handling argument") {
                f = f.with("flag", flag);
            }
            if let Some((_, detail)) = line.rsplit_once("\": ") {
                f = f.with("detail", detail.trim());
            }
        } else if let Some((_, flag)) = line.rsplit_once(": ") {
            f = f.with("flag", flag.trim());
        }
        return finish(f, &hits);
    }

    // 9. Linux/macOS loader errors (checked late: llama.cpp also logs non-fatal backend
    //    load failures, which must not hide the real cause above).
    let loader_hits = find(&|l| {
        let l = l.to_lowercase();
        l.contains("error while loading shared libraries")
            || l.contains("library not loaded")
            || l.contains("cannot open shared object file")
    });
    if !loader_hits.is_empty() {
        let mut f = ServerFailure::new(FailureKind::MissingLibrary, phase);
        if let Some(lib) = loader_hits.iter().find_map(|l| library_in(l)) {
            f = f.with("library", lib);
        }
        return finish(f, &loader_hits);
    }

    // 10. Nothing specific: crash (after ready) or start failure. Keep the last error lines.
    let errors: Vec<&str> = lines
        .iter()
        .copied()
        .filter(|l| {
            let lower = l.to_lowercase();
            l.contains(" E ") || lower.starts_with("error") || lower.contains("exception")
        })
        .collect();
    let tail: Vec<&str> = errors.iter().rev().take(3).rev().copied().collect();
    let kind = match phase {
        FailurePhase::Running => FailureKind::Crashed,
        FailurePhase::Loading => FailureKind::StartFailed,
    };
    let mut f = ServerFailure::new(kind, phase);
    if matches!(
        status,
        Some(STATUS_ACCESS_VIOLATION | STATUS_STACK_BUFFER_OVERRUN)
    ) {
        f = f.with("signal", "access_violation");
    }
    finish(f, &tail)
}

/// `allocating 9000.00 MiB` → 9000; `allocation of size 1073741824` (bytes) → 1024.
fn requested_mib(line: &str) -> Option<String> {
    let lower = line.to_lowercase();
    let Some(pos) = lower.find("mib") else {
        let pos = lower.find("of size ")? + "of size ".len();
        let digits: String = lower[pos..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        let bytes: u64 = digits.parse().ok()?;
        return (bytes >= 1 << 20).then(|| format!("{}", bytes >> 20));
    };
    let before = lower[..pos].trim_end();
    let num: String = before
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let value: f64 = num.parse().ok()?;
    Some(format!("{:.0}", value))
}

/// Failure for a spawn error before the process ran (the OS refused to start it).
pub fn spawn_failure(error: &std::io::Error) -> ServerFailure {
    ServerFailure::new(FailureKind::StartFailed, FailurePhase::Loading).with("detail", error)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Vec<String> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data/failures")
            .join(name);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{}: {}", path.display(), e))
            .lines()
            // As the ring buffer stores them.
            .map(|l| format!("[stderr] {}", l))
            .collect()
    }

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(|l| format!("[stderr] {}", l)).collect()
    }

    #[test]
    fn real_b11236_outputs_are_classified() {
        let f = classify(
            &fixture("b11236-invalid_arg.log"),
            Some(1),
            FailurePhase::Loading,
        );
        assert_eq!(f.kind, FailureKind::InvalidArgument);
        assert_eq!(f.params["flag"], "--no-such-flag-xyz");
        assert_eq!(f.code, "servers.invalid_argument");
        assert_eq!(f.message_key, "failure.invalid_argument");

        let f = classify(
            &fixture("b11236-invalid_val.log"),
            Some(1),
            FailurePhase::Loading,
        );
        assert_eq!(f.kind, FailureKind::InvalidArgument);
        assert_eq!(f.params["flag"], "-ngl");
        assert_eq!(f.params["detail"], "invalid stoi argument");

        for name in ["truncated", "truncated_header", "bad_magic"] {
            let f = classify(
                &fixture(&format!("b11236-{name}.log")),
                Some(1),
                FailurePhase::Loading,
            );
            assert_eq!(f.kind, FailureKind::CorruptModel, "{name}: {f:?}");
            assert!(!f.evidence.is_empty());
        }

        let f = classify(
            &fixture("b11236-unknown_arch.log"),
            Some(1),
            FailurePhase::Loading,
        );
        assert_eq!(f.kind, FailureKind::UnsupportedArchitecture);
        assert_eq!(f.params["arch"], "fakearch");

        let f = classify(
            &fixture("b11236-missing_model.log"),
            Some(1),
            FailurePhase::Loading,
        );
        assert_eq!(f.kind, FailureKind::ModelNotFound);

        let f = classify(
            &fixture("b11236-port_in_use.log"),
            Some(1),
            FailurePhase::Loading,
        );
        assert_eq!(f.kind, FailureKind::PortInUse);
        assert_eq!(f.params["port"], "18702");
        assert_eq!(f.params["exit"], "1");
    }

    #[test]
    fn real_b11236_vulkan_oom_is_classified() {
        let f = classify(
            &fixture("b11236-oom_vulkan.log"),
            Some(1),
            FailurePhase::Loading,
        );
        assert_eq!(f.kind, FailureKind::OutOfMemory, "{f:?}");
        assert!(f.evidence.iter().any(|l| mentions_out_of_memory(l)));
        assert_eq!(f.params["stage"], "kv_cache");
        assert_eq!(f.params["requested_mib"], "1024");
    }

    #[test]
    fn real_b11236_vulkan_huge_context_abort_is_oom() {
        // Real: Qwen2.5-0.5B Q4_K_M, `-c 20000000 -ngl 99`, Vulkan, Windows: ggml aborts with
        // exit 0xC0000409 and no "failed to allocate" line (TC-09 of the regression round).
        let log = fixture("b11236-oom_prealloc_kv_vulkan.log");
        let f = classify(&log, Some(-1073740791), FailurePhase::Loading);
        assert_eq!(f.kind, FailureKind::OutOfMemory, "{f:?}");
        assert!(f.is_out_of_memory());
        assert_eq!(f.params["stage"], "kv_cache");
        assert_eq!(f.params["exit"], "0xC0000409");
        assert!(f.evidence[0].contains("pre-allocated tensor (cache_k_l0)"));
        // The same abort code without any memory evidence is not an OOM.
        let f = classify(
            &lines("0.01 I srv  load_model: loading model 'x.gguf'"),
            Some(-1073740791),
            FailurePhase::Loading,
        );
        assert_eq!(f.kind, FailureKind::StartFailed, "{f:?}");
        // A real unsupported operation names the op: not a memory problem.
        let f = classify(
            &lines("ggml-backend.cpp:1081: pre-allocated tensor (x) in a buffer (CUDA0) that cannot run the operation (MUL_MAT)"),
            Some(-1073740791),
            FailurePhase::Running,
        );
        assert_eq!(f.kind, FailureKind::Crashed, "{f:?}");
    }

    #[test]
    fn missing_dll_exit_status() {
        // Real: llama-server.exe copied without its DLLs, PATH without another llama.cpp.
        let f = classify(&[], Some(-1073741515), FailurePhase::Loading);
        assert_eq!(f.kind, FailureKind::MissingLibrary);
        assert_eq!(f.params["exit"], "0xC0000135");
        // Fixtures (loader messages from glibc and dyld).
        let f = classify(
            &lines("llama-server: error while loading shared libraries: libcuda.so.1: cannot open shared object file: No such file or directory"),
            Some(127),
            FailurePhase::Loading,
        );
        assert_eq!(f.kind, FailureKind::MissingLibrary);
        assert_eq!(f.params["library"], "libcuda.so.1");
        let f = classify(
            &lines("dyld[123]: Library not loaded: @rpath/libggml-metal.dylib\n  Referenced from: llama-server"),
            Some(-6),
            FailurePhase::Loading,
        );
        assert_eq!(f.params["library"], "@rpath/libggml-metal.dylib");
    }

    #[test]
    fn oom_markers_of_other_backends() {
        for text in [
            "ggml_backend_cuda_buffer_type_alloc_buffer: allocating 9000.00 MiB on device 0: cudaMalloc failed: out of memory",
            "ggml_backend_cuda_buffer_type_alloc_buffer: allocating 512.00 MiB on device 0: hipMalloc failed: out of memory",
            "ggml_vulkan: Device memory allocation of size 4294967296 failed.\nggml_vulkan: Requested buffer size exceeds device memory allocation limit: ErrorOutOfDeviceMemory",
            "ggml_metal_buffer_type_alloc_buffer: error: failed to allocate buffer, size = 20000.00 MiB",
            "ggml_backend_cpu_buffer_type_alloc_buffer: failed to allocate buffer of size 68719476736",
            "terminate called after throwing an instance of 'std::bad_alloc'",
        ] {
            let f = classify(&lines(text), Some(1), FailurePhase::Loading);
            assert_eq!(f.kind, FailureKind::OutOfMemory, "{text}");
        }
        let f = classify(
            &lines("allocating 9000.00 MiB on device 0: cudaMalloc failed: out of memory"),
            None,
            FailurePhase::Running,
        );
        assert_eq!(f.params["requested_mib"], "9000");
        assert_eq!(f.phase, FailurePhase::Running);
    }

    #[test]
    fn unknown_failures_by_phase_and_harness_lines_ignored() {
        let log = vec![
            "[Harness Studio Error] önceki: out of memory".to_string(),
            "[stderr] 0.01 E srv  something odd".to_string(),
        ];
        let f = classify(&log, Some(-1073741819), FailurePhase::Running);
        assert_eq!(f.kind, FailureKind::Crashed);
        assert_eq!(f.params["exit"], "0xC0000005");
        assert_eq!(f.params["signal"], "access_violation");
        assert_eq!(f.evidence, vec!["0.01 E srv  something odd"]);
        let f = classify(&log, Some(3), FailurePhase::Loading);
        assert_eq!(f.kind, FailureKind::StartFailed);
        assert_eq!(f.code, "servers.start_failed");
    }

    #[test]
    fn jetsam_lines_name_memory_kills_only() {
        // Real lines (macOS 27, M1 8 GB): an idle exit and the form of a memory kill.
        let log = "2026-10-15 19:57:39.442 Df kernel[0:671] [com.apple.xnu:memorystatus] memorystatus: killing (idle) replayd [86562] due to idle-exit (9)\n\
2026-10-15 19:57:39.444 Df kernel[0:671] [com.apple.xnu:memorystatus] memorystatus: killing_idle_process pid 86562 [replayd] (idle-exit 0 2889s rf:high type:daemon) 6992KB - memorystatus_available_pages: 213901\n\
2026-10-15 19:58:02.101 Df kernel[0:671] [com.apple.xnu:memorystatus] memorystatus: killing_top_process pid 1755 [python3.12] (vm-pageshortage 1) 5123456KB - memorystatus_available_pages: 1203";
        assert!(jetsam_killed(log, 1755));
        assert!(!jetsam_killed(log, 86562), "an idle exit is no memory kill");
        assert!(!jetsam_killed(log, 175), "a pid prefix is another process");
        assert!(!jetsam_killed(log, 5123456), "a size is no pid");
        assert!(!jetsam_killed("", 1755));

        // A kill from outside that the log names becomes out of memory...
        let killed = || {
            let mut f = ServerFailure::new(FailureKind::Crashed, FailurePhase::Running)
                .with("exit", "SIGKILL (9)");
            f.message_key = "failure.killed".into();
            f
        };
        let mut f = killed();
        apply_memory_kill(&mut f, log, 1755);
        assert_eq!(f.kind, FailureKind::OutOfMemory);
        assert_eq!(f.code, "servers.out_of_memory");
        assert_eq!(f.message_key, "failure.killed_by_system");
        assert_eq!(f.params["exit"], "SIGKILL (9)");
        // ...one it does not name (another program, or the line is not there yet) stays killed.
        let mut f = killed();
        apply_memory_kill(&mut f, log, 4242);
        assert_eq!(f.kind, FailureKind::Crashed);
        assert_eq!(f.message_key, "failure.killed");
        // Any other failure is never looked at, even when the log names its pid.
        let mut f = ServerFailure::new(FailureKind::Crashed, FailurePhase::Running);
        apply_memory_kill(&mut f, log, 1755);
        assert_eq!(f.kind, FailureKind::Crashed);
        assert_eq!(f.message_key, "failure.crashed");
    }

    /// A command that does not finish within the deadline is killed and gives nothing (a slow
    /// `log show` under memory pressure must not hold up the failure report).
    #[cfg(unix)]
    #[test]
    fn a_slow_log_query_is_cut_at_its_deadline() {
        let mut slow = std::process::Command::new("sh");
        slow.args(["-c", "sleep 10"]);
        let t = std::time::Instant::now();
        assert_eq!(
            output_within(slow, std::time::Duration::from_millis(200)),
            None
        );
        assert!(
            t.elapsed() < std::time::Duration::from_secs(3),
            "{:?}",
            t.elapsed()
        );
        let mut quick = std::process::Command::new("sh");
        quick.args(["-c", "echo memorystatus"]);
        assert_eq!(
            output_within(quick, std::time::Duration::from_secs(5)).as_deref(),
            Some("memorystatus\n")
        );
    }

    #[test]
    fn a_python_memory_error_is_out_of_memory() {
        let log = lines("Traceback (most recent call last):\n  File \"mlx_worker.py\", line 9, in main\nMemoryError");
        for phase in [FailurePhase::Loading, FailurePhase::Running] {
            let f = classify(&log, Some(1), phase);
            assert_eq!(f.kind, FailureKind::OutOfMemory, "{f:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn unix_signal_fills_exit_and_keeps_kind() {
        use std::os::unix::process::ExitStatusExt;
        let status = std::process::ExitStatus::from_raw(6);
        let mut f = classify(
            &lines("ggml_metal_buffer_init: error: failed to allocate buffer, size = 9000.00 MiB"),
            status.code(),
            FailurePhase::Loading,
        );
        note_signal(&mut f, &status);
        assert_eq!(f.kind, FailureKind::OutOfMemory);
        assert_eq!(f.params["exit"], "SIGABRT (6)");
        assert!(!f.params.contains_key("signal"));

        // An unexplained SIGKILL came from outside: its own message, the kind unchanged until
        // the system's log confirms a memory kill.
        for (phase, kind) in [
            (FailurePhase::Running, FailureKind::Crashed),
            (FailurePhase::Loading, FailureKind::StartFailed),
        ] {
            let status = std::process::ExitStatus::from_raw(9);
            let mut f = classify(&[], status.code(), phase);
            note_signal(&mut f, &status);
            assert_eq!(f.kind, kind);
            assert_eq!(f.message_key, "failure.killed");
            assert_eq!(f.params["exit"], "SIGKILL (9)");
            assert!(!f.params.contains_key("signal"));
        }
        // One the log explains keeps its own kind.
        let status = std::process::ExitStatus::from_raw(9);
        let mut f = classify(
            &lines("error: invalid argument: --foo"),
            status.code(),
            FailurePhase::Loading,
        );
        note_signal(&mut f, &status);
        assert_eq!(f.kind, FailureKind::InvalidArgument);
        assert_ne!(f.message_key, "failure.killed");

        let status = std::process::ExitStatus::from_raw(11);
        let mut f = classify(&[], status.code(), FailurePhase::Running);
        note_signal(&mut f, &status);
        assert_eq!(f.params["exit"], "SIGSEGV (11)");
        assert_eq!(f.params["signal"], "access_violation");

        // A normal exit code is left to `classify`.
        let status = std::process::ExitStatus::from_raw(3 << 8);
        let mut f = classify(&[], status.code(), FailurePhase::Loading);
        note_signal(&mut f, &status);
        assert_eq!(f.params["exit"], "3");
    }

    #[test]
    fn real_b11333_draft_failures_are_classified() {
        // Qwen3-4B Q4_K_M -ngl 99 -c 8192 + Qwen3-0.6B Q8_0 -ngld 99 on a 4 GB RTX 3050 Ti.
        let f = classify(
            &fixture("b11333-cuda-draft_no_fit.log"),
            Some(1),
            FailurePhase::Loading,
        );
        assert_eq!(f.kind, FailureKind::DraftLoadFailed, "{f:?}");
        assert_eq!(f.code, "servers.draft_load_failed");
        assert_eq!(f.params["cause"], "memory");
        assert_eq!(f.params["draft"], "C:\\models\\Qwen3-0.6B-Q8_0.gguf");
        assert_eq!(f.evidence.len(), 2, "{:?}", f.evidence);
        assert!(f.evidence[0].contains("invalid vector subscript"));
        // A draft path that does not exist: not "model not found" (the target loaded).
        let f = classify(
            &fixture("b11333-cuda-draft_missing.log"),
            Some(1),
            FailurePhase::Loading,
        );
        assert_eq!(f.kind, FailureKind::DraftLoadFailed);
        assert_eq!(f.params["cause"], "not_found");
        assert_eq!(f.params["draft"], "C:\\models\\nope-draft.gguf");
    }

    #[test]
    fn lost_gpu_is_classified() {
        // ggml-cuda's format (`CUDA error: %s`, `current device: %d, in function %s at %s:%d`,
        // strings of ggml-cuda.dll b11327) with the cudart text of a TDR during a kernel.
        let tdr = "ggml_cuda_compute_forward: MUL_MAT failed\n\
CUDA error: the launch timed out and was terminated\n  \
current device: 0, in function ggml_cuda_compute_forward at ggml-cuda.cu:2790\n  \
err\n\
ggml-cuda.cu:96: CUDA error";
        let f = classify(&lines(tdr), Some(-1073740791), FailurePhase::Running);
        assert_eq!(f.kind, FailureKind::DeviceLost, "{f:?}");
        assert_eq!(f.code, "servers.device_lost");
        assert_eq!(f.params["cause"], "driver_reset");
        assert_eq!(
            f.params["detail"],
            "the launch timed out and was terminated"
        );
        assert_eq!(f.params["exit"], "0xC0000409");
        assert_eq!(
            f.evidence,
            vec![
                "CUDA error: the launch timed out and was terminated",
                "current device: 0, in function ggml_cuda_compute_forward at ggml-cuda.cu:2790",
            ]
        );

        let f = classify(
            &lines("CUDA error: an illegal memory access was encountered"),
            Some(3),
            FailurePhase::Running,
        );
        assert_eq!(f.kind, FailureKind::DeviceLost);
        assert_eq!(f.params["cause"], "kernel_fault");

        for text in [
            "CUDA error: unspecified launch failure",
            "ggml_vulkan: vk::Queue::submit: ErrorDeviceLost",
            "error: Caused GPU Timeout Error (00000002:kIOGPUCommandBufferCallbackErrorTimeout)",
        ] {
            let f = classify(&lines(text), Some(1), FailurePhase::Running);
            assert_eq!(f.kind, FailureKind::DeviceLost, "{text}");
        }

        // An allocation failure through the same CUDA error path stays out of memory.
        let f = classify(
            &lines("CUDA error: out of memory\n  current device: 0, in function alloc at x.cu:1"),
            Some(1),
            FailurePhase::Loading,
        );
        assert_eq!(f.kind, FailureKind::OutOfMemory);
    }

    #[test]
    fn metal_out_of_memory_is_seen_while_the_server_runs() {
        let l = "[stderr] error: Insufficient Memory (00000008:kIOGPUCommandBufferCallbackErrorOutOfMemory)";
        assert!(metal_out_of_memory(l));
        assert_eq!(
            classify(&[l.into()], None, FailurePhase::Running).kind,
            FailureKind::OutOfMemory
        );
        // The generic command buffer failure and llama-fit's refusal are not this error.
        assert!(!metal_out_of_memory(
            "ggml_metal_synchronize: error: command buffer 0 failed with status 5"
        ));
        assert!(!metal_out_of_memory(
            "common_fit_params: failed to fit params to free device memory: n_gpu_layers already set by user to 99, abort"
        ));
    }

    #[test]
    fn codes_are_stable() {
        let codes: Vec<String> = FailureKind::ALL.iter().map(|k| k.code()).collect();
        assert_eq!(
            codes,
            [
                "servers.out_of_memory",
                "servers.missing_library",
                "servers.corrupt_model",
                "servers.unsupported_architecture",
                "servers.model_not_found",
                "servers.port_in_use",
                "servers.invalid_argument",
                "servers.load_timeout",
                "servers.crashed",
                "servers.start_failed",
                "servers.device_lost",
                "servers.draft_load_failed",
            ]
        );
        let json = serde_json::to_value(ServerFailure::new(
            FailureKind::PortInUse,
            FailurePhase::Loading,
        ))
        .unwrap();
        assert_eq!(json["kind"], "port_in_use");
        assert_eq!(json["phase"], "loading");
    }
}
