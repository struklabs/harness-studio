//! The stdio transport: the server is a child process, one JSON-RPC message per line on its
//! stdin/stdout, its stderr is only logged (kept as a short tail for error messages).
//!
//! The child runs in its own process group (a Windows Job Object that kills the whole tree
//! when the handle closes, a process group elsewhere) and without a console window, so closing
//! the connection or dropping it never leaves an `npx`/`uvx` grandchild behind.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use command_group::AsyncCommandGroup;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot};

use super::McpError;

type Pending = Arc<Mutex<HashMap<i64, oneshot::Sender<Result<Value, McpError>>>>>;

const STDERR_TAIL: usize = 20;

/// How to start a stdio server.
#[derive(Debug, Clone, PartialEq)]
pub struct StdioSpec {
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: Option<std::path::PathBuf>,
}

pub struct StdioTransport {
    write_tx: Mutex<Option<mpsc::UnboundedSender<String>>>,
    pending: Pending,
    child: Mutex<Option<command_group::AsyncGroupChild>>,
    closed: Arc<AtomicBool>,
    pid: Option<u32>,
    #[cfg(windows)]
    _memory_job: Option<crate::process_limits::MemoryJob>,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
}

/// Removes a request from the pending table when its future is dropped (timeout, cancel).
struct PendingGuard {
    pending: Pending,
    id: i64,
}

impl Drop for PendingGuard {
    fn drop(&mut self) {
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.id);
    }
}

impl StdioTransport {
    /// Spawns the server. Messages the server sends that are not responses (notifications) go
    /// to `notifications`.
    pub fn spawn(
        spec: &StdioSpec,
        notifications: mpsc::UnboundedSender<Value>,
    ) -> Result<Self, McpError> {
        let mut cmd = tokio::process::Command::new(&spec.command);
        // macOS: Homebrew / Node folders a Finder-started app lacks (`npx`, `uvx`); set first
        // so the server's own `PATH` still wins.
        if let Some(path) = crate::integrations::launch::augmented_path() {
            cmd.env("PATH", path);
        }
        cmd.args(&spec.args)
            .envs(spec.env.iter().map(|(k, v)| (k, v)))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        if let Some(dir) = &spec.cwd {
            cmd.current_dir(dir);
        }
        crate::process_limits::prepare(cmd.as_std_mut())
            .map_err(|e| McpError::Transport(e.to_string()))?;
        let mut group = cmd.group();
        group.kill_on_drop(true);
        #[cfg(windows)]
        group.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        let mut child = group.spawn().map_err(|e| {
            McpError::Transport(format!("could not start \"{}\": {e}", spec.command))
        })?;
        #[cfg(windows)]
        let memory_job = {
            crate::process_limits::memory_job(child.inner().raw_handle().unwrap() as _)
                .map_err(|e| McpError::Transport(e.to_string()))?
        };
        let pid = child.id();
        let stdin = child
            .inner()
            .stdin
            .take()
            .ok_or_else(|| McpError::Transport("no stdin".into()))?;
        let stdout = child
            .inner()
            .stdout
            .take()
            .ok_or_else(|| McpError::Transport("no stdout".into()))?;
        let stderr = child
            .inner()
            .stderr
            .take()
            .ok_or_else(|| McpError::Transport("no stderr".into()))?;

        let pending: Pending = Arc::default();
        let closed = Arc::new(AtomicBool::new(false));
        let stderr_tail: Arc<Mutex<VecDeque<String>>> = Arc::default();
        let (write_tx, mut write_rx) = mpsc::unbounded_channel::<String>();

        // Writer: owns stdin; ends (closing it, the MCP shutdown signal) when the transport drops.
        tokio::spawn(async move {
            let mut stdin = stdin;
            while let Some(line) = write_rx.recv().await {
                if stdin.write_all(line.as_bytes()).await.is_err()
                    || stdin.write_all(b"\n").await.is_err()
                    || stdin.flush().await.is_err()
                {
                    break;
                }
            }
        });

        // stderr: a tail for error messages.
        {
            let tail = stderr_tail.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let mut t = tail.lock().unwrap_or_else(|e| e.into_inner());
                    if t.len() == STDERR_TAIL {
                        t.pop_front();
                    }
                    t.push_back(line);
                }
            });
        }

        // Reader: responses to the pending table, notifications onward, the few requests a
        // server may send (`ping`) answered here.
        {
            let pending = pending.clone();
            let closed = closed.clone();
            let tail = stderr_tail.clone();
            let reply_tx = write_tx.downgrade();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    let Ok(msg) = serde_json::from_str::<Value>(line) else {
                        continue; // not JSON-RPC (a stray log line): ignore
                    };
                    if let Some(method) = msg.get("method").and_then(Value::as_str) {
                        match msg.get("id") {
                            Some(id) => {
                                let reply = if method == "ping" {
                                    json!({"jsonrpc": "2.0", "id": id, "result": {}})
                                } else {
                                    json!({"jsonrpc": "2.0", "id": id, "error": {
                                        "code": -32601,
                                        "message": "Method not found: this client offers no sampling, roots or elicitation"
                                    }})
                                };
                                if let Some(tx) = reply_tx.upgrade() {
                                    let _ = tx.send(reply.to_string());
                                }
                            }
                            None => {
                                let _ = notifications.send(msg);
                            }
                        }
                        continue;
                    }
                    let Some(id) = msg.get("id").and_then(Value::as_i64) else {
                        continue;
                    };
                    let tx = pending
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .remove(&id);
                    if let Some(tx) = tx {
                        let r = match (msg.get("result"), msg.get("error")) {
                            (_, Some(e)) => Err(McpError::from_rpc_error(e)),
                            (Some(r), None) => Ok(r.clone()),
                            _ => Err(McpError::Protocol("response without result".into())),
                        };
                        let _ = tx.send(r);
                    }
                }
                // stdout closed: the process is gone (or closed its output).
                closed.store(true, Ordering::SeqCst);
                let why = {
                    let t = tail.lock().unwrap_or_else(|e| e.into_inner());
                    t.iter().cloned().collect::<Vec<_>>().join("\n")
                };
                let drained: Vec<_> = pending
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .drain()
                    .collect();
                for (_, tx) in drained {
                    let _ = tx.send(Err(McpError::Closed(why.clone())));
                }
            });
        }

        Ok(Self {
            write_tx: Mutex::new(Some(write_tx)),
            pending,
            child: Mutex::new(Some(child)),
            closed,
            pid,
            #[cfg(windows)]
            _memory_job: memory_job,
            stderr_tail,
        })
    }

    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    pub fn is_closed(&self) -> bool {
        if self.closed.load(Ordering::SeqCst) {
            return true;
        }
        // The reader notices a dead process a moment late; ask the process itself. An error
        // means it can no longer be waited for (on Linux the runtime may have reaped it
        // already: ECHILD), so it is gone too.
        let mut child = self.child.lock().unwrap_or_else(|e| e.into_inner());
        match child.as_mut() {
            Some(c) => !matches!(c.try_wait(), Ok(None)),
            None => true,
        }
    }

    /// The last lines the server wrote to stderr.
    pub fn stderr_tail(&self) -> String {
        self.stderr_tail
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Sends a request and waits for its response. Dropping the future forgets the request
    /// (the caller sends `notifications/cancelled`).
    pub async fn request(&self, id: i64, msg: &Value) -> Result<Value, McpError> {
        if self.is_closed() {
            return Err(McpError::Closed(self.stderr_tail()));
        }
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, tx);
        let _guard = PendingGuard {
            pending: self.pending.clone(),
            id,
        };
        self.write(msg)?;
        match rx.await {
            Ok(r) => r,
            Err(_) => Err(McpError::Closed(self.stderr_tail())),
        }
    }

    pub fn notify(&self, msg: &Value) -> Result<(), McpError> {
        self.write(msg)
    }

    fn write(&self, msg: &Value) -> Result<(), McpError> {
        let guard = self.write_tx.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(tx) => tx
                .send(msg.to_string())
                .map_err(|_| McpError::Closed(self.stderr_tail())),
            None => Err(McpError::Closed(self.stderr_tail())),
        }
    }

    /// Ends the server: stdin closes first (the protocol's shutdown signal), then the whole
    /// process tree is killed if it has not exited within `grace`.
    pub async fn close(&self, grace: Duration) {
        // Dropping the sender ends the writer task, which closes the child's stdin.
        self.write_tx
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        let child = self.child.lock().unwrap_or_else(|e| e.into_inner()).take();
        let Some(mut child) = child else { return };
        let _group_guard = self.pid.map(crate::process_limits::GroupGuard);
        let deadline = tokio::time::Instant::now() + grace;
        while tokio::time::Instant::now() < deadline {
            if matches!(child.try_wait(), Ok(Some(_))) {
                let _ = child.start_kill();
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let _ = child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
    }
}

impl Drop for StdioTransport {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.lock().unwrap_or_else(|e| e.into_inner()).take() {
            // Windows: dropping the child closes the Job Object, which kills every descendant.
            // Unix: kill the process group explicitly.
            let _ = child.start_kill();
        }
    }
}

