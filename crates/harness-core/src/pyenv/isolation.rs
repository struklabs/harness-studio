//! Network isolation of Python processes.
//!
//! A Python engine loads third-party model code and packages, so its processes get no network
//! interfaces where the OS allows it:
//!
//! * **Linux:** the process runs in a new user + network namespace (`unshare -rn`). Inside it
//!   the only interface is a loopback of its own that is down, so it cannot open TCP/UDP
//!   connections to the internet, the LAN or services on this machine's loopback. **Unix
//!   sockets are files, not network:** ones the user can reach (the D-Bus session bus,
//!   `/run/docker.sock` for members of the docker group...) stay reachable. A server process
//!   listens on a Unix socket in a private (0700) folder ([`socket_dir`]) and Harness relays
//!   its TCP port to it ([`relay`]). Kernels or distributions that forbid unprivileged user
//!   namespaces make [`status`] unavailable with the reason; an engine then refuses to start
//!   unless isolation is not required ([`command`]).
//! * **Windows / macOS:** no unprivileged equivalent; isolation is the environment
//!   ([`super::python_env`]: offline Hugging Face Hub, no user site-packages) and the loopback
//!   bind of the server.

use crate::error_key::err;
use serde::Serialize;
use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;
use std::sync::OnceLock;

/// Whether processes can be started without network interfaces here, and why not.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct IsolationStatus {
    pub available: bool,
    /// `unshare: ... Operation not permitted`, "not supported on this OS"...
    pub reason: Option<String>,
}

/// Probed once per run.
pub fn status() -> &'static IsolationStatus {
    static PROBE: OnceLock<IsolationStatus> = OnceLock::new();
    PROBE.get_or_init(probe)
}

pub fn available() -> bool {
    status().available
}

