//! Hermes upstream adapter. Credentials never cross IPC; first tasks have no tools.
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

struct Probe {
    executable: PathBuf,
    at: Instant,
    output: Option<String>,
}
static PROBE: Mutex<Option<Probe>> = Mutex::new(None);
fn probe(exe: &Path) -> Option<String> {
    let mut slot = PROBE.lock().unwrap();
    if let Some(p) = slot
        .as_ref()
        .filter(|p| p.executable == exe && p.at.elapsed() < Duration::from_secs(30))
    {
        return p.output.clone();
    }
    let mut c = command(exe);
    c.arg("--version");
    let output = bounded_output(c, Duration::from_secs(15))
        .ok()
        .and_then(|(ok, s)| ok.then_some(s));
    *slot = Some(Probe {
        executable: exe.to_path_buf(),
        at: Instant::now(),
        output: output.clone(),
    });
    output
}
static CONNECTION_LOCK: Mutex<()> = Mutex::new(());
static TASK: Mutex<Option<Arc<Run>>> = Mutex::new(None);
static INSTALL_RUN: Mutex<Option<Arc<Run>>> = Mutex::new(None);

fn home() -> Result<PathBuf, String> {
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
fn executable() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            candidates.push(dir.join(if cfg!(windows) {
                "hermes.exe"
            } else {
                "hermes"
            }));
        }
    }
    if let Ok(dir) = home() {
        for name in [
            "bin/hermes.exe",
            "hermes-agent/venv/Scripts/hermes.exe",
            "hermes-agent/venv/bin/hermes",
        ] {
            candidates.push(dir.join(name));
        }
    }
    if let Some(dir) = dirs::home_dir() {
        candidates.push(dir.join(".local/bin/hermes"));
    }
    executable_in(candidates)
}
/// Use upstream's source launcher (PM bootstrap) or its legacy virtual environment.
/// The Python API stays in-process; modern `hermes chat` may detach a gateway daemon.
fn library_runtime(exe: &Path) -> Result<(Command, PathBuf), String> {
    let mut roots: Vec<PathBuf> = exe.ancestors().skip(1).map(Path::to_path_buf).collect();
    if let Some(output) = probe(exe) {
        for line in output.lines() {
            if let Some(path) = line.strip_prefix("Project: ") {
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
        for name in [".hermes/bin/hermes.exe", ".hermes/bin/hermes"] {
            let launcher = root.join(name);
            if launcher.is_file() {
                let mut c = command(launcher);
                c.args(["--run-module", "code", "-c", include_str!("hermes_task.py")]);
                return Ok((c, root));
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
                let mut c = command(python);
                c.args(["-c", include_str!("hermes_task.py")]);
                return Ok((c, root));
            }
        }
    }
    Err("agents.library_missing".into())
}
fn command(path: impl AsRef<std::ffi::OsStr>) -> Command {
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
#[derive(Serialize)]
pub struct Detection {
    pub installed: bool,
    pub version: Option<String>,
    pub executable: Option<String>,
    pub config_path: String,
    pub missing_dependencies: Vec<String>,
    pub connected: bool,
    pub installation: TaskStatus,
    pub model: Option<String>,
}
pub fn detect() -> Result<Detection, String> {
    let exe = executable();
    let version = exe
        .as_ref()
        .and_then(|p| probe(p))
        .and_then(|text| text.lines().next().map(str::to_string));
    let mut missing = Vec::new();
    if let Some(exe) = &exe {
        if library_runtime(exe).is_err() {
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
    })
}
fn bounded_output(mut c: Command, timeout: Duration) -> Result<(bool, String), String> {
    c.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = c.group_spawn().map_err(|e| e.to_string())?;
    let stdout = child.inner().stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout.take(65536).read_to_string(&mut s);
        s
    });
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return Ok((status.success(), reader.join().unwrap_or_default()));
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
    let _lock = CONNECTION_LOCK.lock().unwrap();
    if executable().is_none() {
        return Err("agents.not_installed".into());
    }
    if !crate::integrations::agent_models::list()?
        .iter()
        .any(|m| m.alias == model && m.chat != Some(false))
    {
        return Err("agents.model_missing".into());
    }
    connect_in(&home()?, &TokenStore::global(), base, model)
}
fn connect_in(dir: &Path, tokens: &TokenStore, base: &str, model: &str) -> Result<(), String> {
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
    let path = dir.join("config.yaml");
    let old = match std::fs::read(&path) {
        Ok(b) => Some(b),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.to_string()),
    };
    if let Some(bytes) = &old {
        config(bytes)?;
    }
    let backup = dir.join("config.yaml.harness.bak");
    if backup.exists() {
        return Err("agents.backup_exists".into());
    }
    let token = tokens.create_with_policy(
        "Hermes Agent",
        &[Scope::Inference],
        &TokenPolicy {
            models: vec![model.into()],
            endpoints: vec![GatewayEndpoint::Chat, GatewayEndpoint::Models],
            ..Default::default()
        },
    )?;
    let result = (|| {
        let bytes = connected_config(old.as_deref(), base, model, &token.token)?;
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
        JsonStore::sensitive(&path).write_bytes(&bytes)
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
    let _lock = CONNECTION_LOCK.lock().unwrap();
    if running() {
        return Err("agents.busy".into());
    }
    disconnect_in(&home()?, &TokenStore::global())
}
fn disconnect_in(dir: &Path, tokens: &TokenStore) -> Result<(), String> {
    let Some(record) = record(dir)? else {
        return Ok(());
    };
    // Revoke even if a later restore refuses to overwrite an external edit.
    match tokens.revoke(&record.token_id) {
        Ok(()) => {}
        Err(e) if e == crate::control_api::ERR_TOKEN_NOT_FOUND => {}
        Err(e) => return Err(e),
    }
    let path = dir.join("config.yaml");
    let current = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && !record.original_exists => Vec::new(),
        Err(e) => return Err(e.to_string()),
    };
    let backup = dir.join("config.yaml.harness.bak");
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
        JsonStore::sensitive(&path).write_bytes(&bytes)?;
    } else if path.exists() {
        std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    std::fs::remove_file(dir.join("harness-connection.json")).map_err(|e| e.to_string())?;
    if backup.exists() {
        std::fs::remove_file(backup).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[derive(Clone, Serialize, Default)]
pub struct TaskStatus {
    pub status: String,
    pub output: String,
    pub error: Option<String>,
}
struct Run {
    status: Mutex<TaskStatus>,
    cancel: Arc<AtomicBool>,
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
    }
}
/// Give workers time to reap process trees and remove temporary task secrets on exit.
pub fn shutdown() {
    stop();
    cancel_install();
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(3) {
        let installing = INSTALL_RUN
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|r| r.status.lock().unwrap().status == "running");
        if !running() && !installing {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
pub fn start_task(prompt: String) -> Result<(), String> {
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
    let exe = executable().ok_or("agents.not_installed")?;
    let (mut c, source) = library_runtime(&exe)?;
    let dir = home()?;
    let record = record(&dir)?.ok_or("agents.not_connected")?;
    let bytes = std::fs::read(dir.join("config.yaml")).map_err(|e| e.to_string())?;
    if hash(&bytes) != record.applied_hash {
        return Err("agents.config_changed".into());
    }
    let value = config(&bytes)?;
    // A clean upstream home avoids user hooks, MCP servers, skills and credential fallbacks.
    let isolated = tempfile::tempdir().map_err(|e| e.to_string())?;
    JsonStore::sensitive(isolated.path().join("config.yaml")).write_bytes(serde_norway::to_string(&json!({"model": value["model"], "toolsets": [], "agent": {"max_turns": 2}, "skills": {"auto_load": []}})).map_err(|e| e.to_string())?.as_bytes())?;
    JsonStore::sensitive(isolated.path().join("harness-task.json"))
        .save(&json!({"model": value["model"], "prompt": prompt}))?;
    let secret = value["model"]["api_key"].as_str().unwrap_or("").to_string();
    let run = Arc::new(Run {
        status: Mutex::new(TaskStatus {
            status: "running".into(),
            ..Default::default()
        }),
        cancel: Arc::new(AtomicBool::new(false)),
    });
    *task = Some(run.clone());
    std::thread::spawn(move || {
        c.current_dir(isolated.path()).env_clear();
        for key in [
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
        ] {
            if let Some(v) = std::env::var_os(key) {
                c.env(key, v);
            }
        }
        c.env("HARNESS_HERMES_SOURCE", source)
            .env("HERMES_HOME", isolated.path())
            .env("PYTHONIOENCODING", "utf-8")
            .env("PYTHONUNBUFFERED", "1");
        let result = run_process(c, &run, &secret, Duration::from_secs(600));
        let mut status = run.status.lock().unwrap();
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
    });
    Ok(())
}
fn run_process(
    mut c: Command,
    run: &Arc<Run>,
    secret: &str,
    timeout: Duration,
) -> Result<bool, String> {
    c.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = c.group_spawn().map_err(|e| e.to_string())?;
    let mut readers = Vec::new();
    for pipe in [
        child
            .inner()
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
        child
            .inner()
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    ]
    .into_iter()
    .flatten()
    {
        let run = run.clone();
        let secret = secret.to_string();
        readers.push(std::thread::spawn(move || {
            for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                let line = if secret.is_empty() {
                    line
                } else {
                    line.replace(&secret, "[redacted]")
                };
                let mut status = run.status.lock().unwrap();
                status.output.push_str(&line);
                status.output.push('\n');
                if status.output.len() > 65536 {
                    let mut end = status.output.len() - 65536;
                    while !status.output.is_char_boundary(end) {
                        end += 1;
                    }
                    status.output.drain(..end);
                }
            }
        }));
    }
    let start = Instant::now();
    let result = loop {
        if run.cancel.load(Ordering::SeqCst) || start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            break Err("agents.timeout".into());
        }
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break Ok(status.success());
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    for reader in readers {
        let _ = reader.join();
    }
    result
}
pub async fn install() -> Result<(), String> {
    if detect()?.installed {
        return Ok(());
    }
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
pub fn cancel_install() {
    if let Some(r) = INSTALL_RUN.lock().unwrap().as_ref() {
        r.cancel.store(true, Ordering::SeqCst);
    }
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
    let mut c = command(if windows { "powershell" } else { "bash" });
    if windows {
        c.args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&script)
        .args(["-NonInteractive", "-SkipBrowser", "-SkipComputerUse"]);
    } else {
        c.arg(&script)
            .args(["--non-interactive", "--skip-browser", "--skip-computer-use"]);
    }
    let run = run.clone();
    let success =
        tokio::task::spawn_blocking(move || run_process(c, &run, "", Duration::from_secs(1800)))
            .await
            .map_err(|e| e.to_string())??;
    if !success {
        return Err("agents.install_failed".into());
    }
    *PROBE.lock().unwrap() = None;
    if !detect()?.installed {
        return Err("agents.not_installed".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