#[cfg(all(test, windows))]
mod containment_tests {
    use super::*;
    use std::process::{Command, Stdio};
    use std::time::Instant;

    // A separate host uses the actual MCP transport. Killing it bypasses every Rust Drop.
    #[test]
    fn mcp_host_fixture() {
        let Ok(marker) = std::env::var("HARNESS_MCP_HOST_FIXTURE") else {
            return;
        };
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let script = "const cp=require('child_process'),fs=require('fs');const c=cp.spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{stdio:'ignore'});fs.writeFileSync(process.argv[1],JSON.stringify([process.pid,c.pid]));setInterval(()=>{},1000)";
            let (tx, _rx) = mpsc::unbounded_channel();
            let _transport = StdioTransport::spawn(&StdioSpec {
                command: "node".into(), args: vec!["-e".into(), script.into(), marker],
                env: vec![], cwd: None,
            }, tx).unwrap();
            std::future::pending::<()>().await;
        });
    }

    #[test]
    fn killing_host_kills_mcp_child_tree() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("pids.json");
        let mut host = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "tools::mcp_client::stdio::containment_tests::mcp_host_fixture",
                "--nocapture",
            ])
            .env("HARNESS_MCP_HOST_FIXTURE", &marker)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        let pids: Vec<u32> = loop {
            if let Ok(bytes) = std::fs::read(&marker) {
                if let Ok(pids) = serde_json::from_slice(&bytes) {
                    break pids;
                }
            }
            if Instant::now() >= deadline {
                let _ = host.kill();
                let _ = host.wait();
                panic!("MCP host did not start its descendants");
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        assert_eq!(pids.len(), 2);
        use windows::Win32::{Foundation::*, System::Threading::*};
        // Hold process handles before killing, so PID reuse cannot invalidate the check.
        let handles: Vec<_> = pids
            .iter()
            .map(|pid| unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, *pid).unwrap() })
            .collect();
        host.kill().unwrap();
        host.wait().unwrap();
        for handle in handles {
            let result = unsafe { WaitForSingleObject(handle, 10000) };
            unsafe {
                CloseHandle(handle).unwrap();
            }
            assert_eq!(
                result, WAIT_OBJECT_0,
                "MCP descendant survived host termination"
            );
        }
    }
}