#[cfg(target_os = "linux")]
fn probe() -> IsolationStatus {
    match std::process::Command::new("unshare")
        .args(["--user", "--map-root-user", "--net", "--", "true"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
    {
        Ok(o) if o.status.success() => IsolationStatus {
            available: true,
            reason: None,
        },
        Ok(o) => IsolationStatus {
            available: false,
            reason: Some(
                String::from_utf8_lossy(&o.stderr)
                    .trim()
                    .chars()
                    .take(300)
                    .collect::<String>(),
            )
            .filter(|r| !r.is_empty())
            .or_else(|| Some(format!("unshare exited with {}", o.status))),
        },
        Err(e) => IsolationStatus {
            available: false,
            reason: Some(format!("unshare: {e}")),
        },
    }
}

#[cfg(not(target_os = "linux"))]
fn probe() -> IsolationStatus {
    IsolationStatus {
        available: false,
        reason: Some(format!("not supported on {}", std::env::consts::OS)),
    }
}

/// A command that runs `program` without network interfaces when that is [`available`]. When
/// it is not: `program` itself, or with `required` the error `pyenv.isolation_unavailable`
/// carrying the reason. The second value says whether the command is isolated.
pub fn command(program: &Path, required: bool) -> Result<(tokio::process::Command, bool), String> {
    command_raw(program, required).map(|(mut cmd, isolated)| {
        crate::appimage::foreign_tokio(&mut cmd);
        (cmd, isolated)
    })
}

fn command_raw(program: &Path, required: bool) -> Result<(tokio::process::Command, bool), String> {
    let st = status();
    if st.available {
        let mut cmd = tokio::process::Command::new("unshare");
        cmd.args(["--user", "--map-root-user", "--net", "--"])
            .arg(program);
        return Ok((cmd, true));
    }
    if required {
        return Err(err("pyenv.isolation_unavailable")
            .with("reason", st.reason.as_deref().unwrap_or(""))
            .into());
    }
    Ok((tokio::process::Command::new(program), false))
}

/// A command for a local tool that needs no network (sherpa-onnx): without network interfaces
/// when that is [`available`], else as before, said once on stderr.
pub fn offline_command(program: &Path) -> tokio::process::Command {
    let mut cmd = offline_command_raw(program);
    crate::appimage::foreign_tokio(&mut cmd);
    cmd
}

fn offline_command_raw(program: &Path) -> tokio::process::Command {
    let st = status();
    if st.available && cfg!(target_os = "linux") {
        let mut cmd = tokio::process::Command::new("unshare");
        cmd.args(["--user", "--map-root-user", "--net", "--"])
            .arg(program);
        return cmd;
    }
    if cfg!(target_os = "linux") {
        static LOGGED: std::sync::Once = std::sync::Once::new();
        LOGGED.call_once(|| {
            eprintln!(
                "[Harness Studio] Network isolation unavailable, local tools keep network access: {}",
                st.reason.as_deref().unwrap_or("")
            )
        });
    }
    tokio::process::Command::new(program)
}

/// A folder only this user can enter (0700), for the Unix socket of one server process: other
/// local users cannot connect to the worker through it.
#[cfg(unix)]
pub fn socket_dir(parent: &Path, name: &str) -> Result<PathBuf, String> {
    use std::os::unix::fs::PermissionsExt;
    let dir = parent.join(name);
    super::create_dir(&dir)?;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).map_err(|e| {
        String::from(
            err("fs.write_failed")
                .with("path", dir.display())
                .with("error", e),
        )
    })?;
    Ok(dir)
}

/// Relays `127.0.0.1:<port>` connections to the Unix socket a server process listens on inside
/// its namespace. Runs until the returned task is aborted (when the server stops).
///
/// The relay passes bytes through and does not authenticate: like every local engine port, the
/// TCP side is reachable by any process of this machine. The worker behind it checks the
/// per-run API key Harness gives it (as `llama-server --api-key` does), so only Harness and the
/// gateway can use it.
#[cfg(unix)]
pub fn relay(listener: tokio::net::TcpListener, socket: PathBuf) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let mut tcp = match listener.accept().await {
                Ok((tcp, _)) => tcp,
                // Out of file descriptors and the like: wait instead of spinning.
                Err(_) => {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    continue;
                }
            };
            let socket = socket.clone();
            tokio::spawn(async move {
                if let Ok(mut unix) = tokio::net::UnixStream::connect(&socket).await {
                    let _ = tokio::io::copy_bidirectional(&mut tcp, &mut unix).await;
                }
            });
        }
    })
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn isolated_process_cannot_reach_this_machine() {
        if !available() {
            eprintln!("user namespaces are not available here; skipped");
            return;
        }
        // A port that is open on this machine's loopback...
        let host = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = host.local_addr().unwrap().port();
        // ...is out of reach from inside, where the only interface is a loopback of its own
        // (`/proc/net/dev` is per namespace; `/sys/class/net` shows the host's mount).
        let (mut cmd, isolated) = command(Path::new("bash"), true).unwrap();
        assert!(isolated);
        let script = format!(
            "tail -n +3 /proc/net/dev | cut -d: -f1 | tr -d ' '; \
             (exec 3<>/dev/tcp/127.0.0.1/{port}) 2>/dev/null && echo reached || echo blocked"
        );
        let out = cmd.args(["-c", &script]).output().await.unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines, ["lo", "blocked"], "{text}");
        drop(host);
    }

    #[tokio::test]
    async fn offline_tools_cannot_reach_this_machine() {
        let host = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = host.local_addr().unwrap().port();
        let script = format!(
            "(exec 3<>/dev/tcp/127.0.0.1/{port}) 2>/dev/null && echo reached || echo blocked"
        );
        let out = offline_command(Path::new("bash"))
            .args(["-c", &script])
            .output()
            .await
            .unwrap();
        let want = if available() { "blocked" } else { "reached" };
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), want);
        drop(host);
    }

    #[test]
    fn socket_dir_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let d = socket_dir(dir.path(), "worker-1").unwrap();
        let mode = std::fs::metadata(&d).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }

    #[test]
    fn status_explains_an_unavailable_probe() {
        let st = status();
        assert_eq!(st.available, st.reason.is_none(), "{st:?}");
    }

    #[tokio::test]
    async fn relay_reaches_a_unix_socket_server() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("worker.sock");
        let server = tokio::net::UnixListener::bind(&sock).unwrap();
        tokio::spawn(async move {
            let (mut s, _) = server.accept().await.unwrap();
            let mut buf = [0u8; 4];
            s.read_exact(&mut buf).await.unwrap();
            s.write_all(&buf.map(|b| b.to_ascii_uppercase()))
                .await
                .unwrap();
        });
        let tcp = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = tcp.local_addr().unwrap();
        let task = relay(tcp, sock);
        let mut c = tokio::net::TcpStream::connect(addr).await.unwrap();
        c.write_all(b"ping").await.unwrap();
        let mut got = [0u8; 4];
        c.read_exact(&mut got).await.unwrap();
        assert_eq!(&got, b"PING");
        task.abort();
    }
}
