//! Orphan-process protection on macOS.
//!
//! Windows puts llama-server in a Job Object with `KILL_ON_JOB_CLOSE` and Linux sets
//! `PR_SET_PDEATHSIG`, so the server dies with Harness Studio even after a crash. macOS has
//! neither. [`install`] adds a `pre_exec` hook that, in the forked child right before `exec`,
//! double-forks a tiny watchdog (reparented to launchd, so it outlives Harness). The watchdog
//! closes every inherited descriptor and waits on a kqueue for `NOTE_EXIT` of two processes:
//! - Harness Studio exits (normally or by crash) -> `SIGTERM` to the server's process group,
//!   `SIGKILL` 2 s later (the server is started as a group leader, so whatever it started, such
//!   as a Python worker's subprocesses, goes with it);
//! - the server exits -> the watchdog exits too.
//!
//! The watchdog is forked from the server before `exec`, so it starts in the server's group; it
//! moves to a session of its own first, so the group signal does not end the watchdog before its
//! `SIGKILL` and a clean stop of the group (SIGTERM to it) cannot end the watchdog either.
//!
//! Only async-signal-safe system calls run between `fork` and `exec` (fork, waitpid, _exit,
//! close, kqueue, kevent, kill, sleep). Not compiled or run on the development machine
//! (Windows); see private hs-internal architecture notes

#[cfg(target_os = "macos")]
pub fn install(cmd: &mut std::process::Command) {
    install_for(cmd, std::process::id() as libc::pid_t);
}

/// [`install`] watching `parent` instead of this process (tests stand in a parent they can end).
#[cfg(target_os = "macos")]
fn install_for(cmd: &mut std::process::Command, parent: libc::pid_t) {
    use std::os::unix::process::CommandExt;
    // SAFETY: the hook only makes async-signal-safe system calls (see module docs).
    unsafe {
        cmd.pre_exec(move || {
            // Still the forked child: this pid becomes llama-server after exec.
            let server = libc::getpid();
            let middle = libc::fork();
            if middle < 0 {
                return Ok(()); // best effort: start without a watchdog
            }
            if middle == 0 {
                if libc::fork() == 0 {
                    watch(parent, server);
                }
                libc::_exit(0);
            }
            let mut status = 0;
            libc::waitpid(middle, &mut status, 0);
            Ok(())
        });
    }
}

#[cfg(target_os = "macos")]
unsafe fn register_exit(kq: libc::c_int, pid: libc::pid_t) -> bool {
    let mut change: libc::kevent = std::mem::zeroed();
    change.ident = pid as libc::uintptr_t;
    change.filter = libc::EVFILT_PROC;
    change.flags = libc::EV_ADD | libc::EV_ONESHOT;
    change.fflags = libc::NOTE_EXIT;
    libc::kevent(kq, &change, 1, std::ptr::null_mut(), 0, std::ptr::null()) == 0
}

/// Signals the server's whole process group when it leads one, and the server itself in any
/// case.
#[cfg(target_os = "macos")]
unsafe fn signal_server(server: libc::pid_t, group: bool, sig: libc::c_int) {
    if group {
        libc::kill(-server, sig);
    }
    libc::kill(server, sig);
}

/// SIGTERM, then SIGKILL after 2 s. Whether the server leads its group (it does:
/// `command_group` starts it with `setpgid(0, 0)`) is decided once, before SIGTERM: a leader
/// that exits on SIGTERM can no longer be asked (ESRCH), yet a child that ignored it still
/// needs the group's SIGKILL. The group id cannot be reused while the group has members.
#[cfg(target_os = "macos")]
unsafe fn stop_server(server: libc::pid_t) -> ! {
    let group = libc::getpgid(server) == server;
    signal_server(server, group, libc::SIGTERM);
    libc::sleep(2);
    signal_server(server, group, libc::SIGKILL);
    libc::_exit(0)
}

