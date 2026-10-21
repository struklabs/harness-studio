//! Upstream agent adapters (Hermes, OpenClaw) with private Harness history and explicit run
//! grants. Both share the connect/restore protocol, the run machinery and the history; what
//! differs per agent (config format, launch, install) is selected by [`AgentKind`].
use super::openclaw;
use crate::{
    control_api::{Scope, TokenStore},
    store::JsonStore,
    token_policy::{GatewayEndpoint, TokenPolicy},
};
use command_group::CommandGroup;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

/// The upstream agents Harness can connect to a gateway model.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentKind {
    #[default]
    Hermes,
    OpenClaw,
}
impl AgentKind {
    pub fn parse(name: &str) -> Result<Self, String> {
        match name {
            "hermes" => Ok(Self::Hermes),
            "openclaw" => Ok(Self::OpenClaw),
            _ => Err("agents.kind_invalid".into()),
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Hermes => "Hermes Agent",
            Self::OpenClaw => "OpenClaw",
        }
    }
}

struct Probe {
    executable: PathBuf,
    at: Instant,
    output: Option<String>,
}
static PROBE: Mutex<Vec<Probe>> = Mutex::new(Vec::new());
fn probe(exe: &Path) -> Option<String> {
    let mut slots = PROBE.lock().unwrap();
    if let Some(p) = slots
        .iter()
        .find(|p| p.executable == exe && p.at.elapsed() < Duration::from_secs(30))
    {
        return p.output.clone();
    }
    let mut c = if exe
        .file_stem()
        .is_some_and(|n| n.eq_ignore_ascii_case("openclaw"))
    {
        openclaw::launcher(exe)
    } else {
        command(exe)
    };
    c.arg("--version");
    let output = bounded_output(c, Duration::from_secs(60))
        .ok()
        .and_then(|(ok, s)| ok.then_some(s));
    slots.retain(|p| p.executable != exe);
    slots.push(Probe {
        executable: exe.to_path_buf(),
        at: Instant::now(),
        output: output.clone(),
    });
    output
}
static CONNECTION_LOCK: Mutex<()> = Mutex::new(());
static TASK: Mutex<Option<Arc<Run>>> = Mutex::new(None);
static INSTALL_RUN: Mutex<Option<Arc<Run>>> = Mutex::new(None);
static INSTALL_RUN_OPENCLAW: Mutex<Option<Arc<Run>>> = Mutex::new(None);

pub(super) fn home() -> Result<PathBuf, String> {
    if let Some(p) = std::env::var_os("HERMES_HOME") {
        return Ok(p.into());
    }
    #[cfg(windows)]
    if let Some(p) = std::env::var_os("LOCALAPPDATA") {
        return Ok(PathBuf::from(p).join("hermes"));
    }
    dirs::home_dir()
        .map(|p| p.join(".hermes"))
        .ok_or_else(|| "agents.home_missing".into())
}
fn executable_in(paths: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    paths.into_iter().find(|p| p.is_file())
}
pub(super) const HERMES_REVISION: &str = "d923a5eb487a17c0a68801bba9ef0f56c81040a8";

