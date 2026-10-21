//! Port policy: users should not have to think about ports.
//!
//! * **llama-server instances** without a port of their own (`LaunchConfig::port_auto`, or
//!   port 0) get one from Harness's range [`AUTO_RANGE`] (11440–11499); an isolated run (a data
//!   directory other than the regular install's, see [`crate::paths::is_isolated`]) uses
//!   [`ISOLATED_AUTO_RANGE`] instead, so it never takes the real install's ports. The range holds none
//!   of the ports other software usually takes ([`WELL_KNOWN`]: 8080, 3000, 5000, 5173, 8000,
//!   8888, Ollama's 11434, LM Studio's 1234, Harness's own 11435/11500...). An automatic port
//!   that turns out to be taken (by the time llama-server binds it) is replaced silently; an
//!   explicitly chosen one that is taken is a clear error naming the program that holds it.
//! * **The gateway and the Control API** start on a fallback port when their configured port is
//!   held by another program ([`fallback_port`]); the choice is remembered in
//!   `config/ports.json` ([`PortState`]) so it stays the same across restarts, and every client
//!   config Harness writes follows the port actually in use.
//! * [`holder`] names the process listening on a port (Windows `GetExtendedTcpTable`, Linux
//!   `/proc/net/tcp*` + `/proc/<pid>/fd`, macOS `lsof`).

use serde::{Deserialize, Serialize};
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Ports Harness hands out to llama-server instances on its own.
pub const AUTO_RANGE: RangeInclusive<u16> = 11440..=11499;

/// [`AUTO_RANGE`] of an isolated run (`HARNESS_DATA_DIR`, test builds, tests): clear of the
/// regular install's range and of Harness's fixed ports.
pub const ISOLATED_AUTO_RANGE: RangeInclusive<u16> = 12440..=12499;

/// The range automatic ports come from in this process (see [`ISOLATED_AUTO_RANGE`]).
pub fn auto_range() -> RangeInclusive<u16> {
    range_for(crate::paths::is_isolated())
}

fn range_for(isolated: bool) -> RangeInclusive<u16> {
    if isolated {
        ISOLATED_AUTO_RANGE
    } else {
        AUTO_RANGE
    }
}

/// A port Harness may hand out on its own, in either range.
pub fn is_auto(port: u16) -> bool {
    AUTO_RANGE.contains(&port) || ISOLATED_AUTO_RANGE.contains(&port)
}

/// Ports other software commonly uses (dev servers, databases, other local LLM tools) and
/// Harness's own fixed ports. Never chosen automatically.
pub const WELL_KNOWN: &[u16] = &[
    80,
    443,
    1234, // LM Studio
    3000,
    3001,
    4000,
    5000,
    5001,
    5173, // web dev servers
    5432,
    6379,
    8000,
    8008,
    8080,
    8081,
    8443,
    8888,
    9000,  // databases, app servers, Jupyter
    9931,  // llama-server's announced future default port
    11434, // Ollama
    crate::gateway::DEFAULT_PORT,
    crate::control_api::DEFAULT_PORT,
    27017,
];

/// The neighborhood the tests expect to be searched first (the ports right above the configured
/// one, in order).
#[cfg(test)]
const FALLBACK_SPAN: u16 = 100;

pub fn is_well_known(port: u16) -> bool {
    WELL_KNOWN.contains(&port)
}

/// First free port of [`auto_range`] for `host`: `preferred` (the instance's last automatic
/// port) when it is in the range and free, else the first free one not in `taken` (ports of
/// other running instances and saved profiles). `None` when the whole range is taken.
pub fn pick_auto(host: &str, preferred: Option<u16>, taken: &[u16]) -> Option<u16> {
    pick_in(auto_range(), host, preferred, taken)
}

fn pick_in(
    range: RangeInclusive<u16>,
    host: &str,
    preferred: Option<u16>,
    taken: &[u16],
) -> Option<u16> {
    let usable = |p: u16| {
        range.contains(&p)
            && !is_well_known(p)
            && !taken.contains(&p)
            && crate::servers::ensure_port_free(host, p).is_ok()
    };
    if let Some(p) = preferred.filter(|p| usable(*p)) {
        return Some(p);
    }
    range.clone().find(|p| usable(*p))
}

