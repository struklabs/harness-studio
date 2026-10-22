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
//! * **macOS:** the process runs under a Seatbelt profile (`sandbox-exec`, [`SANDBOX_PROFILE`])
//!   that denies every outgoing IP connection, to the internet, the LAN and this machine's
//!   loopback alike; listening (on a Unix socket or its loopback port) and everything else stay
//!   allowed, so a server listens on its loopback port with no socket relay. `sandbox-exec` is
//!   deprecated by Apple but ships with every macOS; where it fails,
//!   [`status`] is unavailable with the reason and the process runs as on Windows.
//! * **Windows:** no unprivileged equivalent; isolation is the environment
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
    match crate::process::hidden_command("unshare")
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

/// Seatbelt profile of an isolated process on macOS: no outgoing IP connection of any kind.
#[cfg(target_os = "macos")]
pub const SANDBOX_PROFILE: &str = "(version 1)(allow default)(deny network-outbound (remote ip))";

#[cfg(target_os = "macos")]
const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

#[cfg(target_os = "macos")]
fn probe() -> IsolationStatus {
    match crate::process::hidden_command(SANDBOX_EXEC)
        .args(["-p", SANDBOX_PROFILE, "/usr/bin/true"])
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
            .or_else(|| Some(format!("sandbox-exec exited with {}", o.status))),
        },
        Err(e) => IsolationStatus {
            available: false,
            reason: Some(format!("sandbox-exec: {e}")),
        },
    }
}

/// The program and the arguments before `program` that run it without network: `unshare` on
/// Linux, `sandbox-exec` on macOS. Only meaningful while [`available`].
fn wrapper() -> (&'static str, Vec<&'static str>) {
    #[cfg(target_os = "macos")]
    return (SANDBOX_EXEC, vec!["-p", SANDBOX_PROFILE]);
    #[cfg(not(target_os = "macos"))]
    ("unshare", vec!["--user", "--map-root-user", "--net", "--"])
}

/// `program` run without network (see [`wrapper`]); the caller checked [`available`].
pub fn isolated_std_command(program: &Path) -> std::process::Command {
    let (wrapper, args) = wrapper();
    let mut cmd = crate::process::hidden_command(wrapper);
    cmd.args(args).arg(program);
    cmd
}

fn isolated_command(program: &Path) -> tokio::process::Command {
    let (wrapper, args) = wrapper();
    let mut cmd = crate::process::hidden_tokio_command(wrapper);
    cmd.args(args).arg(program);
    cmd
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
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
        return Ok((isolated_command(program), true));
    }
    if required {
        return Err(err("pyenv.isolation_unavailable")
            .with("reason", st.reason.as_deref().unwrap_or(""))
            .into());
    }
    Ok((crate::process::hidden_tokio_command(program), false))
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
    if st.available {
        return isolated_command(program);
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
    crate::process::hidden_tokio_command(program)
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
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

    /// macOS: the sandboxed process reaches no port of this machine's loopback, yet listens on
    /// a Unix socket (an isolated worker's) and on a loopback port (an MLX worker's).
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn isolated_process_cannot_reach_this_machine() {
        assert!(available(), "{:?}", status());
        let host = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = host.local_addr().unwrap().port();
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("w.sock");
        let script = format!(
            "import socket\n\
             u = socket.socket(socket.AF_UNIX); u.bind({sock:?}); u.listen(); print('unix')\n\
             t = socket.socket(); t.bind(('127.0.0.1', 0)); t.listen(); print('tcp')\n\
             try:\n    socket.create_connection(('127.0.0.1', {port}), timeout=3); print('reached')\n\
             except OSError:\n    print('blocked')\n",
            sock = sock.to_string_lossy(),
        );
        let (mut cmd, isolated) = command(Path::new("/usr/bin/python3"), true).unwrap();
        assert!(isolated);
        let out = cmd.args(["-c", &script]).output().await.unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert_eq!(
            text.lines().collect::<Vec<_>>(),
            ["unix", "tcp", "blocked"],
            "{text}"
        );
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
