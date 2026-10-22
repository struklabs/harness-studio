//! Runs the real `harness` binary against temporary data directories (never the user's) with
//! an in-memory secret store (never the user's keychain).

#![allow(dead_code)]

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

pub fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_harness")
}

pub fn llama_server() -> Option<PathBuf> {
    std::env::var_os("HS_LLAMA_SERVER")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
}

pub fn test_model() -> Option<PathBuf> {
    std::env::var_os("HS_TEST_MODEL")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
}

/// A command with a clean environment: the given data dir, no remote host, memory secrets.
pub fn cmd(data_dir: &Path) -> Command {
    let mut c = Command::new(bin());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    c.env("HARNESS_DATA_DIR", data_dir)
        .env("HARNESS_MEMORY_SECRETS", "1")
        .env("HOME", data_dir.join("home"))
        .env("USERPROFILE", data_dir.join("home"))
        .env("APPDATA", data_dir.join("appdata"))
        .env("LOCALAPPDATA", data_dir.join("localappdata"))
        .env("XDG_CONFIG_HOME", data_dir.join("home/.config"))
        .env("XDG_DATA_HOME", data_dir.join("home/.local/share"))
        .env("CODEX_HOME", data_dir.join("home/.codex"))
        .env("CLAUDE_CONFIG_DIR", data_dir.join("home/.claude"))
        .env_remove("HARNESS_HOST")
        .env_remove("HARNESS_TOKEN")
        .env_remove("HARNESS_SERVER_API_KEY")
        .stdin(Stdio::null());
    c
}

pub struct Run {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Run {
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.stdout).unwrap_or_else(|e| {
            panic!(
                "stdout is not JSON ({e}):\n{}\n{}",
                self.stdout, self.stderr
            )
        })
    }

    /// The `--json` error on stderr.
    pub fn error(&self) -> Value {
        let line = self
            .stderr
            .lines()
            .rev()
            .find(|l| l.starts_with('{'))
            .unwrap_or_else(|| panic!("no JSON error on stderr:\n{}", self.stderr));
        serde_json::from_str::<Value>(line).unwrap()["error"].clone()
    }
}

impl From<Output> for Run {
    fn from(o: Output) -> Self {
        Run {
            code: o.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
        }
    }
}

pub fn run(data_dir: &Path, args: &[&str]) -> Run {
    cmd(data_dir).args(args).output().unwrap().into()
}

/// An OS-assigned loopback port at or above `from` (tests keep to 18500+).
/// Callers starting a process must retry bind conflicts, as `Serve` does.
pub fn free_port(from: u16) -> u16 {
    loop {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        if port >= from && ![11435, 11440, 11500].contains(&port) {
            return port;
        }
    }
}

/// A temp data dir; `llama` = configure the real llama-server with default server port `port`.
pub fn data_dir(llama: Option<&Path>, port: u16) -> tempfile::TempDir {
    let dir = tempfile::Builder::new()
        .prefix(&harness_core::paths::test_dir_prefix("cli-test"))
        .tempdir()
        .unwrap();
    let mut settings = serde_json::json!({
        "schema_version": 3,
        "server": {"host": "127.0.0.1", "port": port, "default_ngl": 99, "default_ctx": 2048},
    });
    if let Some(bin) = llama {
        settings["runtime_mode"] = "Custom".into();
        settings["custom_binary"] = serde_json::json!({"binary_path": bin, "is_valid": true});
    }
    std::fs::create_dir_all(dir.path().join("config")).unwrap();
    std::fs::write(
        dir.path().join("config/settings.json"),
        serde_json::to_string_pretty(&settings).unwrap(),
    )
    .unwrap();
    dir
}

/// A running `harness serve` (killed on drop if a test fails before shutting it down).
pub struct Serve {
    pub child: Child,
    pub ready: Value,
    pub port: u16,
    lines: std::sync::mpsc::Receiver<String>,
}

impl Serve {
    pub fn start(data_dir: &Path, port: u16) -> Serve {
        Self::start_with(data_dir, port, &[])
    }

    /// `start` with extra `serve` arguments.
    pub fn start_with(data_dir: &Path, port: u16, extra: &[&str]) -> Serve {
        Self::start_options(data_dir, Some(port), extra)
    }