/// A free port for a listener whose configured port is taken: `remembered` (the fallback of the
/// last run) when it is free, else the first free port above `configured`, then (wrapping
/// around) the unprivileged ports below it. Never well known, in an automatic range
/// ([`is_auto`]: llama-server instances), or in `avoid`.
pub fn fallback_port(
    host: &str,
    configured: u16,
    remembered: Option<u16>,
    avoid: &[u16],
) -> Option<u16> {
    fallback_candidates(host, configured, remembered, avoid).next()
}

fn fallback_candidates<'a>(
    host: &'a str,
    configured: u16,
    remembered: Option<u16>,
    avoid: &'a [u16],
) -> impl Iterator<Item = u16> + 'a {
    candidates_where(configured, remembered, avoid, move |p| {
        crate::servers::ensure_port_free(host, p).is_ok()
    })
}

/// [`fallback_candidates`] with the "is this port free" probe injected, so the order and the
/// exclusions can be tested without depending on what else is listening on the machine.
fn candidates_where<'a>(
    configured: u16,
    remembered: Option<u16>,
    avoid: &'a [u16],
    is_free: impl Fn(u16) -> bool + 'a,
) -> impl Iterator<Item = u16> + 'a {
    // Windows can exclude a whole 100-port block immediately above an ephemeral port
    // (`netsh int ipv4 show excludedportrange`). Exhausting that neighborhood must not hide
    // usable ports elsewhere: keep going upward, then wrap to the low unprivileged ports. Each
    // port is visited once, including when configured is u16::MAX.
    let above = (configured.saturating_add(1)..=u16::MAX).filter(move |&p| p > configured);
    let below = 1024..configured;
    remembered
        .into_iter()
        .chain(above.chain(below).filter(move |p| Some(*p) != remembered))
        .filter(move |&p| {
            p >= 1024
                && p != configured
                && !is_well_known(p)
                && !is_auto(p)
                && !avoid.contains(&p)
                && is_free(p)
        })
}

/// Whether a bind error means "the address is taken" (on Windows also `WSAEACCES`, which a port
/// excluded or held exclusively by another process gives).
pub fn is_addr_in_use(e: &std::io::Error) -> bool {
    e.kind() == std::io::ErrorKind::AddrInUse
        || (cfg!(windows) && e.kind() == std::io::ErrorKind::PermissionDenied)
}

/// A listener that could not use its configured port.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PortFallback {
    /// The configured port, held by another program.
    pub configured: u16,
    /// That program, when it could be found.
    #[serde(default)]
    pub holder: Option<PortHolder>,
}

/// Binds `host:configured`, or — when another program holds it — a fallback port
/// ([`fallback_port`]: `remembered` first, never a port in `avoid`). The configured port is
/// checked like a server port first ([`crate::servers::ensure_port_free`]): on Windows a bind
/// can succeed next to another program's wildcard listener and would split its traffic.
/// `avoid = None`: no fallback (a port given explicitly, e.g. `harness serve --port`); a taken
/// port is then an `AddrInUse` error naming the program that holds it.
pub async fn bind_with_fallback(
    host: &str,
    configured: u16,
    remembered: Option<u16>,
    avoid: Option<Vec<u16>>,
) -> std::io::Result<(tokio::net::TcpListener, Option<PortFallback>)> {
    let check_host = host.to_string();
    let free = tokio::task::spawn_blocking(move || {
        crate::servers::ensure_port_free(&check_host, configured).is_ok()
    })
    .await
    .unwrap_or(true);
    if free {
        match tokio::net::TcpListener::bind((host, configured)).await {
            Ok(l) => return Ok((l, None)),
            Err(e) if !is_addr_in_use(&e) => return Err(e),
            Err(_) => {}
        }
    }
    let h = host.to_string();
    let (holder, mut port, mut avoid) = tokio::task::spawn_blocking(move || {
        let port = avoid
            .as_deref()
            .and_then(|avoid| fallback_port(&h, configured, remembered, avoid));
        (holder(configured), port, avoid)
    })
    .await
    .map_err(std::io::Error::other)?;
    while let Some(candidate) = port {
        match tokio::net::TcpListener::bind((host, candidate)).await {
            Ok(listener) => return Ok((listener, Some(PortFallback { configured, holder }))),
            Err(e) if !is_addr_in_use(&e) => return Err(e),
            Err(_) => {}
        }
        let avoided = avoid.as_mut().expect("fallback requires an avoid list");
        avoided.push(candidate);
        let avoided = avoided.clone();
        let h = host.to_string();
        port = tokio::task::spawn_blocking(move || {
            fallback_candidates(&h, configured, remembered, &avoided).next()
        })
        .await
        .map_err(std::io::Error::other)?;
    }
    let by = holder
        .map(|h| format!(" by '{}' (PID {})", h.label(), h.pid))
        .unwrap_or_default();
    Err(std::io::Error::new(
        std::io::ErrorKind::AddrInUse,
        format!("port {} is in use{}", configured, by),
    ))
}

