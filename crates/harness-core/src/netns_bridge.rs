//! Network isolation of native servers that only listen on TCP (Linux).
//!
//! A Python worker listens on a Unix socket inside its namespace without network
//! ([`crate::pyenv::isolation`]); CrispASR, audio.cpp and the ONNX sidecar cannot. They run
//! behind this bridge instead: Harness starts
//!
//! ```text
//! unshare --user --map-root-user --net -- <harness executable> --harness-netns-bridge \
//!     <socket> <host> <port> -- <server> <args...>
//! ```
//!
//! and the executable, before anything else in `main`, calls [`child_main`]. Inside the new
//! namespace the bridge brings up its private loopback (the only interface there), starts the
//! server, which binds `<host>:<port>` on that loopback, and passes each connection of the one
//! Unix socket `<socket>` to it. Outside, [`crate::pyenv::isolation::relay`] passes the real
//! TCP port to the socket. The bridge opens no other socket and asks for nothing beyond what
//! `unshare -r` gives it inside its own namespace: the server gets no route to the internet,
//! the LAN or services on this machine's loopback.

use std::ffi::OsString;

/// First argument of a bridge run.
pub const FLAG: &str = "--harness-netns-bridge";

/// The bridge's arguments after `program` (`unshare ... -- <program>`): `socket`, the server's
/// `host` and `port` inside the namespace, then `--`, the server and its own arguments.
pub fn bridge_args(socket: &std::path::Path, host: &str, port: u16) -> Vec<OsString> {
    vec![
        FLAG.into(),
        socket.as_os_str().to_owned(),
        host.into(),
        port.to_string().into(),
        "--".into(),
    ]
}

