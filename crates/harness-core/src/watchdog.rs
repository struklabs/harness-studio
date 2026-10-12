//! Orphan-process protection on macOS.
//!
//! Windows puts llama-server in a Job Object with `KILL_ON_JOB_CLOSE` and Linux sets
//! `PR_SET_PDEATHSIG`, so the server dies with Harness Studio even after a crash. macOS has
//! neither. [`install`] adds a `pre_exec` hook that, in the forked child right before `exec`,
//! double-forks a tiny watchdog (reparented to launchd, so it outlives Harness). The watchdog
//! closes every inherited descriptor and waits on a kqueue for `NOTE_EXIT` of two processes:
//! - Harness Studio exits (normally or by crash) -> `SIGTERM` to the server, `SIGKILL` 2 s later;
//! - the server exits -> the watchdog exits too.
//!
//! Only async-signal-safe system calls run between `fork` and `exec` (fork, waitpid, _exit,
//! close, kqueue, kevent, kill, sleep). Not compiled or run on the development machine
//! (Windows); see ARCHITECTURE §13.7.

#[cfg(target_os = "macos")]
pub fn install(cmd: &mut std::process::Command) {
    use std::os::unix::process::CommandExt;
    let parent = std::process::id() as libc::pid_t;
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

#[cfg(target_os = "macos")]
unsafe fn stop_server(server: libc::pid_t) -> ! {
    libc::kill(server, libc::SIGTERM);
    libc::sleep(2);
    libc::kill(server, libc::SIGKILL);
    libc::_exit(0)
}

#[cfg(target_os = "macos")]
unsafe fn watch(parent: libc::pid_t, server: libc::pid_t) -> ! {
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