    /// Starts without a port override, exercising settings/default selection.
    pub fn start_auto(data_dir: &Path, extra: &[&str]) -> Serve {
        Self::start_options(data_dir, None, extra)
    }

    fn start_options(data_dir: &Path, mut port: Option<u16>, extra: &[&str]) -> Serve {
        let mut extra: Vec<String> = extra.iter().map(|s| (*s).to_owned()).collect();
        for attempt in 0..5 {
            let mut command = cmd(data_dir);
            command.args(["serve", "--json"]);
            if let Some(port) = port {
                command.args(["--port", &port.to_string()]);
            }
            let mut child = command
                .args(&extra)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let stderr = child.stderr.take().unwrap();
            let errors = std::thread::spawn(move || std::io::read_to_string(stderr).unwrap());
            let out = BufReader::new(child.stdout.take().unwrap());
            let (tx, rx) = std::sync::mpsc::channel::<String>();
            std::thread::spawn(move || {
                for line in out.lines().map_while(Result::ok) {
                    let _ = tx.send(line);
                }
            });
            let deadline = Instant::now() + Duration::from_secs(30);
            let ready = loop {
                match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                    Ok(line) if line.contains("\"ready\"") => {
                        break Some(serde_json::from_str::<Value>(&line).unwrap());
                    }
                    Ok(_) => {}
                    Err(_) => break None,
                }
            };
            let gateway_error = ready
                .as_ref()
                .and_then(|r| r["gateway"]["error"].as_str())
                .unwrap_or_default()
                .to_owned();
            if let Some(ready) = ready.filter(|_| gateway_error.is_empty()) {
                let port = reqwest::Url::parse(ready["base_url"].as_str().unwrap())
                    .unwrap()
                    .port()
                    .unwrap();
                return Serve {
                    child,
                    ready,
                    port,
                    lines: rx,
                };
            }
            let _ = child.kill();
            let _ = child.wait();
            let stderr = errors.join().unwrap();
            let error = format!("{gateway_error}\n{stderr}");
            let conflict = error.contains("gateway.port_conflict")
                || ((error.contains("api.bind_failed") || error.contains("gateway.bind_failed"))
                    && error.contains("is in use"));
            assert!(conflict && attempt < 4, "serve failed: {error}");
            port = port.map(|_| free_port(18500));
            if let Some(i) = extra.iter().position(|s| s == "--gateway-port") {
                extra[i + 1] = free_port(19000).to_string();
            }
        }
        unreachable!()
    }

    pub fn gateway_port(&self) -> u16 {
        reqwest::Url::parse(self.ready["gateway"]["base_url"].as_str().unwrap())
            .unwrap()
            .port()
            .unwrap()
    }

    /// Waits for the process to exit after a shutdown request; returns its exit code and the
    /// last stdout line.
    pub fn wait_exit(&mut self, timeout: Duration) -> (i32, Option<String>) {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                let last = self.lines.recv_timeout(Duration::from_secs(2)).ok();
                return (status.code().unwrap_or(-1), last);
            }
            assert!(Instant::now() < deadline, "serve did not exit");
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

impl Drop for Serve {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// Whether something accepts connections on a loopback port.
pub fn port_open(port: u16) -> bool {
    std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        Duration::from_millis(300),
    )
    .is_ok()
}

/// Starts an external HTTP fixture on a port it must know before spawning.
pub fn start_http_process(mut command: impl FnMut(u16) -> Command) -> (Child, u16) {
    for attempt in 0..5 {
        let port = free_port(18500);
        let mut child = command(port)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stderr = child.stderr.take().unwrap();
        let errors = std::thread::spawn(move || std::io::read_to_string(stderr).unwrap());
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            if child.try_wait().unwrap().is_some() {
                let logs = errors.join().unwrap();
                let conflict = logs.contains("EADDRINUSE")
                    || logs.contains("AddrInUse")
                    || logs.to_lowercase().contains("address already in use");
                assert!(conflict && attempt < 4, "HTTP fixture exited: {logs}");
                break;
            }
            if port_open(port) {
                return (child, port);
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!(
                    "HTTP fixture did not become ready: {}",
                    errors.join().unwrap()
                );
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    unreachable!()
}