/// A bridge run of this executable: runs it and returns the exit code (the server's). `None`
/// for every other run. Call first in `main`, before any thread starts.
pub fn child_main() -> Option<i32> {
    let mut args = std::env::args_os().skip(1);
    if args.next()? != FLAG {
        return None;
    }
    let args: Vec<OsString> = args.collect();
    #[cfg(target_os = "linux")]
    {
        Some(match linux::run(&args) {
            Ok(code) => code,
            Err(e) => {
                eprintln!("[Harness Studio] Network bridge: {e}");
                125
            }
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = args;
        eprintln!("[Harness Studio] Network bridge: Linux only");
        Some(125)
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::ffi::OsString;
    use std::io;
    use std::net::TcpStream;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::os::unix::process::{CommandExt, ExitStatusExt};
    use std::path::PathBuf;
    use std::process::Command;

    pub fn run(args: &[OsString]) -> Result<i32, String> {
        let [socket, host, port, sep, program, rest @ ..] = args else {
            return Err("usage: <socket> <host> <port> -- <program> [args...]".into());
        };
        if sep != "--" {
            return Err("expected `--` before the program".into());
        }
        let socket = PathBuf::from(socket);
        let host = host.to_str().ok_or("host is not UTF-8")?;
        let host = host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_string();
        let port: u16 = port
            .to_str()
            .and_then(|p| p.parse().ok())
            .ok_or("port is not a number")?;
        die_with_parent().map_err(|e| format!("PR_SET_PDEATHSIG: {e}"))?;
        loopback_up().map_err(|e| format!("loopback: {e}"))?;
        let _ = std::fs::remove_file(&socket);
        let listener =
            UnixListener::bind(&socket).map_err(|e| format!("bind {}: {e}", socket.display()))?;
        let mut cmd = Command::new(program);
        cmd.args(rest);
        // SAFETY: only async-signal-safe libc calls between fork and exec.
        unsafe {
            cmd.pre_exec(die_with_parent);
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("{}: {e}", PathBuf::from(program).display()))?;
        std::thread::spawn(move || {
            for conn in listener.incoming() {
                let Ok(conn) = conn else { continue };
                let host = host.clone();
                std::thread::spawn(move || pipe(conn, &host, port));
            }
        });
        let status = child.wait().map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(&socket);
        Ok(status
            .code()
            .unwrap_or_else(|| 128 + status.signal().unwrap_or(0)))
    }

    /// The bridge and the server die with their parent (Harness, or the bridge).
    fn die_with_parent() -> io::Result<()> {
        // SAFETY: plain syscalls.
        unsafe {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) == -1 {
                return Err(io::Error::last_os_error());
            }
            // The parent may have died before PR_SET_PDEATHSIG.
            if libc::getppid() == 1 {
                libc::raise(libc::SIGKILL);
            }
        }
        Ok(())
    }

    /// A new network namespace's loopback is down (and without addresses until it is up).
    pub(super) fn loopback_up() -> io::Result<()> {
        // SAFETY: an ioctl on a socket this function owns, with a zeroed `ifreq` naming `lo`.
        unsafe {
            let fd = libc::socket(libc::AF_INET, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, 0);
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            let mut req: libc::ifreq = std::mem::zeroed();
            for (d, s) in req.ifr_name.iter_mut().zip(b"lo\0") {
                *d = *s as libc::c_char;
            }
            let mut r = libc::ioctl(fd, libc::SIOCGIFFLAGS, &mut req);
            if r == 0 {
                req.ifr_ifru.ifru_flags |= libc::IFF_UP as libc::c_short;
                r = libc::ioctl(fd, libc::SIOCSIFFLAGS, &req);
            }
            let e = io::Error::last_os_error();
            libc::close(fd);
            if r != 0 {
                return Err(e);
            }
        }
        Ok(())
    }

    /// One connection of the socket to the server (closed when either side ends).
    fn pipe(unix: UnixStream, host: &str, port: u16) {
        let Ok(tcp) = TcpStream::connect((host, port)) else {
            return;
        };
        let (Ok(mut unix_r), Ok(mut tcp_w)) = (unix.try_clone(), tcp.try_clone()) else {
            return;
        };
        let up = std::thread::spawn(move || {
            let _ = io::copy(&mut unix_r, &mut tcp_w);
            let _ = tcp_w.shutdown(std::net::Shutdown::Write);
        });
        let (mut tcp_r, mut unix_w) = (tcp, unix);
        let _ = io::copy(&mut tcp_r, &mut unix_w);
        let _ = unix_w.shutdown(std::net::Shutdown::Write);
        let _ = up.join();
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    //! The test binary runs itself as the bridge and as the server inside a namespace (each
    //! child test below does nothing in a normal run).
    use std::io::{Read, Write};
    use std::process::Command;
    use std::time::{Duration, Instant};

    const BRIDGE_ENV: &str = "HS_NETNS_TEST_BRIDGE";
    const SERVER_ENV: &str = "HS_NETNS_TEST_SERVER";
    const INNER_PORT: u16 = 18123;

    fn libtest_args(test: &str) -> Vec<String> {
        vec![
            "--exact".into(),
            test.into(),
            "--nocapture".into(),
            "--test-threads=1".into(),
        ]
    }

    #[test]
    fn bridge_child() {
        let Ok(args) = std::env::var(BRIDGE_ENV) else {
            return;
        };
        let args: Vec<std::ffi::OsString> = serde_json::from_str::<Vec<String>>(&args)
            .unwrap()
            .into_iter()
            .map(Into::into)
            .collect();
        std::process::exit(super::linux::run(&args).unwrap_or(125));
    }

    /// Answers each connection with whether a listener outside the namespace was reachable.
    #[test]
    fn server_child() {
        let Ok(outside) = std::env::var(SERVER_ENV) else {
            return;
        };
        let listener = std::net::TcpListener::bind(("127.0.0.1", INNER_PORT)).unwrap();
        for conn in listener.incoming() {
            let mut conn = conn.unwrap();
            let addr: std::net::SocketAddr = outside.parse().unwrap();
            let reach = std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(1));
            let net = std::net::TcpStream::connect_timeout(
                &"1.1.1.1:53".parse().unwrap(),
                Duration::from_secs(1),
            );
            let _ = write!(
                conn,
                "outside={} internet={}",
                if reach.is_ok() { "open" } else { "closed" },
                if net.is_ok() { "open" } else { "closed" }
            );
        }
    }

    // Blocking reads below: the relay needs a thread of its own.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn bridged_server_has_no_network_but_answers_through_the_relay() {
        if !crate::pyenv::isolation::available() {
            eprintln!("skipped: {:?}", crate::pyenv::isolation::status().reason);
            return;
        }
        // A service on this machine's loopback that the server must not reach.
        let outside = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let outside_addr = outside.local_addr().unwrap().to_string();
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("w.sock");
        let exe = std::env::current_exe().unwrap();
        let mut bridge: Vec<String> = super::bridge_args(&socket, "127.0.0.1", INNER_PORT)
            .into_iter()
            .map(|a| a.into_string().unwrap())
            .skip(1)
            .collect();
        bridge.push(exe.display().to_string());
        bridge.extend(libtest_args("netns_bridge::tests::server_child"));
        let mut child = Command::new("unshare")
            .args(["--user", "--map-root-user", "--net", "--"])
            .arg(&exe)
            .args(libtest_args("netns_bridge::tests::bridge_child"))
            .env(BRIDGE_ENV, serde_json::to_string(&bridge).unwrap())
            .env(SERVER_ENV, &outside_addr)
            .spawn()
            .unwrap();
        let relay_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let relay_port = relay_listener.local_addr().unwrap().port();
        let relay = crate::pyenv::isolation::relay(relay_listener, socket.clone());
        let deadline = Instant::now() + Duration::from_secs(20);
        let answer = loop {
            let mut s = std::net::TcpStream::connect(("127.0.0.1", relay_port)).unwrap();
            let mut answer = String::new();
            let _ = s.read_to_string(&mut answer);
            if !answer.is_empty() || Instant::now() > deadline {
                break answer;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        };
        relay.abort();
        let _ = child.kill();
        let _ = child.wait();
        // Reachable from here: only the namespace keeps the server from it.
        assert!(std::net::TcpStream::connect(&outside_addr).is_ok());
        assert_eq!(answer, "outside=closed internet=closed");
    }

    #[test]
    fn other_runs_are_not_bridge_runs() {
        assert_eq!(super::child_main(), None);
    }
}
