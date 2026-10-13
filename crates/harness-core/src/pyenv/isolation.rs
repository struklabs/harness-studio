//! Network isolation of Python processes.
//!
//! A Python engine loads third-party model code and packages, so its processes get no network
//! where the OS allows it:
//!
//! * **Linux:** the process runs in a new user + network namespace (`unshare -rn`). Inside it
//!   there is only a loopback interface that is down, so nothing can connect anywhere, not even
//!   to other services on this machine. A server process therefore listens on a Unix socket
//!   (a file, reachable across the namespace boundary) and Harness relays its TCP port to it
//!   ([`relay`]). Kernels or distributions that forbid unprivileged user namespaces make
//!   [`available`] false; the process then runs with [`super::python_env`] only.
//! * **Windows / macOS:** no unprivileged equivalent; isolation is the environment
//!   ([`super::python_env`]: offline Hugging Face Hub, no user site-packages) and the loopback
//!   bind of the server.

use std::path::Path;
use std::sync::OnceLock;

/// Whether processes can be started without network here (probed once per run).
pub fn available() -> bool {
    static PROBE: OnceLock<bool> = OnceLock::new();
    *PROBE.get_or_init(probe)
}

#[cfg(target_os = "linux")]
fn probe() -> bool {
    std::process::Command::new("unshare")
        .args(["--user", "--map-root-user", "--net", "--", "true"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(not(target_os = "linux"))]
fn probe() -> bool {
    false
}

/// A command that runs `program` without network when [`available`], else `program` itself.
/// The second value says which one it is.
pub fn command(program: &Path) -> (tokio::process::Command, bool) {
    if available() {
        let mut cmd = tokio::process::Command::new("unshare");
        cmd.args(["--user", "--map-root-user", "--net", "--"])
            .arg(program);
        (cmd, true)
    } else {
        (tokio::process::Command::new(program), false)
    }
}

/// Relays `127.0.0.1:<port>` connections to the Unix socket a server process listens on inside
/// its network namespace. Runs until the returned task is aborted (when the server stops).
#[cfg(unix)]
pub fn relay(
    listener: tokio::net::TcpListener,
    socket: std::path::PathBuf,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let Ok((mut tcp, _)) = listener.accept().await else {
                continue;
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
        let (mut cmd, isolated) = command(Path::new("bash"));
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