pub fn managed_root() -> PathBuf {
    crate::paths::bin_dir().join("modules/hermes")
}
/// Launchers an upstream install publishes under `<install>/.hermes/bin`. A source install
/// without distlib gets a command file (`hermes.cmd`) instead of `hermes.exe`.
const LAUNCHERS: [&str; 3] = [
    ".hermes/bin/hermes.exe",
    ".hermes/bin/hermes.cmd",
    ".hermes/bin/hermes",
];
fn managed_executable() -> Option<PathBuf> {
    let root = managed_root();
    executable_in(
        LAUNCHERS
            .iter()
            .map(|p| root.join("hermes-agent").join(p))
            .chain(
                [
                    "hermes-agent/venv/Scripts/hermes.exe",
                    "hermes-agent/venv/bin/hermes",
                ]
                .map(|p| root.join(p)),
            ),
    )
}
/// Key files of a Harness-managed install that are gone. Empty for an external install (not
/// ours to judge) and for a folder that was never created.
pub(super) fn missing_files() -> Vec<String> {
    if managed_root().is_dir() && managed_executable().is_none() {
        vec!["hermes".into()]
    } else {
        vec![]
    }
}
pub(super) fn external_runtime() -> Option<PathBuf> {
    external_executable().and_then(|exe| library_entry(&exe).ok().map(|(_, root)| root))
}
/// The data root that holds a new-style external install's dependency environment: the folder
/// with an `installs/<key>/facts.json`. Upstream puts the checkout at `<root>/hermes-agent`, so
/// that parent is tried first, then the platform default home. `None` for an old-style install
/// (a `venv` inside the checkout), which needs no such root.
fn installs_owner(checkout: &Path, default_home: Option<&Path>) -> Option<PathBuf> {
    let has_environment = |root: &Path| {
        std::fs::read_dir(root.join("installs"))
            .into_iter()
            .flatten()
            .flatten()
            .any(|entry| entry.path().join("facts.json").is_file())
    };
    checkout
        .parent()
        .into_iter()
        .chain(default_home)
        .find(|root| has_environment(root))
        .map(Path::to_path_buf)
}
/// Links `link` to the directory `target` (an NTFS junction on Windows, which needs no
/// privilege; a symlink elsewhere). An existing link to another target is replaced.
fn link_dir(target: &Path, link: &Path) -> Result<(), String> {
    if let Ok(meta) = std::fs::symlink_metadata(link) {
        if std::fs::canonicalize(link).ok() == std::fs::canonicalize(target).ok() {
            return Ok(());
        }
        // Only a link is ever replaced; a real folder is not ours to remove.
        if !meta.file_type().is_symlink() && !is_junction(link) {
            return Err("agents.home_unavailable".into());
        }
        std::fs::remove_dir(link)
            .or_else(|_| std::fs::remove_file(link))
            .map_err(|e| e.to_string())?;
    }
    #[cfg(windows)]
    {
        let done = command("cmd")
            .arg("/C")
            .arg("mklink")
            .arg("/J")
            // `mklink` reads a forward slash as an option, so both paths use backslashes.
            .arg(link.to_string_lossy().replace('/', "\\"))
            .arg(target.to_string_lossy().replace('/', "\\"))
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| e.to_string())?;
        if !done.status.success() {
            return Err("agents.home_unavailable".into());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    std::os::unix::fs::symlink(target, link).map_err(|e| e.to_string())
}
fn is_junction(path: &Path) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        std::fs::symlink_metadata(path).is_ok_and(|m| m.file_attributes() & 0x400 != 0)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        false
    }
}
/// A Harness-owned profile root for an external new-style install. Upstream maps
/// `<root>/profiles/<name>` back to `<root>` to find `<root>/installs`, so the root here only
/// holds a link to the install's `installs` folder and the throwaway profiles; the user's own
/// home (config, sessions, credentials) is never used as the task home.
fn external_profiles(owner: &Path) -> Result<PathBuf, String> {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    owner.hash(&mut hasher);
    let root = crate::paths::data_root()
        .join("agents/hermes-external")
        .join(format!("{:016x}", hasher.finish()));
    let profiles = root.join("profiles");
    std::fs::create_dir_all(&profiles).map_err(|e| e.to_string())?;
    link_dir(&owner.join("installs"), &root.join("installs"))?;
    Ok(profiles)
}
/// A clean, throwaway Hermes home. Upstream keeps the dependency environment under
/// `<root>/installs`, so a home that is not a root or one of its profiles cannot find it
/// ("No module named dotenv"). The managed install uses a profile folder of its own root, a
/// new-style external install one under a Harness-owned root that links to its `installs`; an
/// old-style install gets a plain temporary folder.
fn isolated_home(exe: &Path) -> Result<tempfile::TempDir, String> {
    let profiles = if managed_executable().is_some_and(|m| m == exe) {
        let profiles = managed_root().join("profiles");
        std::fs::create_dir_all(&profiles).map_err(|e| e.to_string())?;
        Some(profiles)
    } else {
        match library_entry(exe) {
            Ok((_, checkout)) => match installs_owner(&checkout, home().ok().as_deref()) {
                Some(owner) => Some(external_profiles(&owner)?),
                None => None,
            },
            Err(_) => None,
        }
    };
    match profiles {
        Some(dir) => tempfile::Builder::new()
            .prefix("task-")
            .tempdir_in(dir)
            .map_err(|e| e.to_string()),
        None => tempfile::tempdir().map_err(|e| e.to_string()),
    }
}
pub(super) fn verify() -> Result<(), String> {
    PROBE.lock().unwrap().clear();
    let exe = managed_executable()
        .or_else(executable)
        .ok_or("agents.not_installed")?;
    if probe(&exe).is_none() {
        return Err("agents.runtime_broken".into());
    }
    let (mut c, source) = library_runtime(&exe)?;
    let temporary = isolated_home(&exe)?;
    c.current_dir(temporary.path())
        .env("HERMES_HOME", temporary.path())
        .env("HARNESS_HERMES_SOURCE", source)
        .env("HARNESS_HERMES_VERIFY", "1")
        .env("PYTHONIOENCODING", "utf-8");
    let (ok, _) = bounded_output(c, Duration::from_secs(120))?;
    if ok {
        Ok(())
    } else {
        Err("agents.runtime_broken".into())
    }
}
fn executable() -> Option<PathBuf> {
    managed_executable().or_else(external_executable)
}
fn external_executable() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            if cfg!(windows) {
                candidates.push(dir.join("hermes.exe"));
                candidates.push(dir.join("hermes.cmd"));
            } else {
                candidates.push(dir.join("hermes"));
            }
        }
    }
    if let Ok(dir) = home() {
        for name in [
            "bin/hermes.exe",
            "bin/hermes.cmd",
            "hermes-agent/venv/Scripts/hermes.exe",
            "hermes-agent/venv/bin/hermes",
        ] {
            candidates.push(dir.join(name));
        }
        for name in LAUNCHERS {
            candidates.push(dir.join("hermes-agent").join(name));
        }
    }
    if let Some(dir) = dirs::home_dir() {
        candidates.push(dir.join(".local/bin/hermes"));
    }
    executable_in(candidates)
}
enum Entry {
    Launcher(PathBuf),
    Python(PathBuf),
}
/// Finds the upstream checkout (the folder with `run_agent.py`) and the way to start its
/// Python: the published source launcher or a legacy virtual environment.
fn library_entry(exe: &Path) -> Result<(Entry, PathBuf), String> {
    let mut roots: Vec<PathBuf> = exe.ancestors().skip(1).map(Path::to_path_buf).collect();
    if let Some(output) = probe(exe) {
        for line in output.lines() {
            if let Some(path) = line
                .strip_prefix("Project: ")
                .or_else(|| line.strip_prefix("Install directory: "))
            {
                roots.push(PathBuf::from(path.trim()));
            }
        }
    }
    if let Ok(home) = home() {
        roots.push(home.join("hermes-agent"));
    }
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join(".hermes/hermes-agent"));
    }
    let canonical = std::fs::canonicalize(exe).unwrap_or_else(|_| exe.to_path_buf());
    roots.extend(canonical.ancestors().skip(1).map(Path::to_path_buf));
    for root in roots
        .into_iter()
        .filter(|r| r.join("run_agent.py").is_file())
    {
        for name in LAUNCHERS {
            let launcher = root.join(name);
            if launcher.is_file() {
                return Ok((Entry::Launcher(launcher), root));
            }
        }
        for name in [
            "venv/Scripts/python.exe",
            ".venv/Scripts/python.exe",
            "venv/bin/python",
            ".venv/bin/python",
        ] {
            let python = root.join(name);
            if python.is_file() {
                return Ok((Entry::Python(python), root));
            }
        }
    }
    Err("agents.library_missing".into())
}
type SourceCommand = (PathBuf, Option<std::time::SystemTime>, Vec<String>);
static SOURCE_COMMANDS: Mutex<Vec<SourceCommand>> = Mutex::new(Vec::new());
/// The interpreter command of a source launcher, from its `--print-runtime-command` boundary,
/// with the module entry replaced by the task adapter. Cached per launcher file. Upstream's
/// Windows command file cannot carry a multi-line `-c` script through `%*`, so the adapter
/// goes in as one base64 line.
fn source_command(launcher: &Path) -> Option<Command> {
    use base64::Engine;
    const MODULE: &str = "harness_task";
    let modified = std::fs::metadata(launcher).and_then(|m| m.modified()).ok();
    let cached = SOURCE_COMMANDS
        .lock()
        .unwrap()
        .iter()
        .find(|(p, m, _)| p == launcher && *m == modified)
        .map(|(_, _, args)| args.clone());
    let args = match cached {
        Some(args) => args,
        None => {
            let mut c = command(launcher);
            c.args(["--print-runtime-command", "--module", MODULE]);
            let (ok, output) = bounded_output(c, Duration::from_secs(60)).ok()?;
            let printed: Vec<String> = serde_json::from_str(output.lines().last()?.trim())
                .ok()
                .filter(|_| ok)?;
            let entry =
                format!("runpy.run_module('{MODULE}', run_name='__main__', alter_sys=True)");
            let [python, flag, code, bootstrap] = <[String; 4]>::try_from(printed).ok()?;
            if flag != "-I" || code != "-c" || !bootstrap.ends_with(&entry) {
                return None;
            }
            let args = vec![
                python,
                bootstrap[..bootstrap.len() - entry.len()].to_string(),
            ];
            let mut slot = SOURCE_COMMANDS.lock().unwrap();
            slot.retain(|(p, ..)| p != launcher);
            slot.push((launcher.to_path_buf(), modified, args.clone()));
            args
        }
    };
    let script = base64::engine::general_purpose::STANDARD.encode(include_str!("hermes_task.py"));
    let mut c = command(&args[0]);
    c.args([
        "-I".to_string(),
        "-c".to_string(),
        format!(
            "{}exec(__import__('base64').b64decode('{script}'))",
            args[1]
        ),
    ]);
    Some(c)
}
/// Use upstream's source launcher (PM bootstrap) or its legacy virtual environment.
/// The Python API stays in-process; modern `hermes chat` may detach a gateway daemon.
fn library_runtime(exe: &Path) -> Result<(Command, PathBuf), String> {
    let (entry, root) = library_entry(exe)?;
    match entry {
        Entry::Launcher(launcher) => {
            if let Some(c) = source_command(&launcher) {
                return Ok((c, root));
            }
            // Older launchers take the adapter as an inline script; a command file cannot.
            if launcher
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("cmd"))
            {
                return Err("agents.library_missing".into());
            }
            let mut c = command(launcher);
            c.args(["--run-module", "code", "-c", include_str!("hermes_task.py")]);
            Ok((c, root))
        }
        Entry::Python(python) => {
            let mut c = command(python);
            c.args(["-c", include_str!("hermes_task.py")]);
            Ok((c, root))
        }
    }
}
pub(super) fn command(path: impl AsRef<std::ffi::OsStr>) -> Command {
    let c = Command::new(path);
    #[cfg(windows)]
    let c = {
        use std::os::windows::process::CommandExt;
        let mut c = c;
        c.creation_flags(0x0800_0000);
        c
    };
    c
}
#[derive(Serialize, Clone)]
pub struct Detection {
    pub installed: bool,
    pub version: Option<String>,
    pub executable: Option<String>,
    pub config_path: String,
    pub missing_dependencies: Vec<String>,
    pub connected: bool,
    pub installation: TaskStatus,
    pub model: Option<String>,
    /// The executable is Harness's own install (the only one Harness may remove).
    pub managed: bool,
}
pub fn detect() -> Result<Detection, String> {
    let exe = executable();
    let version = exe
        .as_ref()
        .and_then(|p| probe(p))
        .and_then(|text| text.lines().next().map(str::to_string));
    let mut missing = Vec::new();
    if let Some(exe) = &exe {
        if library_entry(exe).is_err() {
            missing.push("agents.library_missing".into());
        }
    }
    if exe.is_none() {
        for dep in if cfg!(windows) {
            vec!["git", "powershell"]
        } else {
            vec!["git", "bash", "curl", "tar", "shasum"]
        } {
            if command(dep)
                .arg(if dep == "powershell" {
                    "-Help"
                } else {
                    "--version"
                })
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_err()
            {
                missing.push(dep.to_string());
            }
        }
    }
    if exe.is_some() && version.is_none() {
        missing.push("agents.runtime_broken".into());
    }
    let record = record(&home()?)?;
    Ok(Detection {
        installation: INSTALL_RUN
            .lock()
            .unwrap()
            .as_ref()
            .map(|r| r.status.lock().unwrap().clone())
            .unwrap_or_default(),
        installed: version.is_some(),
        version,
        executable: exe.map(|p| p.display().to_string()),
        config_path: home()?.join("config.yaml").display().to_string(),
        missing_dependencies: missing,
        connected: record.is_some(),
        model: record.map(|r| r.model),
        managed: false,
    })
}
/// Detection of the given agent (the Hermes variant is [`detect`]).
pub fn detect_kind(kind: AgentKind) -> Result<Detection, String> {
    match kind {
        AgentKind::Hermes => detect(),
        AgentKind::OpenClaw => detect_openclaw(),
    }
}
fn tool_version(tool: &str) -> Option<String> {
    let mut c = command(tool);
    c.arg("--version");
    bounded_output(c, Duration::from_secs(15))
        .ok()
        .and_then(|(ok, s)| ok.then_some(s))
        .and_then(|s| s.lines().next().map(|l| l.trim().to_string()))
}
fn npm_tool() -> &'static str {
    if cfg!(windows) {
        "npm.cmd"
    } else {
        "npm"
    }
}
fn detect_openclaw() -> Result<Detection, String> {
    let exe = openclaw::executable();
    let version = exe
        .as_ref()
        .and_then(|p| probe(p))
        .and_then(|text| text.lines().next().map(str::to_string));
    let mut missing = Vec::new();
    if exe.is_some() && version.is_none() {
        missing.push("agents.runtime_broken".into());
    }
    if exe.is_none() {
        // Only an install needs them: Node (upstream engines range) and npm.
        match tool_version("node") {
            None => missing.push("node".into()),
            Some(v) if !openclaw::node_supported(&v) => missing.push("agents.node_outdated".into()),
            Some(_) => {}
        }
        if tool_version(npm_tool()).is_none() {
            missing.push("npm".into());
        }
    }
    let config = openclaw::config_path()?;
    let record = record_at(&config)?;
    let managed = exe.is_some() && exe == openclaw::managed_executable();
    Ok(Detection {
        installation: INSTALL_RUN_OPENCLAW
            .lock()
            .unwrap()
            .as_ref()
            .map(|r| r.status.lock().unwrap().clone())
            .unwrap_or_default(),
        installed: version.is_some(),
        version,
        executable: exe.map(|p| p.display().to_string()),
        config_path: config.display().to_string(),
        missing_dependencies: missing,
        connected: record.is_some(),
        model: record.map(|r| r.model),
        managed,
    })
}
fn bounded_output(mut c: Command, timeout: Duration) -> Result<(bool, String), String> {
    c.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    crate::process_limits::prepare(&mut c).map_err(|e| e.to_string())?;
    let mut group = c.group();
    #[cfg(windows)]
    {
        group.kill_on_drop(true);
        group.creation_flags(0x0800_0000);
    }
    let mut child = group.spawn().map_err(|e| e.to_string())?;
    #[cfg(windows)]
    let _memory_job = {
        use std::os::windows::io::AsRawHandle;
        crate::process_limits::memory_job(child.inner().as_raw_handle())
            .map_err(|e| e.to_string())?
    };
    let _group_guard = crate::process_limits::GroupGuard(child.id());
    let stdout = child.inner().stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout.take(65536).read_to_string(&mut s);
        s
    });
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            let _ = child.kill();
            let drain = Instant::now();
            while !reader.is_finished() && drain.elapsed() < Duration::from_millis(1500) {
                std::thread::sleep(Duration::from_millis(10));
            }
            let output = if reader.is_finished() {
                reader.join().unwrap_or_default()
            } else {
                String::new()
            };
            return Ok((status.success(), output));
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err("agents.timeout".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[derive(Serialize, Deserialize)]
struct Connection {
    token_id: String,
    original_exists: bool,
    applied_hash: String,
    model: String,
}
fn record(dir: &Path) -> Result<Option<Connection>, String> {
    let p = dir.join("harness-connection.json");
    if p.exists() {
        JsonStore::sensitive(p).load().map(Some)
    } else {
        Ok(None)
    }
}
/// The journal sits beside the config file it protects.
fn config_dir(config: &Path) -> &Path {
    config.parent().unwrap_or(Path::new("."))
}
fn record_at(config: &Path) -> Result<Option<Connection>, String> {
    record(config_dir(config))
}
fn backup_of(config: &Path) -> PathBuf {
    let mut name = config.as_os_str().to_os_string();
    name.push(".harness.bak");
    name.into()
}
fn config_file(kind: AgentKind) -> Result<PathBuf, String> {
    match kind {
        AgentKind::Hermes => Ok(home()?.join("config.yaml")),
        AgentKind::OpenClaw => openclaw::config_path(),
    }
}
fn parse_config(kind: AgentKind, bytes: &[u8]) -> Result<serde_json::Value, String> {
    match kind {
        AgentKind::Hermes => config(bytes),
        AgentKind::OpenClaw => openclaw::parse_config(bytes),
    }
}
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn config(bytes: &[u8]) -> Result<serde_json::Value, String> {
    let value: serde_json::Value =
        serde_norway::from_slice(bytes).map_err(|_| "agents.config_invalid".to_string())?;
    if value.is_null() {
        return Ok(json!({}));
    }
    if !value.is_object() {
        return Err("agents.config_invalid".into());
    }
    Ok(value)
}
fn connected_config(
    old: Option<&[u8]>,
    base: &str,
    model: &str,
    key: &str,
) -> Result<Vec<u8>, String> {
    let mut value = old.map(config).transpose()?.unwrap_or(json!({}));
    if !value["model"].is_object() {
        value["model"] = json!({});
    }
    for (name, v) in [
        ("default", model),
        ("model", model),
        ("provider", "custom"),
        ("base_url", base),
        ("api_key", key),
    ] {
        value["model"][name] = v.into();
    }
    serde_norway::to_string(&value)
        .map(String::into_bytes)
        .map_err(|e| e.to_string())
}
pub fn connect(base: &str, model: &str) -> Result<(), String> {
    connect_kind(AgentKind::Hermes, base, model)
}
/// OpenClaw's install is independent of the Hermes module transaction, so only an explicit
/// "disabled" state of the agents module blocks it.
fn ensure_kind_enabled(kind: AgentKind) -> Result<(), String> {
    match kind {
        AgentKind::Hermes => super::ensure_enabled("agents"),
        AgentKind::OpenClaw => super::ensure_not_disabled("agents"),
    }
}
fn executable_kind(kind: AgentKind) -> Option<PathBuf> {
    match kind {
        AgentKind::Hermes => executable(),
        AgentKind::OpenClaw => openclaw::executable(),
    }
}
pub fn connect_kind(kind: AgentKind, base: &str, model: &str) -> Result<(), String> {
    let _operation = super::OPERATION.try_lock().map_err(|_| "modules.busy")?;
    ensure_kind_enabled(kind)?;
    let _lock = CONNECTION_LOCK.lock().unwrap();
    if executable_kind(kind).is_none() {
        return Err("agents.not_installed".into());
    }
    let found = crate::integrations::agent_models::list_with_link()?;
    let Some(chosen) = found
        .iter()
        .find(|m| m.alias == model && m.chat != Some(false))
    else {
        return Err("agents.model_missing".into());
    };
    connect_at(
        kind,
        &config_file(kind)?,
        &TokenStore::global(),
        base,
        model,
        chosen.context_size,
    )
}
#[cfg(test)]
fn connect_in(dir: &Path, tokens: &TokenStore, base: &str, model: &str) -> Result<(), String> {
    connect_at(
        AgentKind::Hermes,
        &dir.join("config.yaml"),
        tokens,
        base,
        model,
        None,
    )
}
fn connect_at(
    kind: AgentKind,
    path: &Path,
    tokens: &TokenStore,
    base: &str,
    model: &str,
    context: Option<u32>,
) -> Result<(), String> {
    let dir = config_dir(path);
    let url = reqwest::Url::parse(base).map_err(|_| "agents.gateway_required".to_string())?;
    if url.scheme() != "http"
        || !matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))
        || url.path() != "/v1"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("agents.gateway_required".into());
    }
    if model.is_empty() || model.contains(['*', '?']) {
        return Err("agents.model_missing".into());
    }
    if record(dir)?.is_some() {
        return Err("agents.already_connected".into());
    }
    let old = match std::fs::read(path) {
        Ok(b) => Some(b),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.to_string()),
    };
    if let Some(bytes) = &old {
        parse_config(kind, bytes)?;
    }
    let backup = backup_of(path);
    if backup.exists() {
        return Err("agents.backup_exists".into());
    }
    if kind == AgentKind::OpenClaw {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    // The model id is matched exactly (a Harness Link model is `remote:<link>/<model>`).
    let token = tokens.create_with_policy(
        kind.label(),
        &[Scope::Inference],
        &TokenPolicy {
            models: vec![model.into()],
            endpoints: vec![GatewayEndpoint::Chat, GatewayEndpoint::Models],
            ..Default::default()
        },
    )?;
    let result = (|| {
        let bytes = match kind {
            AgentKind::Hermes => connected_config(old.as_deref(), base, model, &token.token)?,
            AgentKind::OpenClaw => {
                openclaw::connected_config(old.as_deref(), base, model, &token.token, context)?
            }
        };
        if let Some(old) = &old {
            JsonStore::sensitive(&backup).write_bytes(old)?;
        }
        // Journal before the atomic config replacement: interrupted connections remain recoverable.
        JsonStore::sensitive(dir.join("harness-connection.json")).save(&Connection {
            token_id: token.info.id.clone(),
            original_exists: old.is_some(),
            applied_hash: hash(&bytes),
            model: model.into(),
        })?;
        JsonStore::sensitive(path).write_bytes(&bytes)
    })();
    if result.is_err() {
        let _ = tokens.revoke(&token.info.id);
        // If the journal was never committed, no config replacement was attempted.
        if !dir.join("harness-connection.json").exists() && backup.exists() {
            let _ = std::fs::remove_file(&backup);
        }
    }
    result
}
pub fn disconnect() -> Result<(), String> {
    disconnect_kind(AgentKind::Hermes)
}
pub fn disconnect_kind(kind: AgentKind) -> Result<(), String> {
    let _lock = CONNECTION_LOCK.lock().unwrap();
    if running() {
        return Err("agents.busy".into());
    }
    disconnect_at(&config_file(kind)?, &TokenStore::global())
}
#[cfg(test)]
fn disconnect_in(dir: &Path, tokens: &TokenStore) -> Result<(), String> {
    disconnect_at(&dir.join("config.yaml"), tokens)
}
fn disconnect_at(path: &Path, tokens: &TokenStore) -> Result<(), String> {
    let dir = config_dir(path);
    let Some(record) = record(dir)? else {
        return Ok(());
    };
    // Revoke even if a later restore refuses to overwrite an external edit.
    match tokens.revoke(&record.token_id) {
        Ok(()) => {}
        Err(e) if e == crate::control_api::ERR_TOKEN_NOT_FOUND => {}
        Err(e) => return Err(e),
    }
    let current = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && !record.original_exists => Vec::new(),
        Err(e) => return Err(e.to_string()),
    };
    let backup = backup_of(path);
    let original = if record.original_exists {
        Some(std::fs::read(&backup).map_err(|e| e.to_string())?)
    } else {
        None
    };
    if !(hash(&current) == record.applied_hash
        || original.as_ref() == Some(&current)
        || !record.original_exists && !path.exists())
    {
        return Err("agents.config_changed".into());
    }
    if let Some(bytes) = original {
        JsonStore::sensitive(path).write_bytes(&bytes)?;
    } else if path.exists() {
        std::fs::remove_file(path).map_err(|e| e.to_string())?;
    }
    std::fs::remove_file(dir.join("harness-connection.json")).map_err(|e| e.to_string())?;
    if backup.exists() {
        std::fs::remove_file(backup).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct TaskStatus {
    pub id: String,
    /// Which agent ran the task (entries from before OpenClaw are Hermes runs).
    pub agent: AgentKind,
    pub model: String,
    pub prompt: String,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub tokens: Option<u64>,
    pub grants: RunGrants,
    pub audit: Vec<String>,
    pub status: String,
    pub output: String,
    pub error: Option<String>,
    /// The project the task was started for (its folders are the read grants).
    pub project_id: Option<String>,
}
/// Only bounded, read-only file tools are supported. Shell, writes and additional
/// network access cannot be granted by this adapter.
#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct RunGrants {
    pub read_folders: Vec<PathBuf>,
}
impl RunGrants {
    fn validate(&mut self) -> Result<(), String> {
        if self.read_folders.len() > 8 {
            return Err("agents.scope_invalid".into());
        }
        for folder in &mut self.read_folders {
            #[cfg(windows)]
            {
                use std::{
                    os::windows::fs::MetadataExt,
                    path::{Component, Prefix},
                };
                if !matches!(folder.components().next(), Some(Component::Prefix(p)) if matches!(p.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)))
                {
                    return Err("agents.scope_invalid".into());
                }
                let mut parts: Vec<_> = folder.ancestors().collect();
                parts.reverse();
                for part in parts {
                    if std::fs::symlink_metadata(part)
                        .map_err(|_| "agents.scope_invalid")?
                        .file_attributes()
                        & 0x400
                        != 0
                    {
                        return Err("agents.scope_invalid".into());
                    }
                }
            }
            if !folder.is_absolute() || !folder.is_dir() || folder.parent().is_none() {
                return Err("agents.scope_invalid".into());
            }
            *folder = folder.canonicalize().map_err(|_| "agents.scope_invalid")?;
            if folder.parent().is_none() {
                return Err("agents.scope_invalid".into());
            }
        }
        Ok(())
    }
}
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn history_store() -> JsonStore {
    JsonStore::sensitive(crate::paths::data_root().join("agents/history.json"))
}
fn save_task(status: &TaskStatus) -> Result<(), String> {
    history_store().update::<Vec<TaskStatus>, _>(|entries| {
        entries.retain(|e| e.id != status.id);
        entries.insert(0, status.clone());
        Ok(())
    })
}
pub fn task_history() -> Result<Vec<TaskStatus>, String> {
    // Serialize recovery with start/delete so a newly started run cannot be
    // mistaken for an interrupted run by an earlier history poll.
    let task = TASK.lock().unwrap();
    let active = task
        .as_ref()
        .map(|run| run.status.lock().unwrap().clone())
        .unwrap_or_default();
    history_store().update::<Vec<TaskStatus>, _>(|entries| {
        for entry in entries.iter_mut() {
            if entry.id == active.id {
                // Never replace a newer durable completion with a stale poll.
                continue;
            } else if entry.status == "running" {
                entry.status = "interrupted".into();
                entry.finished_at = Some(now());
                entry.audit.push("interrupted".into());
            }
        }
        let mut view = entries.clone();
        if let Some(entry) = view.iter_mut().find(|e| e.id == active.id) {
            *entry = active;
        }
        Ok(view)
    })
}
pub fn delete_task(id: &str) -> Result<(), String> {
    let mut slot = TASK.lock().unwrap();
    if let Some(run) = slot.as_ref() {
        let status = run.status.lock().unwrap();
        if status.id == id && status.status == "running" {
            return Err("agents.busy".into());
        }
    }
    history_store().update::<Vec<TaskStatus>, _>(|entries| {
        entries.retain(|e| e.id != id);
        Ok(())
    })?;
    if slot
        .as_ref()
        .is_some_and(|r| r.status.lock().unwrap().id == id)
    {
        *slot = None;
    }
    Ok(())
}
struct Run {
    status: Mutex<TaskStatus>,
    cancel: Arc<AtomicBool>,
}
pub(super) fn clear_history() {
    *TASK.lock().unwrap() = None;
}
pub fn task_status() -> TaskStatus {
    TASK.lock()
        .unwrap()
        .as_ref()
        .map(|r| r.status.lock().unwrap().clone())
        .unwrap_or(TaskStatus {
            status: "idle".into(),
            ..Default::default()
        })
}
fn running() -> bool {
    task_status().status == "running"
}
pub fn stop() {
    if let Some(r) = TASK.lock().unwrap().as_ref() {
        r.cancel.store(true, Ordering::SeqCst);
        r.status.lock().unwrap().audit.push("stop_requested".into());
    }
}
/// Give workers time to reap process trees and remove temporary task secrets on exit.
pub fn shutdown() {
    stop();
    cancel_install();
    cancel_install_kind(AgentKind::OpenClaw);
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(3) {
        let installing = [&INSTALL_RUN, &INSTALL_RUN_OPENCLAW]
            .into_iter()
            .any(|slot| {
                slot.lock()
                    .unwrap()
                    .as_ref()
                    .is_some_and(|r| r.status.lock().unwrap().status == "running")
            });
        if !running() && !installing {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
pub fn start_task(prompt: String) -> Result<(), String> {
    start_task_with_grants(prompt, RunGrants::default())
}
pub fn start_task_with_grants(prompt: String, grants: RunGrants) -> Result<(), String> {
    start_task_for_project(prompt, grants, None)
}
/// Starts a Hermes task filed under `project_id` (see `crate::projects::start_agent_task`).
pub fn start_task_for_project(
    prompt: String,
    grants: RunGrants,
    project_id: Option<String>,
) -> Result<(), String> {
    start_task_kind_for_project(AgentKind::Hermes, prompt, grants, project_id)
}
/// How a prepared task is launched; `extra` is added to the minimal inherited environment.
struct Launch {
    command: Command,
    extra: Vec<(&'static str, std::ffi::OsString)>,
    /// Keep the user's HOME/profile variables out of the child (OpenClaw reads them).
    isolate_profile: bool,
    /// OpenClaw prints its result as one JSON envelope on stdout.
    envelope: Option<Arc<Mutex<String>>>,
    audit: Vec<String>,
}
/// Environment variables a task process inherits from Harness.
const INHERITED_ENV: [&str; 12] = [
    "PATH",
    "SystemRoot",
    "WINDIR",
    "COMSPEC",
    "PATHEXT",
    "TEMP",
    "TMP",
    "HOME",
    "USERPROFILE",
    "LOCALAPPDATA",
    "APPDATA",
    "LANG",
];
const PROFILE_ENV: [&str; 4] = ["HOME", "USERPROFILE", "LOCALAPPDATA", "APPDATA"];

pub fn start_task_kind(kind: AgentKind, prompt: String, grants: RunGrants) -> Result<(), String> {
    start_task_kind_for_project(kind, prompt, grants, None)
}

/// [`start_task_kind`] for a task filed under `project_id`.
pub fn start_task_kind_for_project(
    kind: AgentKind,
    prompt: String,
    mut grants: RunGrants,
    project_id: Option<String>,
) -> Result<(), String> {
    grants.validate()?;
    if kind == AgentKind::OpenClaw && grants.read_folders.len() > 1 {
        // OpenClaw's read tool is confined to one workspace folder.
        return Err("agents.openclaw_one_folder".into());
    }
    // A folder is only readable when the user granted it to this agent (Permissions page):
    // revoking the grant stops the next run, whatever asked for it.
    let subject = match kind {
        AgentKind::Hermes => crate::permissions::AGENT_HERMES,
        AgentKind::OpenClaw => crate::permissions::AGENT_OPENCLAW,
    };
    crate::permissions::Permissions::shared().check_agent_run(subject, &grants.read_folders)?;
    let _operation = super::OPERATION.try_lock().map_err(|_| "modules.busy")?;
    ensure_kind_enabled(kind)?;
    let _connection = CONNECTION_LOCK.lock().unwrap();
    let mut task = TASK.lock().unwrap();
    if task
        .as_ref()
        .is_some_and(|r| r.status.lock().unwrap().status == "running")
    {
        return Err("agents.busy".into());
    }
    if prompt.trim().is_empty() || prompt.len() > 16384 {
        return Err("agents.task_invalid".into());
    }
    let exe = executable_kind(kind).ok_or("agents.not_installed")?;
    let hermes_source = match kind {
        AgentKind::Hermes => Some(library_runtime(&exe)?),
        AgentKind::OpenClaw => None,
    };
    let config_path = config_file(kind)?;
    let record = record_at(&config_path)?.ok_or("agents.not_connected")?;
    let bytes = std::fs::read(&config_path).map_err(|e| e.to_string())?;
    if hash(&bytes) != record.applied_hash {
        return Err("agents.config_changed".into());
    }
    let value = parse_config(kind, &bytes)?;
    // A clean upstream home avoids user hooks, MCP servers, skills and credential fallbacks.
    let isolated = match kind {
        AgentKind::Hermes => isolated_home(&exe)?,
        AgentKind::OpenClaw => tempfile::tempdir().map_err(|e| e.to_string())?,
    };
    let (launch, secret) = match (kind, hermes_source) {
        (AgentKind::Hermes, Some((mut c, source))) => {
            JsonStore::sensitive(isolated.path().join("config.yaml")).write_bytes(serde_norway::to_string(&json!({"model": value["model"], "toolsets": [], "agent": {"max_turns": 2}, "skills": {"auto_load": []}})).map_err(|e| e.to_string())?.as_bytes())?;
            JsonStore::sensitive(isolated.path().join("harness-task.json")).save(
                &json!({"model": value["model"], "prompt": prompt, "read_folders": grants.read_folders}),
            )?;
            let secret = value["model"]["api_key"].as_str().unwrap_or("").to_string();
            c.current_dir(isolated.path());
            (
                Launch {
                    command: c,
                    extra: vec![
                        ("HARNESS_HERMES_SOURCE", source.into()),
                        ("HERMES_HOME", isolated.path().into()),
                        ("PYTHONIOENCODING", "utf-8".into()),
                        ("PYTHONUNBUFFERED", "1".into()),
                    ],
                    isolate_profile: false,
                    envelope: None,
                    audit: vec![],
                },
                secret,
            )
        }
        _ => {
            let provider = value["models"]["providers"][openclaw::PROVIDER].clone();
            let secret = provider["apiKey"].as_str().unwrap_or("").to_string();
            let root = isolated.path();
            let (state, home, workspace) =
                (root.join("state"), root.join("home"), root.join("work"));
            for dir in [&state, &home, &workspace] {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            // A granted folder is the agent workspace and the only place its read tool reaches.
            let cwd =
                openclaw::plain_path(grants.read_folders.first().cloned().unwrap_or(workspace));
            let config = root.join("openclaw.json");
            let task_config = openclaw::task_config(
                &provider,
                &record.model,
                &cwd,
                !grants.read_folders.is_empty(),
            );
            JsonStore::sensitive(&config).write_bytes(
                &serde_json::to_vec_pretty(&task_config).map_err(|e| e.to_string())?,
            )?;
            let prompt_file = root.join("prompt.txt");
            JsonStore::sensitive(&prompt_file).write_bytes(prompt.as_bytes())?;
            let mut c = openclaw::task_command(
                &exe,
                &config,
                &state,
                &cwd,
                &prompt_file,
                &record.model,
                600,
            );
            c.arg("--json").current_dir(root);
            let scope = if grants.read_folders.is_empty() {
                json!({"kind": "scope", "tools": []})
            } else {
                json!({"kind": "scope", "tools": ["read"], "workspace": cwd})
            };
            (
                Launch {
                    command: c,
                    extra: vec![
                        ("HOME", home.clone().into()),
                        ("USERPROFILE", home.clone().into()),
                        ("APPDATA", home.join("AppData/Roaming").into()),
                        ("LOCALAPPDATA", home.join("AppData/Local").into()),
                        ("OPENCLAW_HOME", home.into()),
                        ("OPENCLAW_STATE_DIR", state.into()),
                        ("OPENCLAW_CONFIG_PATH", config.into()),
                    ],
                    isolate_profile: true,
                    envelope: Some(Arc::new(Mutex::new(String::new()))),
                    audit: vec![scope.to_string()],
                },
                secret,
            )
        }
    };
    let mut audit = vec!["granted".to_string(), "started".to_string()];
    audit.extend(launch.audit.iter().cloned());
    let run = Arc::new(Run {
        status: Mutex::new(TaskStatus {
            id: uuid::Uuid::new_v4().to_string(),
            agent: kind,
            model: record.model,
            prompt,
            started_at: now(),
            grants,
            audit,
            status: "running".into(),
            project_id,
            ..Default::default()
        }),
        cancel: Arc::new(AtomicBool::new(false)),
    });
    save_task(&run.status.lock().unwrap())?;
    *task = Some(run.clone());
    std::thread::spawn(move || {
        let Launch {
            mut command,
            extra,
            isolate_profile,
            envelope,
            ..
        } = launch;
        command.env_clear();
        for key in INHERITED_ENV {
            if isolate_profile && PROFILE_ENV.contains(&key) {
                continue;
            }
            if let Some(v) = std::env::var_os(key) {
                command.env(key, v);
            }
        }
        for (key, value) in extra {
            command.env(key, value);
        }
        let result = run_process_ext(
            command,
            &run,
            &secret,
            Duration::from_secs(600),
            envelope.clone(),
        );
        let mut status = run.status.lock().unwrap();
        if let Some(envelope) = &envelope {
            apply_envelope(&mut status, &envelope.lock().unwrap(), &secret);
        }
        match result {
            Ok(true) => status.status = "completed".into(),
            Ok(false) => {
                status.status = "failed".into();
                status.error = Some("agents.task_failed".into());
            }
            Err(e) => {
                status.status = "failed".into();
                status.error = Some(e);
            }
        }
        if run.cancel.load(Ordering::SeqCst) {
            status.status = "stopped".into();
            status.error = None;
        }
        status.finished_at = Some(now());
        let event = status.status.clone();
        status.audit.push(event);
        if let Err(e) = save_task(&status) {
            status.error = Some(e);
        }
        drop(isolated);
    });
    Ok(())
}
/// Shows the answer of an OpenClaw run (`final` text, token usage) instead of the diagnostics
/// that were collected while it ran. A run that printed no envelope keeps its raw output.
fn apply_envelope(status: &mut TaskStatus, stdout: &str, secret: &str) {
    let Some(start) = stdout.find('{') else {
        return;
    };
    let Some(end) = stdout.rfind('}') else { return };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&stdout[start..=end]) else {
        return;
    };
    let mut text = value["payloads"]
        .as_array()
        .map(|p| {
            p.iter()
                .filter_map(|p| p["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .filter(|t| !t.is_empty())
        .or_else(|| value["final"].as_str().map(str::to_string))
        .unwrap_or_default();
    if !secret.is_empty() {
        text = text.replace(secret, "[redacted]");
    }
    if value["ok"] == false {
        if text.is_empty() {
            text = value["error"]["message"]
                .as_str()
                .unwrap_or_default()
                .into();
        }
        if !status.output.trim().is_empty() {
            text = format!("{text}\n\n{}", status.output.trim_end());
        }
    }
    status.output = text;
    trim_output(&mut status.output);
    if let Some(total) = value["usage"]["total"].as_u64() {
        status.tokens = Some(total);
    }
}
fn run_process(
    c: Command,
    run: &Arc<Run>,
    secret: &str,
    timeout: Duration,
) -> Result<bool, String> {
    run_process_ext(c, run, secret, timeout, None)
}
/// With `capture`, stdout goes there (bounded) instead of the visible output; stderr is shown.
fn run_process_ext(
    mut c: Command,
    run: &Arc<Run>,
    secret: &str,
    timeout: Duration,
    capture: Option<Arc<Mutex<String>>>,
) -> Result<bool, String> {
    c.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::process_limits::prepare(&mut c).map_err(|e| e.to_string())?;
    let mut group = c.group();
    #[cfg(windows)]
    {
        group.kill_on_drop(true);
        group.creation_flags(0x0800_0000);
    }
    let mut child = group.spawn().map_err(|e| e.to_string())?;
    #[cfg(windows)]
    let _memory_job = {
        use std::os::windows::io::AsRawHandle;
        crate::process_limits::memory_job(child.inner().as_raw_handle())
            .map_err(|e| e.to_string())?
    };
    let _group_guard = crate::process_limits::GroupGuard(child.id());
    let mut readers = Vec::new();
    for (is_stdout, pipe) in [
        (
            true,
            child
                .inner()
                .stdout
                .take()
                .map(|p| Box::new(p) as Box<dyn Read + Send>),
        ),
        (
            false,
            child
                .inner()
                .stderr
                .take()
                .map(|p| Box::new(p) as Box<dyn Read + Send>),
        ),
    ]
    .into_iter()
    .filter_map(|(is_stdout, pipe)| pipe.map(|p| (is_stdout, p)))
    {
        let run = run.clone();
        let secret = secret.to_string();
        let capture = capture.clone().filter(|_| is_stdout);
        readers.push(std::thread::spawn(move || {
            for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                if let Some(capture) = &capture {
                    // Redaction happens when the envelope is read; keep a bounded copy.
                    let mut text = capture.lock().unwrap();
                    if text.len() < 1 << 20 {
                        text.push_str(&line);
                        text.push('\n');
                    }
                    continue;
                }
                let line = if secret.is_empty() {
                    line
                } else {
                    line.replace(&secret, "[redacted]")
                };
                let mut status = run.status.lock().unwrap();
                if let Some(event) = line.strip_prefix("HARNESS_EVENT:") {
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(event) {
                        if value["kind"] == "usage" {
                            status.tokens = value["tokens"].as_u64();
                        } else if value["kind"] == "output" {
                            status.output.push_str(value["text"].as_str().unwrap_or(""));
                            status.output.push('\n');
                            trim_output(&mut status.output);
                        } else if status.audit.len() < 256 {
                            status.audit.push(value.to_string());
                        }
                        if !status.id.is_empty() {
                            if let Err(e) = save_task(&status) {
                                status.error = Some(e);
                            }
                        }
                        continue;
                    }
                }
                status.output.push_str(&line);
                status.output.push('\n');
                trim_output(&mut status.output);
            }
        }));
    }
    let start = Instant::now();
    let mut result = loop {
        if run.cancel.load(Ordering::SeqCst) || start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            break Err("agents.timeout".into());
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status.success()),
            Ok(None) => {}
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(e.to_string());
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    // Descendants can retain stdout after their parent exits. Keep cancellation
    // and the deadline active until pipes close instead of blocking in join().
    while readers.iter().any(|reader| !reader.is_finished()) {
        if run.cancel.load(Ordering::SeqCst) || start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            result = Err("agents.timeout".into());
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
    for reader in readers {
        if reader.is_finished() {
            let _ = reader.join();
        }
    }
    result
}
fn trim_output(output: &mut String) {
    if output.len() > 65536 {
        let mut end = output.len() - 65536;
        while !output.is_char_boundary(end) {
            end += 1;
        }
        output.drain(..end);
    }
}
pub async fn install_managed() -> Result<(), String> {
    let run = Arc::new(Run {
        status: Mutex::new(TaskStatus {
            status: "running".into(),
            ..Default::default()
        }),
        cancel: Arc::new(AtomicBool::new(false)),
    });
    {
        let mut slot = INSTALL_RUN.lock().unwrap();
        if slot
            .as_ref()
            .is_some_and(|r| r.status.lock().unwrap().status == "running")
        {
            return Err("modules.busy".into());
        }
        *slot = Some(run.clone());
    }
    let result = install_upstream(&run).await;
    if let Err(error) = &result {
        // The module transaction erases the partial files next; keep what explains the failure.
        keep_install_log(&run, error);
    }
    let mut status = run.status.lock().unwrap();
    status.status = if run.cancel.load(Ordering::SeqCst) {
        "stopped"
    } else if result.is_ok() {
        "completed"
    } else {
        "failed"
    }
    .into();
    status.error = result.as_ref().err().cloned();
    result
}
/// Saves the installer output (and upstream's own install.log) as
/// `<logs>/agents-install.log`, replacing the previous one. Best effort.
fn keep_install_log(run: &Arc<Run>, error: &str) {
    let mut text = format!(
        "error: {error}
revision: {HERMES_REVISION}

--- installer output ---
"
    );
    text.push_str(&run.status.lock().unwrap().output);
    if let Ok(upstream) = std::fs::read(managed_root().join("logs/install.log")) {
        text.push_str(
            "
--- upstream install.log ---
",
        );
        text.push_str(&String::from_utf8_lossy(&upstream));
    }
    let dir = crate::paths::logs_dir();
    if std::fs::create_dir_all(&dir).is_ok() {
        let _ = std::fs::write(dir.join("agents-install.log"), text);
    }
}
pub fn cancel_install() {
    cancel_install_kind(AgentKind::Hermes);
}
pub fn cancel_install_kind(kind: AgentKind) {
    let slot = match kind {
        AgentKind::Hermes => &INSTALL_RUN,
        AgentKind::OpenClaw => &INSTALL_RUN_OPENCLAW,
    };
    if let Some(r) = slot.lock().unwrap().as_ref() {
        r.cancel.store(true, Ordering::SeqCst);
    }
}
/// Installs the pinned upstream OpenClaw release into Harness's own npm prefix with the
/// official `npm install` method (no global command, no PATH change, no daemon, no onboarding).
pub async fn install_openclaw() -> Result<(), String> {
    super::ensure_not_disabled("agents")?;
    let run = Arc::new(Run {
        status: Mutex::new(TaskStatus {
            agent: AgentKind::OpenClaw,
            status: "running".into(),
            ..Default::default()
        }),
        cancel: Arc::new(AtomicBool::new(false)),
    });
    {
        let mut slot = INSTALL_RUN_OPENCLAW.lock().unwrap();
        if slot
            .as_ref()
            .is_some_and(|r| r.status.lock().unwrap().status == "running")
        {
            return Err("modules.busy".into());
        }
        *slot = Some(run.clone());
    }
    let result = install_openclaw_run(&run).await;
    let mut status = run.status.lock().unwrap();
    status.status = if run.cancel.load(Ordering::SeqCst) {
        "stopped"
    } else if result.is_ok() {
        "completed"
    } else {
        "failed"
    }
    .into();
    status.error = result.as_ref().err().cloned();
    result
}
async fn install_openclaw_run(run: &Arc<Run>) -> Result<(), String> {
    if openclaw::managed_executable().is_some() {
        return Ok(());
    }
    let node = tokio::task::spawn_blocking(|| tool_version("node"))
        .await
        .map_err(|e| e.to_string())?;
    if !node.as_deref().is_some_and(openclaw::node_supported) {
        return Err("agents.node_outdated".into());
    }
    let npm = tokio::task::spawn_blocking(|| tool_version(npm_tool()))
        .await
        .map_err(|e| e.to_string())?
        .ok_or("agents.npm_missing")?;
    let root = openclaw::managed_root();
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let mut c = command(npm_tool());
    // Upstream documents a global install; a private prefix keeps it out of the user's globals.
    c.args(["install", "--global", "--prefix"])
        .arg(&root)
        .arg(format!("openclaw@{}", openclaw::OPENCLAW_VERSION))
        .args(["--no-fund", "--no-audit", "--no-update-notifier"])
        // A large package on a flaky connection: retry instead of failing on one reset.
        .args(["--fetch-retries=5", "--fetch-retry-mintimeout=2000"])
        .env("npm_config_cache", root.join("cache"));
    if openclaw::npm_allows_scripts_flag(&npm) {
        // Upstream's documented flag lets the package run its own install scripts.
        c.arg("--allow-scripts=openclaw");
    }
    let task = run.clone();
    let success =
        tokio::task::spawn_blocking(move || run_process(c, &task, "", Duration::from_secs(1800)))
            .await
            .map_err(|e| e.to_string())??;
    PROBE.lock().unwrap().clear();
    if !success {
        return Err("agents.install_failed".into());
    }
    if openclaw::managed_executable().is_none() {
        return Err("agents.not_installed".into());
    }
    Ok(())
}
/// Removes Harness's own OpenClaw install (never an external one) after restoring its config.
pub fn remove_openclaw() -> Result<(), String> {
    let _operation = super::OPERATION.try_lock().map_err(|_| "modules.busy")?;
    if running()
        || INSTALL_RUN_OPENCLAW
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|r| r.status.lock().unwrap().status == "running")
    {
        return Err("agents.busy".into());
    }
    disconnect_kind(AgentKind::OpenClaw)?;
    let root = openclaw::managed_root();
    match std::fs::symlink_metadata(&root) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.to_string()),
        Ok(m) if m.file_type().is_symlink() => return Err("modules.unsafe_path".into()),
        Ok(_) => std::fs::remove_dir_all(&root).map_err(|e| e.to_string())?,
    }
    PROBE.lock().unwrap().clear();
    *INSTALL_RUN_OPENCLAW.lock().unwrap() = None;
    Ok(())
}
async fn install_upstream(run: &Arc<Run>) -> Result<(), String> {
    let temp = tempfile::tempdir().map_err(|e| e.to_string())?;
    let windows = cfg!(windows);
    let url = if windows {
        "https://hermes-agent.nousresearch.com/install.ps1"
    } else {
        "https://hermes-agent.nousresearch.com/install.sh"
    };
    let bytes = reqwest::Client::builder()
        .https_only(true)
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?
        .get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .bytes()
        .await
        .map_err(|e| e.to_string())?;
    let script = temp
        .path()
        .join(if windows { "install.ps1" } else { "install.sh" });
    std::fs::write(&script, bytes).map_err(|e| e.to_string())?;
    // Use upstream's staged protocol. Do not publish global commands, edit PATH,
    // run setup, or install services. PM prepares the source launcher and runtime.
    for stage in [
        "prerequisites",
        "repository",
        "venv",
        "python-deps",
        "complete",
    ] {
        let mut c = command(if windows { "powershell" } else { "bash" });
        c.env("HERMES_HOME", managed_root())
            .env("HERMES_INSTALL_DIR", managed_root().join("hermes-agent"))
            .env("UV_CACHE_DIR", managed_root().join("cache"))
            .env("UV_PYTHON_INSTALL_DIR", managed_root().join("python"))
            // Stream every child command's output so a failure reaches agents-install.log.
            .env("HERMES_INSTALL_VERBOSE", "1");
        if windows {
            c.args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(&script)
            .args([
                "-NonInteractive",
                "-SkipBrowser",
                "-SkipComputerUse",
                "-Stage",
                stage,
            ])
            .args(["-Commit", HERMES_REVISION])
            .arg("-InstallDir")
            .arg(managed_root().join("hermes-agent"));
        } else {
            c.arg(&script).args([
                "--non-interactive",
                "--skip-browser",
                "--skip-computer-use",
                "--stage",
                stage,
                "--commit",
                HERMES_REVISION,
            ]);
        }
        let run = run.clone();
        let success = tokio::task::spawn_blocking(move || {
            run_process(c, &run, "", Duration::from_secs(1800))
        })
        .await
        .map_err(|e| e.to_string())??;
        if !success {
            return Err("agents.install_failed".into());
        }
    }

    PROBE.lock().unwrap().clear();
    if managed_executable().is_none() {
        return Err("agents.not_installed".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installs_owner_detects_both_layouts() {
        let temp = tempfile::tempdir().unwrap();
        // New style: <root>/hermes-agent with <root>/installs/<key>/facts.json.
        let root = temp.path().join("root");
        let checkout = root.join("hermes-agent");
        std::fs::create_dir_all(&checkout).unwrap();
        assert_eq!(installs_owner(&checkout, None), None);
        std::fs::create_dir_all(root.join("installs/abc")).unwrap();
        assert_eq!(installs_owner(&checkout, None), None, "no facts.json yet");
        std::fs::write(root.join("installs/abc/facts.json"), "{}").unwrap();
        assert_eq!(installs_owner(&checkout, None), Some(root.clone()));
        // A checkout elsewhere falls back to the platform default home.
        let elsewhere = temp.path().join("src/hermes-agent");
        std::fs::create_dir_all(&elsewhere).unwrap();
        assert_eq!(installs_owner(&elsewhere, Some(&root)), Some(root.clone()));
        assert_eq!(installs_owner(&elsewhere, None), None);
        // Old style: a venv in the checkout and no installs folder.
        let old = temp.path().join("old/hermes-agent");
        std::fs::create_dir_all(old.join("venv")).unwrap();
        assert_eq!(installs_owner(&old, Some(&temp.path().join("old"))), None);
    }
    #[test]
    fn link_dir_links_replaces_links_and_keeps_real_folders() {
        let temp = tempfile::tempdir().unwrap();
        let (a, b) = (temp.path().join("a"), temp.path().join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(a.join("marker"), "a").unwrap();
        let link = temp.path().join("link");
        link_dir(&a, &link).unwrap();
        assert!(link.join("marker").is_file());
        link_dir(&a, &link).unwrap();
        link_dir(&b, &link).unwrap();
        assert!(!link.join("marker").exists());
        // Removing the link never touches the target.
        std::fs::remove_dir(&link)
            .or_else(|_| std::fs::remove_file(&link))
            .unwrap();
        assert!(a.join("marker").is_file());
        let real = temp.path().join("real");
        std::fs::create_dir_all(&real).unwrap();
        assert!(link_dir(&a, &real).is_err());
        assert!(real.is_dir());
    }
    #[test]
    fn source_install_with_a_command_file_launcher_is_found() {
        // Upstream publishes hermes.cmd (no hermes.exe) when distlib is unavailable.
        let temp = tempfile::tempdir().unwrap();
        let install = temp.path().join("hermes-agent");
        std::fs::create_dir_all(install.join(".hermes/bin")).unwrap();
        std::fs::write(install.join("run_agent.py"), "").unwrap();
        let launcher = install.join(".hermes/bin/hermes.cmd");
        std::fs::write(&launcher, "@echo off").unwrap();
        assert_eq!(
            executable_in(LAUNCHERS.iter().map(|p| install.join(p))),
            Some(launcher.clone())
        );
        match library_entry(&launcher) {
            Ok((Entry::Launcher(found), root)) => {
                assert_eq!(found, launcher);
                assert_eq!(root, install);
            }
            _ => panic!("the source launcher must be recognised"),
        }
    }
    #[test]
    fn grants_fail_closed_and_history_deletion_preserves_agent_data() {
        let temp = tempfile::tempdir().unwrap();
        let external = temp.path().join("agent-session.json");
        std::fs::write(&external, "keep").unwrap();
        let mut grants = RunGrants {
            read_folders: vec![temp.path().to_path_buf()],
        };
        grants.validate().unwrap();
        assert_eq!(grants.read_folders[0], temp.path().canonicalize().unwrap());
        for folders in [
            vec![PathBuf::from("relative")],
            vec![external.clone()],
            vec![temp.path().to_path_buf(); 9],
        ] {
            assert!(RunGrants {
                read_folders: folders
            }
            .validate()
            .is_err());
        }
        assert!(serde_json::from_value::<RunGrants>(json!({"shell": true})).is_err());
        assert!(RunGrants::default().read_folders.is_empty());
        // Without a standing grant for the folder, a run is refused before anything starts.
        let err = start_task_with_grants("hello".into(), grants.clone()).unwrap_err();
        assert!(err.starts_with("permissions.folder_not_granted"), "{err}");
        let entry = TaskStatus {
            id: uuid::Uuid::new_v4().to_string(),
            model: "chat".into(),
            status: "running".into(),
            output: "retained output".into(),
            grants,
            started_at: now(),
            ..Default::default()
        };
        save_task(&entry).unwrap();
        let history = task_history().unwrap();
        let recovered = history.iter().find(|e| e.id == entry.id).unwrap();
        assert_eq!(recovered.status, "interrupted");
        assert_eq!(recovered.output, "retained output");
        assert!(recovered.finished_at.is_some());
        delete_task(&entry.id).unwrap();
        assert!(task_history().unwrap().iter().all(|e| e.id != entry.id));
        assert_eq!(std::fs::read_to_string(external).unwrap(), "keep");
        std::fs::remove_file(crate::paths::data_root().join("agents/history.json")).unwrap();
        std::fs::remove_dir(crate::paths::data_root().join("agents")).unwrap();
    }
    #[test]
    fn process_audit_events_respect_the_entry_limit() {
        let event = json!({"kind": "audit", "text": "recorded"});
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join("event.json"),
            format!("HARNESS_EVENT:{event}\n"),
        )
        .unwrap();
        for initial_len in [0, 255, 256] {
            let run = Arc::new(Run {
                status: Mutex::new(TaskStatus {
                    audit: vec!["existing".into(); initial_len],
                    ..Default::default()
                }),
                cancel: Arc::new(AtomicBool::new(false)),
            });
            let mut c = command(if cfg!(windows) { "cmd" } else { "sh" });
            c.current_dir(temp.path());
            if cfg!(windows) {
                c.args(["/C", "type event.json"]);
            } else {
                c.args(["-c", "cat event.json"]);
            }
            assert!(run_process(c, &run, "", Duration::from_secs(10)).unwrap());
            let status = run.status.lock().unwrap();
            assert_eq!(status.audit.len(), (initial_len + 1).min(256));
            assert!(status.audit[..initial_len].iter().all(|e| e == "existing"));
            if initial_len < 256 {
                assert_eq!(status.audit[initial_len], event.to_string());
            }
            assert!(status.output.is_empty());
        }
    }

    #[test]
    fn process_errors_redact_secrets_and_stop_is_bounded() {
        let run = Arc::new(Run {
            status: Mutex::new(TaskStatus::default()),
            cancel: Arc::new(AtomicBool::new(false)),
        });
        let mut c = command(if cfg!(windows) { "cmd" } else { "sh" });
        if cfg!(windows) {
            c.args(["/C", "echo hs_secret_test& exit /b 7"]);
        } else {
            c.args(["-c", "echo hs_secret_test; exit 7"]);
        }
        assert!(!run_process(c, &run, "hs_secret_test", Duration::from_secs(10)).unwrap());
        let output = run.status.lock().unwrap().output.clone();
        assert!(output.contains("[redacted]"));
        assert!(!output.contains("hs_secret_test"));
        run.cancel.store(true, Ordering::SeqCst);
        let mut c = command(if cfg!(windows) { "cmd" } else { "sh" });
        if cfg!(windows) {
            c.args(["/C", "ping -n 30 127.0.0.1 >nul"]);
        } else {
            c.args(["-c", "sleep 30"]);
        }
        let start = Instant::now();
        assert!(run_process(c, &run, "", Duration::from_secs(10)).is_err());
        assert!(start.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn stop_reaps_descendants_after_the_parent_exits() {
        let run = Arc::new(Run {
            status: Mutex::new(TaskStatus::default()),
            cancel: Arc::new(AtomicBool::new(false)),
        });
        let mut c = command(std::env::current_exe().unwrap());
        c.args([
            "--ignored",
            "--exact",
            "modules::agents::tests::pipe_child_fixture",
            "--nocapture",
        ])
        .env("HS_AGENT_PIPE_FIXTURE", "parent");
        let cancel = run.cancel.clone();
        let stopper = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            cancel.store(true, Ordering::SeqCst);
        });
        let start = Instant::now();
        assert!(run_process(c, &run, "", Duration::from_secs(3)).is_err());
        stopper.join().unwrap();
        assert!(start.elapsed() < Duration::from_secs(5));
    }

    #[test]
    #[ignore = "subprocess fixture for the descendant cancellation test"]
    #[allow(clippy::zombie_processes)] // The parent must exit; run_process reaps its entire group.
    fn pipe_child_fixture() {
        if std::env::var("HS_AGENT_PIPE_FIXTURE").as_deref() == Ok("parent") {
            let mut c = command(std::env::current_exe().unwrap());
            c.args([
                "--ignored",
                "--exact",
                "modules::agents::tests::pipe_child_fixture",
                "--nocapture",
            ])
            .env("HS_AGENT_PIPE_FIXTURE", "child")
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
            // Deliberately leave the descendant alive with inherited output.
            let _child = c.spawn().unwrap();
        } else if std::env::var("HS_AGENT_PIPE_FIXTURE").as_deref() == Ok("child") {
            std::thread::sleep(Duration::from_secs(30));
        }
    }

    #[test]
    fn interrupted_replace_and_empty_config_recover_without_losing_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let tokens = TokenStore::at(&dir.join("tokens"));
        let old = b"# An otherwise empty configuration\n";
        std::fs::write(dir.join("config.yaml"), old).unwrap();
        connect_in(dir, &tokens, "http://127.0.0.1:11435/v1", "chat").unwrap();
        assert!(crate::instance::is_private_to_current_user(&dir.join("config.yaml")).unwrap());
        assert!(
            crate::instance::is_private_to_current_user(&dir.join("config.yaml.harness.bak"))
                .unwrap()
        );
        // The journal and backup exist, but replacement never took place.
        std::fs::write(dir.join("config.yaml"), old).unwrap();
        disconnect_in(dir, &tokens).unwrap();
        assert_eq!(std::fs::read(dir.join("config.yaml")).unwrap(), old);
        assert!(tokens.list().unwrap().is_empty());
        std::fs::remove_file(dir.join("config.yaml")).unwrap();
        connect_in(dir, &tokens, "http://127.0.0.1:11435/v1", "chat").unwrap();
        std::fs::remove_file(dir.join("config.yaml")).unwrap();
        disconnect_in(dir, &tokens).unwrap();
        assert!(!dir.join("harness-connection.json").exists());
    }
    #[test]
    fn config_backup_restore_and_scoped_token() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let tokens = TokenStore::at(&dir.join("tokens"));
        let old = b"# Keep my comments\nmodel: old\nterminal:\n  backend: docker\n";
        std::fs::write(dir.join("config.yaml"), old).unwrap();
        connect_in(dir, &tokens, "http://127.0.0.1:11435/v1", "chat").unwrap();
        assert_eq!(
            std::fs::read(dir.join("config.yaml.harness.bak")).unwrap(),
            old
        );
        let cfg = config(&std::fs::read(dir.join("config.yaml")).unwrap()).unwrap();
        assert_eq!(cfg["terminal"]["backend"], "docker");
        assert_eq!(cfg["model"]["provider"], "custom");
        let list = tokens.list().unwrap();
        assert_eq!(list[0].scopes, vec![Scope::Inference]);
        assert_eq!(list[0].policy.models, vec!["chat"]);
        assert!(connect_in(dir, &tokens, "http://127.0.0.1:11435/v1", "chat").is_err());
        disconnect_in(dir, &tokens).unwrap();
        assert!(tokens.list().unwrap().is_empty());
        assert_eq!(std::fs::read(dir.join("config.yaml")).unwrap(), old);
        disconnect_in(dir, &tokens).unwrap();
    }
    #[test]
    fn refuses_corruption_and_external_edits_revokes_anyway() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let tokens = TokenStore::at(&dir.join("tokens"));
        std::fs::write(dir.join("config.yaml"), "model: [broken").unwrap();
        assert!(connect_in(dir, &tokens, "http://127.0.0.1:11435/v1", "chat").is_err());
        assert!(tokens.list().unwrap().is_empty());
        std::fs::remove_file(dir.join("config.yaml")).unwrap();
        connect_in(dir, &tokens, "http://127.0.0.1:11435/v1", "chat").unwrap();
        std::fs::write(dir.join("config.yaml"), "my: edit").unwrap();
        assert_eq!(
            disconnect_in(dir, &tokens).unwrap_err(),
            "agents.config_changed"
        );
        assert!(tokens.list().unwrap().is_empty());
        assert_eq!(
            std::fs::read_to_string(dir.join("config.yaml")).unwrap(),
            "my: edit"
        );
    }
    #[test]
    fn absent_config_restored_and_detection_requires_a_file() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let tokens = TokenStore::at(&dir.join("tokens"));
        assert!(executable_in([dir.join("missing"), dir.to_path_buf()]).is_none());
        let exe = dir.join("hermes");
        std::fs::write(&exe, "").unwrap();
        assert_eq!(executable_in([exe.clone()]), Some(exe));
        connect_in(dir, &tokens, "http://127.0.0.1:11435/v1", "chat").unwrap();
        disconnect_in(dir, &tokens).unwrap();
        assert!(!dir.join("config.yaml").exists());
        assert!(connect_in(dir, &tokens, "https://example.com/v1", "chat").is_err());
        assert!(connect_in(dir, &tokens, "http://127.0.0.1:11435/v1", "*").is_err());
    }

    #[test]
    fn openclaw_connection_restores_exact_bytes_and_scopes_the_token() {
        let temp = tempfile::tempdir().unwrap();
        let tokens = TokenStore::at(&temp.path().join("tokens"));
        let config = temp.path().join("state").join("openclaw.json");
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        let old =
            b"// my comments survive in the backup\n{\n  \"channels\": {\"telegram\": {}},\n}\n";
        std::fs::write(&config, old).unwrap();
        let url = "http://127.0.0.1:11620/v1";
        connect_at(
            AgentKind::OpenClaw,
            &config,
            &tokens,
            url,
            "chat",
            Some(8192),
        )
        .unwrap();
        assert_eq!(std::fs::read(backup_of(&config)).unwrap(), old);
        let cfg = openclaw::parse_config(&std::fs::read(&config).unwrap()).unwrap();
        assert_eq!(cfg["channels"]["telegram"], json!({}));
        assert_eq!(
            cfg["agents"]["defaults"]["model"]["primary"],
            "harness/chat"
        );
        assert_eq!(
            cfg["models"]["providers"]["harness"]["models"][0]["id"],
            "chat"
        );
        let token = &tokens.list().unwrap()[0];
        assert_eq!(token.scopes, vec![Scope::Inference]);
        assert_eq!(token.policy.models, vec!["chat"]);
        assert_eq!(
            token.policy.endpoints,
            vec![GatewayEndpoint::Chat, GatewayEndpoint::Models]
        );
        assert!(token.policy.allows_model("chat") && !token.policy.allows_model("other"));
        assert!(record_at(&config).unwrap().is_some());
        assert_eq!(
            connect_at(AgentKind::OpenClaw, &config, &tokens, url, "chat", None).unwrap_err(),
            "agents.already_connected"
        );
        disconnect_at(&config, &tokens).unwrap();
        assert_eq!(std::fs::read(&config).unwrap(), old);
        assert!(tokens.list().unwrap().is_empty());
        assert!(!backup_of(&config).exists());
        assert!(record_at(&config).unwrap().is_none());
        disconnect_at(&config, &tokens).unwrap();
    }

    #[test]
    fn openclaw_refuses_external_edits_and_unparseable_configs() {
        let temp = tempfile::tempdir().unwrap();
        let tokens = TokenStore::at(&temp.path().join("tokens"));
        let config = temp.path().join("openclaw.json");
        let url = "http://127.0.0.1:11620/v1";
        std::fs::write(&config, "{ unquoted: json5 }").unwrap();
        assert_eq!(
            connect_at(AgentKind::OpenClaw, &config, &tokens, url, "chat", None).unwrap_err(),
            "agents.config_invalid"
        );
        assert!(tokens.list().unwrap().is_empty());
        assert!(!backup_of(&config).exists());
        std::fs::remove_file(&config).unwrap();
        connect_at(AgentKind::OpenClaw, &config, &tokens, url, "chat", None).unwrap();
        std::fs::write(&config, "{\"mine\": true}").unwrap();
        assert_eq!(
            disconnect_at(&config, &tokens).unwrap_err(),
            "agents.config_changed"
        );
        // The token is revoked even when the restore is refused; the edit stays untouched.
        assert!(tokens.list().unwrap().is_empty());
        assert_eq!(
            std::fs::read_to_string(&config).unwrap(),
            "{\"mine\": true}"
        );
    }

    #[test]
    fn openclaw_without_an_original_config_removes_what_it_wrote() {
        let temp = tempfile::tempdir().unwrap();
        let tokens = TokenStore::at(&temp.path().join("tokens"));
        let config = temp.path().join("fresh").join("openclaw.json");
        let url = "http://127.0.0.1:11620/v1";
        connect_at(AgentKind::OpenClaw, &config, &tokens, url, "chat", None).unwrap();
        assert!(config.exists());
        disconnect_at(&config, &tokens).unwrap();
        assert!(!config.exists());
        for (base, model) in [("https://example.com/v1", "chat"), (url, "a*"), (url, "")] {
            assert!(connect_at(AgentKind::OpenClaw, &config, &tokens, base, model, None).is_err());
        }
        assert!(tokens.list().unwrap().is_empty());
    }

    #[test]
    fn link_models_get_an_exact_token_policy_for_every_agent() {
        let model = "remote:lab-box/qwen3:8b";
        let temp = tempfile::tempdir().unwrap();
        let tokens = TokenStore::at(&temp.path().join("tokens"));
        let url = "http://127.0.0.1:11620/v1";
        connect_in(temp.path(), &tokens, url, model).unwrap();
        let config = temp.path().join("oc").join("openclaw.json");
        connect_at(AgentKind::OpenClaw, &config, &tokens, url, model, None).unwrap();
        let list = tokens.list().unwrap();
        assert_eq!(list.len(), 2);
        for token in &list {
            assert_eq!(token.policy.models, vec![model]);
            assert!(token.policy.allows_model(model));
            assert!(token.policy.allows_model("REMOTE:lab-box/QWEN3:8b"));
            assert!(!token.policy.allows_model("remote:lab-box/qwen3:70b"));
            assert!(!token.policy.allows_model("remote:other/qwen3:8b"));
            assert!(!token.policy.allows_model("qwen3:8b"));
        }
        // A glob-looking id would widen the token: refused.
        let glob = temp.path().join("g.json");
        assert!(connect_at(
            AgentKind::OpenClaw,
            &glob,
            &tokens,
            url,
            "remote:lab/*",
            None
        )
        .is_err());
    }

    #[test]
    fn openclaw_envelope_becomes_the_task_answer_and_usage() {
        let mut status = TaskStatus {
            output: "diagnostics\n".into(),
            ..Default::default()
        };
        let stdout = "{\n \"ok\": true,\n \"payloads\": [{\"text\": \"HARNESS_OK sk-secret\"}],\n \"usage\": {\"total\": 42}\n}\n";
        apply_envelope(&mut status, stdout, "sk-secret");
        assert_eq!(status.output, "HARNESS_OK [redacted]");
        assert_eq!(status.tokens, Some(42));
        let mut failed = TaskStatus {
            output: "stderr line\n".into(),
            ..Default::default()
        };
        apply_envelope(
            &mut failed,
            "{\"ok\": false, \"error\": {\"message\": \"timed out\"}}",
            "",
        );
        assert_eq!(failed.output, "timed out\n\nstderr line");
        let mut raw = TaskStatus {
            output: "kept".into(),
            ..Default::default()
        };
        apply_envelope(&mut raw, "not json", "");
        assert_eq!(raw.output, "kept");
    }

    #[test]
    fn agent_kinds_parse_and_old_history_entries_are_hermes() {
        assert_eq!(AgentKind::parse("hermes").unwrap(), AgentKind::Hermes);
        assert_eq!(AgentKind::parse("openclaw").unwrap(), AgentKind::OpenClaw);
        assert_eq!(
            AgentKind::parse("other").unwrap_err(),
            "agents.kind_invalid"
        );
        let old: TaskStatus =
            serde_json::from_value(json!({"id": "x", "status": "completed"})).unwrap();
        assert_eq!(old.agent, AgentKind::Hermes);
        assert_eq!(
            serde_json::to_value(AgentKind::OpenClaw).unwrap(),
            "openclaw"
        );
    }

    #[test]
    fn captured_stdout_stays_out_of_the_visible_output() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("out.json"), "{\"ok\": true}\n").unwrap();
        let run = Arc::new(Run {
            status: Mutex::new(TaskStatus::default()),
            cancel: Arc::new(AtomicBool::new(false)),
        });
        let mut c = command(if cfg!(windows) { "cmd" } else { "sh" });
        c.current_dir(temp.path());
        if cfg!(windows) {
            c.args(["/C", "type out.json& echo diagnostic 1>&2"]);
        } else {
            c.args(["-c", "cat out.json; echo diagnostic >&2"]);
        }
        let capture = Arc::new(Mutex::new(String::new()));
        assert!(
            run_process_ext(c, &run, "", Duration::from_secs(10), Some(capture.clone())).unwrap()
        );
        assert!(capture.lock().unwrap().contains("\"ok\""));
        let output = run.status.lock().unwrap().output.clone();
        assert!(output.contains("diagnostic") && !output.contains("\"ok\""));
    }
}