/// The port the Control API of this process listens on (0 = not running).
static LIVE_API_PORT: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(0);

/// Records the Control API's listening port (`None` when it stops).
pub fn set_live_api_port(port: Option<u16>) {
    LIVE_API_PORT.store(port.unwrap_or(0), std::sync::atomic::Ordering::SeqCst);
}

/// The Control API's listening port in this process, while it runs.
pub fn live_api_port() -> Option<u16> {
    Some(LIVE_API_PORT.load(std::sync::atomic::Ordering::SeqCst)).filter(|p| *p != 0)
}

// --- remembered fallback ports ---

/// `config/ports.json`: fallback ports the gateway and the Control API used because their
/// configured port was taken (the next start tries the same one first).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PortState {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gateway: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub control_api: Option<u16>,
    /// Every port the gateway has listened on (newest last, at most 8): client configs that
    /// still name one of them were written by Harness for the gateway and are moved to the
    /// current port automatically.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub gateway_history: Vec<u16>,
    /// Every port the Control API has listened on (newest last, at most 8): Harness MCP HTTP
    /// entries that still name one of them are moved to the current port automatically.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub control_api_history: Vec<u16>,
}

fn state_file(data_dir: &Path) -> PathBuf {
    data_dir.join("config").join("ports.json")
}

fn state_lock() -> &'static Mutex<()> {
    static LOCK: Mutex<()> = Mutex::new(());
    &LOCK
}

impl PortState {
    pub fn load(data_dir: &Path) -> PortState {
        crate::store::JsonStore::new(state_file(data_dir)).load_or_default()
    }

    /// Reads, changes and writes `ports.json` (best effort: a failed write is logged).
    pub fn update(data_dir: &Path, f: impl FnOnce(&mut PortState)) -> PortState {
        let _g = state_lock().lock().unwrap_or_else(|e| e.into_inner());
        let mut s = Self::load(data_dir);
        let before = s.clone();
        f(&mut s);
        if s != before {
            if let Err(e) = crate::store::JsonStore::new(state_file(data_dir)).save(&s) {
                eprintln!("[Harness Studio] ports.json yazılamadı: {}", e);
            }
        }
        s
    }

    /// Records the port the gateway listens on now.
    pub fn record_gateway_port(&mut self, port: u16) {
        self.gateway_history.retain(|p| *p != port);
        self.gateway_history.push(port);
        let extra = self.gateway_history.len().saturating_sub(8);
        self.gateway_history.drain(..extra);
    }

    /// Records the port the Control API listens on now.
    pub fn record_api_port(&mut self, port: u16) {
        self.control_api_history.retain(|p| *p != port);
        self.control_api_history.push(port);
        let extra = self.control_api_history.len().saturating_sub(8);
        self.control_api_history.drain(..extra);
    }
}

// --- who holds a port ---

/// The process listening on a port.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PortHolder {
    pub pid: u32,
    /// Executable name (`ollama.exe`, `node`...); empty when the process could not be read.
    pub name: String,
}

impl PortHolder {
    /// The name to show: the executable, else `PID <n>`.
    pub fn label(&self) -> String {
        if self.name.is_empty() {
            format!("PID {}", self.pid)
        } else {
            self.name.clone()
        }
    }
}

/// The process listening on TCP `port` (any address), if it can be found.
pub fn holder(port: u16) -> Option<PortHolder> {
    let pid = listener_pid(port)?;
    Some(PortHolder {
        pid,
        name: process_name(pid).unwrap_or_default(),
    })
}

