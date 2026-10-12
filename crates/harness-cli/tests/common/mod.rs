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
    c.env("HARNESS_DATA_DIR", data_dir)
        .env("HARNESS_MEMORY_SECRETS", "1")
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

/// A free loopback port at or above `from` (tests keep to 18500+).
pub fn free_port(from: u16) -> u16 {
    (from..from + 400)
        .find(|p| std::net::TcpListener::bind(("127.0.0.1", *p)).is_ok())
        .expect("free port")
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
        let mut child = cmd(data_dir)
            .args(["serve", "--json", "--port", &port.to_string()])
            .args(extra)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut out = BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            line.clear();
            assert!(out.read_line(&mut line).unwrap() > 0, "serve exited early");
            if line.contains("\"ready\"") {
                break;
            }
            assert!(Instant::now() < deadline, "serve not ready");
        }
        let ready: Value = serde_json::from_str(line.trim()).unwrap();
        // Keep reading stdout (the `stopped` line) on a thread.
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        std::thread::spawn(move || {
            for l in out.lines().map_while(Result::ok) {
                let _ = tx.send(l);
            }
        });
        Serve {
            child,
            ready,
            port,
            lines: rx,
        }
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
