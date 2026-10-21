//! Orphan-process protection on macOS and Linux.
//!
//! Windows puts every engine in a Job Object with `KILL_ON_JOB_CLOSE`, so it dies with Harness
//! Studio even after a crash. Unix has nothing that covers a whole process group: macOS has no
//! parent-death signal at all, and Linux's `PR_SET_PDEATHSIG` reaches only the direct child (a
//! Python worker's subprocesses would survive). [`install`] adds a `pre_exec` hook that, in the
//! forked child right before `exec`, double-forks a tiny watchdog (reparented to init/launchd,
//! so it outlives Harness). The watchdog closes every inherited descriptor and waits for the
//! exit of two processes (a kqueue `NOTE_EXIT` on macOS, pidfds on Linux):
//! - Harness Studio exits (normally, by crash or by SIGKILL) -> `SIGTERM` to the child's process
//!   group, `SIGKILL` 2 s later (servers are started as group leaders, so whatever they started
//!   goes with them);
//! - the child exits -> the watchdog exits too.
//!
//! The watchdog is forked from the child before `exec`, so it starts in the child's group; it
//! moves to a session of its own first, so the group signal does not end the watchdog before its
//! `SIGKILL` and a clean stop of the group (SIGTERM to it) cannot end the watchdog either.
//!
//! Only async-signal-safe system calls run between `fork` and `exec` (fork, waitpid, _exit,
//! close, getpgid, setsid, kqueue, kevent, pidfd_open, poll, kill, sleep). A Linux kernel older
//! than 5.3 has no pidfd: the watchdog then exits and `PR_SET_PDEATHSIG` (set by
//! [`crate::orphans::protect`] on such a kernel) is what is left. See private hs-internal
//! architecture notes.

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub fn install(cmd: &mut std::process::Command) {
    install_for(cmd, std::process::id() as libc::pid_t);
}

/// [`install`] watching `parent` instead of this process (tests stand in a parent they can end).
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn install_for(cmd: &mut std::process::Command, parent: libc::pid_t) {
    use std::os::unix::process::CommandExt;
    // SAFETY: the hook only makes async-signal-safe system calls (see module docs).
    unsafe {
        cmd.pre_exec(move || {
            // Still the forked child: this pid becomes the engine after exec.
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

/// Which of the two watched processes exited first.
#[cfg(any(target_os = "macos", target_os = "linux"))]
enum Exit {
    Parent,
    Server,
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

/// Waits for the first exit; `None` when it cannot be watched (the watchdog then gives up).
#[cfg(target_os = "macos")]
unsafe fn first_exit(parent: libc::pid_t, server: libc::pid_t) -> Option<Exit> {
    let kq = libc::kqueue();
    if kq < 0 {
        return None;
    }
    if !register_exit(kq, server) {
        return Some(Exit::Server); // the server is already gone
    }
    // Registration fails (ESRCH) when Harness already exited; a zero-signal probe covers
    // an exit right after a successful registration check as well.
    if !register_exit(kq, parent) || libc::kill(parent, 0) != 0 {
        return Some(Exit::Parent);
    }
    let mut event: libc::kevent = std::mem::zeroed();
    loop {
        let n = libc::kevent(kq, std::ptr::null(), 0, &mut event, 1, std::ptr::null());
        if n <= 0 {
            // EINTR or nothing: wait again.
            continue;
        }
        return Some(if event.ident == parent as libc::uintptr_t {
            Exit::Parent
        } else {
            Exit::Server
        });
    }
}

#[cfg(target_os = "linux")]
unsafe fn errno() -> libc::c_int {
    *libc::__errno_location()
}

/// Waits for the first exit; `None` when it cannot be watched (the watchdog then gives up).
#[cfg(target_os = "linux")]
unsafe fn first_exit(parent: libc::pid_t, server: libc::pid_t) -> Option<Exit> {
    let pidfd = |pid: libc::pid_t| libc::syscall(libc::SYS_pidfd_open, pid, 0) as libc::c_int;
    let server_fd = pidfd(server);
    if server_fd < 0 {
        return (errno() == libc::ESRCH).then_some(Exit::Server);
    }
    let parent_fd = pidfd(parent);
    if parent_fd < 0 {
        return (errno() == libc::ESRCH).then_some(Exit::Parent);
    }
    let mut fds = [
        libc::pollfd {
            fd: parent_fd,
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: server_fd,
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    loop {
        if libc::poll(fds.as_mut_ptr(), 2, -1) <= 0 {
            if errno() == libc::EINTR {
                continue;
            }
            return None;
        }
        // Both may be ready at once: the parent's exit decides, so the group is stopped.
        return Some(if fds[0].revents != 0 {
            Exit::Parent
        } else {
            Exit::Server
        });
    }
}

/// SIGTERM, then SIGKILL after 2 s, to the server's group when it leads one and to the server
/// itself in any case. Whether it leads its group (servers do: `command_group` starts them
/// with `setpgid(0, 0)`) was decided when the watchdog started: the leader may be gone by now,
/// and one that exits on SIGTERM can no longer be asked (ESRCH), yet a child that ignored it
/// still needs the group's SIGKILL. The group id cannot be reused while the group has members.
#[cfg(any(target_os = "macos", target_os = "linux"))]
unsafe fn stop_server(server: libc::pid_t, group: bool) -> ! {
    for (n, sig) in [libc::SIGTERM, libc::SIGKILL].into_iter().enumerate() {
        if n > 0 {
            libc::sleep(2);
        }
        if group {
            libc::kill(-server, sig);
        }
        libc::kill(server, sig);
    }
    libc::_exit(0)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
unsafe fn watch(parent: libc::pid_t, server: libc::pid_t) -> ! {
    let group = libc::getpgid(server) == server;
    // Out of the server's process group (we were forked from it), so signalling the group
    // spares the watchdog.
    libc::setsid();
    // Keep none of the server's pipes open (the log readers wait for EOF).
    let max_fd = libc::getdtablesize().max(256);
    for fd in 0..max_fd {
        libc::close(fd);
    }
    match first_exit(parent, server) {
        Some(Exit::Parent) => stop_server(server, group),
        Some(Exit::Server) | None => libc::_exit(0),
    }
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
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