/// The error of a taken port: `servers.port_in_use_by {port, program, pid}` when the program
/// holding it is found ("8080 portunu 'ollama.exe' kullanıyor"), else
/// `servers.port_in_use_at {port}`.
pub fn in_use_error(port: u16) -> String {
    use crate::error_key::err;
    match holder(port) {
        Some(h) => err("servers.port_in_use_by")
            .num("port", port)
            .with("program", h.label())
            .num("pid", h.pid)
            .into(),
        None => err("servers.port_in_use_at").num("port", port).into(),
    }
}

fn process_name(pid: u32) -> Option<String> {
    #[cfg(target_os = "linux")]
    if let Ok(comm) = std::fs::read_to_string(format!("/proc/{}/comm", pid)) {
        let comm = comm.trim();
        if !comm.is_empty() {
            return Some(comm.to_string());
        }
    }
    let mut sys = sysinfo::System::new();
    let target = sysinfo::Pid::from_u32(pid);
    sys.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::Some(&[target]),
        true,
        sysinfo::ProcessRefreshKind::nothing(),
    );
    sys.process(target)
        .map(|p| p.name().to_string_lossy().to_string())
        .filter(|n| !n.is_empty())
}

#[cfg(windows)]
fn listener_pid(port: u16) -> Option<u32> {
    use windows::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID,
        TCP_TABLE_OWNER_PID_LISTENER,
    };
    use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6};

    /// The table of one address family as raw bytes (`dwNumEntries` then the rows).
    fn table(family: u16) -> Option<Vec<u8>> {
        let mut size: u32 = 0;
        // SAFETY: a null buffer asks for the needed size only.
        unsafe {
            GetExtendedTcpTable(
                None,
                &mut size,
                false,
                family as u32,
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            )
        };
        for _ in 0..3 {
            if size == 0 {
                return None;
            }
            // u32 alignment for the rows.
            let mut buf = vec![0u32; (size as usize).div_ceil(4) + 1];
            let mut len = (buf.len() * 4) as u32;
            // SAFETY: `buf` holds `len` writable bytes.
            let rc = unsafe {
                GetExtendedTcpTable(
                    Some(buf.as_mut_ptr().cast()),
                    &mut len,
                    false,
                    family as u32,
                    TCP_TABLE_OWNER_PID_LISTENER,
                    0,
                )
            };
            if rc == 0 {
                let bytes: Vec<u8> = buf.iter().flat_map(|w| w.to_ne_bytes()).collect();
                return Some(bytes);
            }
            // ERROR_INSUFFICIENT_BUFFER (122): the table grew in between.
            if rc != 122 {
                return None;
            }
            size = len;
        }
        None
    }

    fn rows<T: Copy>(bytes: &[u8]) -> Vec<T> {
        let count = u32::from_ne_bytes(bytes[..4].try_into().unwrap()) as usize;
        let row = std::mem::size_of::<T>();
        // The rows start after `dwNumEntries`, aligned for `T` (4 bytes for both row types).
        (0..count)
            .filter_map(|i| {
                let start = 4 + i * row;
                let slice = bytes.get(start..start + row)?;
                // SAFETY: `T` is a plain `repr(C)` row of integers; read unaligned.
                Some(unsafe { std::ptr::read_unaligned(slice.as_ptr().cast::<T>()) })
            })
            .collect()
    }

    let net_port = |p: u32| u16::from_be((p & 0xFFFF) as u16);
    if let Some(bytes) = table(AF_INET.0) {
        if let Some(r) = rows::<MIB_TCPROW_OWNER_PID>(&bytes)
            .into_iter()
            .find(|r| net_port(r.dwLocalPort) == port)
        {
            return Some(r.dwOwningPid);
        }
    }
    let bytes = table(AF_INET6.0)?;
    rows::<MIB_TCP6ROW_OWNER_PID>(&bytes)
        .into_iter()
        .find(|r| net_port(r.dwLocalPort) == port)
        .map(|r| r.dwOwningPid)
}

#[cfg(target_os = "linux")]
fn listener_pid(port: u16) -> Option<u32> {
    let inodes: Vec<String> = ["/proc/net/tcp", "/proc/net/tcp6"]
        .iter()
        .filter_map(|f| std::fs::read_to_string(f).ok())
        .flat_map(|text| listening_inodes(&text, port))
        .collect();
    if inodes.is_empty() {
        return None;
    }
    let targets: Vec<String> = inodes.iter().map(|i| format!("socket:[{}]", i)).collect();
    for entry in std::fs::read_dir("/proc").ok()?.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(fds) = std::fs::read_dir(entry.path().join("fd")) else {
            continue;
        };
        for fd in fds.flatten() {
            if let Ok(link) = std::fs::read_link(fd.path()) {
                if targets.iter().any(|t| link.as_os_str() == t.as_str()) {
                    return Some(pid);
                }
            }
        }
    }
    None
}