#[cfg(target_os = "macos")]
unsafe fn watch(parent: libc::pid_t, server: libc::pid_t) -> ! {
    // Out of the server's process group (we were forked from it), so signalling the group
    // spares the watchdog.
    libc::setsid();
    // Keep none of the server's pipes open (the log readers wait for EOF).
    let max_fd = libc::getdtablesize().max(256);
    for fd in 0..max_fd {
        libc::close(fd);
    }
    let kq = libc::kqueue();
    if kq < 0 {
        libc::_exit(0);
    }
    if !register_exit(kq, server) {
        libc::_exit(0); // the server is already gone
    }
    // Registration fails (ESRCH) when Harness already exited; a zero-signal probe covers
    // an exit right after a successful registration check as well.
    if !register_exit(kq, parent) || libc::kill(parent, 0) != 0 {
        stop_server(server);
    }
    let mut event: libc::kevent = std::mem::zeroed();
    loop {
        let n = libc::kevent(kq, std::ptr::null(), 0, &mut event, 1, std::ptr::null());
        if n <= 0 {
            // EINTR or nothing: wait again.
            continue;
        }
        if event.ident == parent as libc::uintptr_t {
            stop_server(server);
        }
        libc::_exit(0);
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    fn alive(pid: i32) -> bool {
        // SAFETY: a zero signal only checks that the process exists.
        unsafe { libc::kill(pid, 0) == 0 }
    }

    /// When the watched parent dies, the watchdog ends the server and what the server started
    /// (its process group), and the watchdog is not in that group.
    #[test]
    fn the_watchdog_ends_the_servers_group_when_the_parent_dies() {
        // A stand-in for Harness Studio that we can end.
        let mut parent = Command::new("sleep").arg("60").spawn().unwrap();
        // A "server" that starts a child of its own and writes its pid, as a group leader. The
        // server ends on SIGTERM; its child ignores SIGTERM, so only the group's SIGKILL, sent
        // after the leader is gone, ends it.
        let dir = tempfile::tempdir().unwrap();
        let pidfile = dir.path().join("child.pid");
        let mut cmd = Command::new("sh");
        cmd.args([
            "-c",
            &format!(
                "(trap '' TERM; exec sleep 300) & echo $! > '{}'; wait",
                pidfile.display()
            ),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
        super::install_for(&mut cmd, parent.id() as libc::pid_t);
        let mut server = cmd.spawn().unwrap();
        let server_pid = server.id() as i32;

        let t = Instant::now();
        let child_pid = loop {
            if let Some(pid) = std::fs::read_to_string(&pidfile)
                .ok()
                .and_then(|s| s.trim().parse::<i32>().ok())
            {
                break pid;
            }
            assert!(t.elapsed() < Duration::from_secs(5), "no child pid");
            std::thread::sleep(Duration::from_millis(20));
        };
        // SAFETY: plain queries.
        let child_group = unsafe { libc::getpgid(child_pid) };
        assert_eq!(
            child_group, server_pid,
            "the child is in the server's group"
        );
        // The watchdog is a process of its own group (setsid), outside the server's: the group
        // holds the server and its child only.
        let members = Command::new("pgrep")
            .args(["-g", &server_pid.to_string()])
            .output()
            .unwrap();
        let mut members: Vec<i32> = String::from_utf8_lossy(&members.stdout)
            .split_whitespace()
            .filter_map(|p| p.parse().ok())
            .collect();
        members.sort();
        let mut expected = vec![server_pid, child_pid];
        expected.sort();
        assert_eq!(members, expected, "the watchdog is in the server's group");

        parent.kill().unwrap();
        parent.wait().unwrap();
        let t = Instant::now();
        while (alive(child_pid) || server.try_wait().unwrap().is_none())
            && t.elapsed() < Duration::from_secs(6)
        {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(server.try_wait().unwrap().is_some(), "the server survived");
        assert!(!alive(child_pid), "the server's child survived");
    }
}