/// Inodes of the sockets listening on `port` in a `/proc/net/tcp` table.
#[cfg(any(target_os = "linux", test))]
fn listening_inodes(table: &str, port: u16) -> Vec<String> {
    table
        .lines()
        .skip(1)
        .filter_map(|line| {
            let cols: Vec<&str> = line.split_whitespace().collect();
            let local = cols.get(1)?;
            let state = cols.get(3)?;
            let inode = cols.get(9)?;
            let p = u16::from_str_radix(local.rsplit(':').next()?, 16).ok()?;
            (p == port && *state == "0A" && *inode != "0").then(|| inode.to_string())
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn listener_pid(port: u16) -> Option<u32> {
    let out = crate::process::quiet_command("lsof")
        .args(["-nP", &format!("-iTCP:{}", port), "-sTCP:LISTEN", "-Fp"])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|l| l.strip_prefix('p').and_then(|p| p.trim().parse().ok()))
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn listener_pid(_port: u16) -> Option<u32> {
    None
}

// --- Ollama ---

/// Whether Ollama is installed on this computer (its program is on `PATH` or in its default
/// install location). The gateway never takes Ollama's port then, even while Ollama is not
/// running: starting Ollama later would fail.
pub fn ollama_installed() -> bool {
    let exe = if cfg!(windows) {
        "ollama.exe"
    } else {
        "ollama"
    };
    let on_path = crate::integrations::launch::find_file_on_path(exe).is_some();
    if on_path {
        return true;
    }
    let mut candidates: Vec<PathBuf> = Vec::new();
    if cfg!(windows) {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            candidates.push(PathBuf::from(local).join("Programs/Ollama/ollama.exe"));
        }
    } else {
        candidates.push(PathBuf::from("/usr/local/bin/ollama"));
        candidates.push(PathBuf::from("/usr/bin/ollama"));
        candidates.push(PathBuf::from("/Applications/Ollama.app"));
    }
    candidates.iter().any(|p| p.exists())
}

/// Whether the process holding a port is Ollama.
pub fn is_ollama(holder: &PortHolder) -> bool {
    holder.name.to_ascii_lowercase().starts_with("ollama")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn the_auto_range_holds_no_well_known_port() {
        for p in WELL_KNOWN {
            assert!(!is_auto(*p), "{p}");
        }
        assert!(is_well_known(8080) && is_well_known(11434) && is_well_known(1234));
        assert!(is_well_known(crate::gateway::DEFAULT_PORT));
        assert!(is_well_known(crate::control_api::DEFAULT_PORT));
    }

    #[test]
    fn auto_ports_come_from_the_range_and_skip_taken_ones() {
        // Hold one port of the range (when it is free on this machine) and reserve another.
        let held = auto_range()
            .clone()
            .find_map(|p| TcpListener::bind(("127.0.0.1", p)).ok());
        let held_port = held.as_ref().map(|l| l.local_addr().unwrap().port());
        let first = pick_auto("127.0.0.1", None, &[]).unwrap();
        assert!(auto_range().contains(&first));
        assert_ne!(Some(first), held_port);
        let second = pick_auto("127.0.0.1", None, &[first]).unwrap();
        assert!(auto_range().contains(&second) && second != first);
        // The preferred (sticky) port is kept while it is free, ignored outside the range.
        assert_eq!(pick_auto("127.0.0.1", Some(second), &[]), Some(second));
        assert_eq!(pick_auto("127.0.0.1", Some(8080), &[]), Some(first));
        // A held preferred port is replaced.
        if let Some(h) = held_port {
            let got = pick_auto("127.0.0.1", Some(h), &[]).unwrap();
            assert_ne!(got, h);
        }
    }

    #[test]
    fn isolated_runs_take_their_ports_from_a_range_of_their_own() {
        // Unit tests never call `paths::init`: their data root is a temp dir, an isolated run.
        assert!(crate::paths::is_isolated());
        assert_eq!(auto_range(), ISOLATED_AUTO_RANGE);
        assert_eq!(range_for(false), AUTO_RANGE);
        // Clear of the regular install's range and of every fixed port.
        for p in ISOLATED_AUTO_RANGE {
            assert!(!AUTO_RANGE.contains(&p) && !is_well_known(p), "{p}");
        }
        let got = pick_auto("127.0.0.1", Some(*AUTO_RANGE.start()), &[]).unwrap();
        assert!(ISOLATED_AUTO_RANGE.contains(&got), "{got}");
        assert!(is_auto(got) && is_auto(*AUTO_RANGE.start()));
        // The regular install keeps its range.
        let regular = pick_in(range_for(false), "127.0.0.1", None, &[]).unwrap();
        assert!(AUTO_RANGE.contains(&regular), "{regular}");
    }

    #[test]
    fn fallback_skips_the_configured_port_ranges_and_avoided_ports() {
        // Probing a port connects first. Keep this test below the ephemeral source-port
        // range, so those probes cannot occupy the fallback being checked next.
        let listener = (20000..27000)
            .find_map(|port| TcpListener::bind(("127.0.0.1", port)).ok())
            .expect("a free test port below the ephemeral range");
        let configured = listener.local_addr().unwrap().port();
        let got = fallback_port("127.0.0.1", configured, None, &[configured + 1]).unwrap();
        assert!(got > configured + 1 && got <= configured + FALLBACK_SPAN);
        assert!(!is_well_known(got) && !AUTO_RANGE.contains(&got));
        // The remembered fallback wins while it is free.
        let again = fallback_port("127.0.0.1", configured, Some(got), &[]).unwrap();
        assert_eq!(again, got);
        // The gateway's default: its fallbacks never land in the llama-server range or on 11500.
        let gw = crate::gateway::DEFAULT_PORT;
        if let Some(p) = fallback_port("127.0.0.1", gw, None, &[]) {
            assert!(p != gw && !AUTO_RANGE.contains(&p) && p != crate::control_api::DEFAULT_PORT);
        }
    }

    #[test]
    fn fallback_candidates_skip_taken_and_reserved_ports_in_order() {
        let configured = 11433;
        // A fixed "taken" port instead of a live listener: other tests and programs bind ports
        // in parallel, so two live probes may disagree.
        let taken = 11435;
        let avoided = 11501;
        let free = move |p: u16| p != taken;
        let remembered = 11502;
        let candidates: Vec<_> =
            candidates_where(configured, Some(remembered), &[avoided], free).collect();
        assert_eq!(candidates.first(), Some(&remembered));
        assert_eq!(candidates.iter().filter(|&&p| p == remembered).count(), 1);
        let nearby: Vec<_> = candidates[1..]
            .iter()
            .copied()
            .filter(|&p| p > configured && p <= configured + FALLBACK_SPAN)
            .collect();
        assert!(nearby.windows(2).all(|ports| ports[0] < ports[1]));
        assert!(candidates.iter().all(|&p| {
            p >= 1024
                && p != configured
                && p != taken
                && p != avoided
                && !is_auto(p)
                && !is_well_known(p)
        }));
        let retried: Vec<_> =
            candidates_where(configured, Some(remembered), &[avoided, remembered], free).collect();
        assert_eq!(retried, candidates[1..]);
        assert!(candidates_where(1000, Some(80), &[], |_| true).all(|p| p >= 1024));
        assert_eq!(
            candidates_where(u16::MAX, None, &[], |_| true).next(),
            Some(1024)
        );
        // The live probe is still used by the real entry point.
        let held = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = held.local_addr().unwrap().port();
        assert_ne!(
            fallback_candidates("127.0.0.1", port - 1, Some(port), &[]).next(),
            Some(port)
        );
    }

    #[test]
    fn fallback_searches_beyond_an_exhausted_neighborhood() {
        // The reported Windows failure: configured=49690, excluded=49691..=49790.
        let configured = 49690;
        let excluded = configured + 1..=configured + FALLBACK_SPAN;
        let usable = 49791;
        assert_eq!(
            candidates_where(configured, None, &[], |p| p == usable).next(),
            Some(usable)
        );
        // Past the excluded block the search keeps going upward before wrapping around.
        assert_eq!(
            candidates_where(configured, None, &[], |p| !excluded.contains(&p)).next(),
            Some(usable)
        );
        assert_eq!(
            candidates_where(configured, None, &[], |p| p < configured).next(),
            Some(1024)
        );
        assert!(
            candidates_where(configured, None, &[usable], |p| p == usable)
                .next()
                .is_none()
        );
        assert!(candidates_where(configured, None, &[], |_| false)
            .next()
            .is_none());
        let candidates: Vec<_> =
            candidates_where(configured, Some(usable), &[], |_| true).collect();
        let unique: std::collections::HashSet<_> = candidates.iter().copied().collect();
        assert_eq!(unique.len(), candidates.len(), "no duplicate retries");
        assert_eq!(candidates[0], usable, "remembered port still wins");
        assert_eq!(
            candidates[1],
            configured + 1,
            "nearby ports still come first"
        );
    }

    #[test]
    fn the_holder_of_a_listening_port_is_this_process() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let h = holder(port).expect("listener found");
        assert_eq!(h.pid, std::process::id());
        let me = std::env::current_exe().unwrap();
        let stem = me.file_stem().unwrap().to_string_lossy().to_lowercase();
        assert!(
            h.name
                .to_lowercase()
                .starts_with(&stem[..stem.len().min(15)]),
            "{h:?} vs {stem}"
        );
        assert!(!is_ollama(&h));
    }

    #[tokio::test]
    async fn a_taken_port_falls_back_and_names_the_holder() {
        let other = TcpListener::bind("127.0.0.1:0").unwrap();
        let configured = other.local_addr().unwrap().port();
        // Force the real bind path past its preferred neighborhood as well.
        let avoided: Vec<_> =
            (configured.saturating_add(1)..=configured.saturating_add(FALLBACK_SPAN)).collect();
        let (l, fb) = bind_with_fallback("127.0.0.1", configured, None, Some(avoided.clone()))
            .await
            .unwrap();
        let port = l.local_addr().unwrap().port();
        assert_ne!(port, configured);
        assert!(!avoided.contains(&port));
        let fb = fb.expect("fell back");
        assert_eq!(fb.configured, configured);
        assert_eq!(fb.holder.map(|h| h.pid), Some(std::process::id()));
        // Without a fallback (a port given explicitly) the error names the holder.
        let e = bind_with_fallback("127.0.0.1", configured, None, None)
            .await
            .unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::AddrInUse);
        assert!(
            e.to_string()
                .contains(&format!("(PID {})", std::process::id())),
            "{e}"
        );
        // Free again: the configured port is used, no fallback. A test running in parallel can
        // be handed the just-freed port by its own `bind(0)`; then try with another free port.
        drop(other);
        drop(l);
        let mut configured = configured;
        for attempt in 0.. {
            let (l, fb) = bind_with_fallback("127.0.0.1", configured, Some(port), Some(vec![]))
                .await
                .unwrap();
            if fb.is_none() {
                assert_eq!(l.local_addr().unwrap().port(), configured);
                break;
            }
            assert!(attempt < 5, "port {configured} was taken every time");
            drop(l);
            configured = TcpListener::bind("127.0.0.1:0")
                .unwrap()
                .local_addr()
                .unwrap()
                .port();
        }
    }

    #[test]
    fn proc_net_tcp_rows_are_parsed() {
        let table = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n   0: 0100007F:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 4242 1 0000000000000000 100 0 0 10 0\n   1: 0100007F:1F90 0100007F:D431 01 00000000:00000000 00:00000000 00000000  1000        0 4343 1 0000000000000000 20 4 30 10 -1\n";
        assert_eq!(listening_inodes(table, 8080), ["4242"]);
        assert!(listening_inodes(table, 8081).is_empty());
    }

    #[test]
    fn port_state_round_trips_and_keeps_a_short_history() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(PortState::load(dir.path()), PortState::default());
        PortState::update(dir.path(), |s| {
            s.gateway = Some(11437);
            for p in 11430..11445 {
                s.record_gateway_port(p);
            }
            s.record_gateway_port(11437);
        });
        let s = PortState::load(dir.path());
        assert_eq!(s.gateway, Some(11437));
        assert_eq!(s.gateway_history.len(), 8);
        assert_eq!(s.gateway_history.last(), Some(&11437));
        assert_eq!(s.control_api, None);
    }
}
