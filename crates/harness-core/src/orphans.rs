//! Engines must not outlive Harness Studio, even when it is killed (SIGKILL, a crash, a force
//! quit). Two layers on Unix (Windows: every engine is in a Job Object that closes with Harness):
//!
//! - [`protect`], called from `process_limits::prepare` and for the engines started without
//!   it: the child dies with Harness. The [`crate::watchdog`] stops the child's whole process
//!   group once Harness exits. A Linux kernel without pidfds (before 5.3) gets
//!   `PR_SET_PDEATHSIG` instead, which reaches the child only, and fires when the *thread* that
//!   spawned it ends (a `spawn_blocking` thread going idle): the watchdog has neither limit.
//! - [`Registration`]: while an engine runs, `<data>/run/engines/<pid>.json` names it (pid and
//!   start time). The next owner of the data directory ([`sweep`], from
//!   `InstanceLock::acquire`) stops what a killed owner left behind: the last resort when the
//!   first layer could not act (its watchdog was killed too, say by a `kill -9` of the whole
//!   session). The start time tells a left engine from an unrelated process that got its pid.

#[cfg(unix)]
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The child dies with Harness Studio (see the module docs). Call it before the command's own
/// process group is set up (`command_group`), like `process_limits::prepare`.
pub fn protect(cmd: &mut std::process::Command) {
    #[cfg(target_os = "linux")]
    if !pidfds() {
        use std::os::unix::process::CommandExt;
        // SAFETY: only async-signal-safe libc calls are made between fork and exec.
        unsafe {
            cmd.pre_exec(|| {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                // The parent may have died between fork and PR_SET_PDEATHSIG.
                if libc::getppid() == 1 {
                    libc::raise(libc::SIGKILL);
                }
                Ok(())
            });
        }
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    crate::watchdog::install(cmd);
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let _ = cmd;
}

/// Whether this kernel has `pidfd_open` (Linux 5.3), which the watchdog needs.
#[cfg(target_os = "linux")]
fn pidfds() -> bool {
    static HAS: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *HAS.get_or_init(|| {
        // SAFETY: opens and closes a descriptor for this process.
        unsafe {
            let fd = libc::syscall(libc::SYS_pidfd_open, libc::getpid(), 0) as libc::c_int;
            fd >= 0 && libc::close(fd) == 0
        }
    })
}

fn engines_dir(root: &Path) -> PathBuf {
    root.join("run").join("engines")
}

#[cfg(unix)]
#[derive(Debug, Serialize, Deserialize)]
struct Entry {
    pid: u32,
    /// Seconds since the epoch, as `sysinfo` reports it.
    start: u64,
    #[serde(default)]
    program: String,
}

#[cfg(unix)]
fn start_time(sys: &mut sysinfo::System, pid: u32) -> Option<u64> {
    let id = sysinfo::Pid::from_u32(pid);
    sys.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::Some(&[id]),
        true,
        sysinfo::ProcessRefreshKind::nothing(),
    );
    sys.process(id).map(|p| p.start_time())
}

/// A running engine named in the data directory; dropping it (the engine stopped) removes the
/// entry. Best effort: an entry that cannot be written leaves the first layer alone.
#[derive(Debug)]
pub struct Registration(Option<PathBuf>);

impl Registration {
    pub fn new(pid: u32) -> Self {
        Self::in_dir(&crate::paths::data_root(), pid)
    }

    fn in_dir(root: &Path, pid: u32) -> Self {
        #[cfg(unix)]
        {
            let mut sys = sysinfo::System::new();
            let Some(start) = start_time(&mut sys, pid) else {
                return Self(None);
            };
            let program = sys
                .process(sysinfo::Pid::from_u32(pid))
                .map(|p| p.name().to_string_lossy().into_owned())
                .unwrap_or_default();
            let dir = engines_dir(root);
            let path = dir.join(format!("{pid}.json"));
            let entry = Entry {
                pid,
                start,
                program,
            };
            let written = std::fs::create_dir_all(&dir).is_ok()
                && serde_json::to_vec(&entry)
                    .ok()
                    .is_some_and(|data| std::fs::write(&path, data).is_ok());
            Self(written.then_some(path))
        }
        #[cfg(not(unix))]
        {
            let _ = (root, pid);
            Self(None)
        }
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Stops the engines a previous owner of `root` left running (see the module docs) and clears
/// their entries. Returns how many were still running. Blocks up to 2 s when one ignores
/// SIGTERM.
pub fn sweep(root: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(engines_dir(root)) else {
        return 0;
    };
    #[cfg(unix)]
    {
        let own = std::process::id();
        let mut sys = sysinfo::System::new();
        let mut left = Vec::new();
        for e in entries.flatten() {
            let path = e.path();
            let entry = std::fs::read(&path)
                .ok()
                .and_then(|data| serde_json::from_slice::<Entry>(&data).ok());
            let _ = std::fs::remove_file(&path);
            let Some(entry) = entry.filter(|entry| entry.pid != own) else {
                continue;
            };
            if start_time(&mut sys, entry.pid) == Some(entry.start) {
                left.push(entry.pid as libc::pid_t);
            }
        }
        // A server leads its process group (whatever it started is in it); signal the group
        // only while the verified leader still holds it.
        let groups: Vec<bool> = left
            .iter()
            // SAFETY: plain queries and signals to processes verified above.
            .map(|pid| unsafe { libc::getpgid(*pid) == *pid })
            .collect();
        let signal = |sig| {
            for (pid, group) in left.iter().zip(&groups) {
                // SAFETY: see above.
                unsafe {
                    if *group {
                        libc::kill(-pid, sig);
                    }
                    libc::kill(*pid, sig);
                }
            }
        };
        signal(libc::SIGTERM);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        // SAFETY: a zero signal only checks that the process exists.
        let alive = |pid: &libc::pid_t| unsafe { libc::kill(*pid, 0) == 0 };
        while left.iter().any(alive) && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        signal(libc::SIGKILL);
        left.len()
    }
    #[cfg(not(unix))]
    {
        for e in entries.flatten() {
            let _ = std::fs::remove_file(e.path());
        }
        0
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    fn alive(pid: i32) -> bool {
        // SAFETY: a zero signal only checks that the process exists.
        unsafe { libc::kill(pid, 0) == 0 }
    }

    fn wait_gone(pid: i32) -> bool {
        let t = Instant::now();
        while alive(pid) && t.elapsed() < Duration::from_secs(6) {
            std::thread::sleep(Duration::from_millis(50));
        }
        !alive(pid)
    }

    /// An engine registered by an owner that is gone is stopped by the next owner's sweep, with
    /// the rest of its group (a child that ignores SIGTERM included); its entry is removed.
    #[test]
    fn sweep_stops_a_left_engine_and_its_group() {
        let root = tempfile::tempdir().unwrap();
        let pidfile = root.path().join("child.pid");
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
        let registration = Registration::in_dir(root.path(), server.id());
        let entry = registration.0.clone().expect("the engine is registered");
        assert!(entry.exists());
        // The owner was killed: its registration was never dropped.
        std::mem::forget(registration);

        assert_eq!(sweep(root.path()), 1);
        server.wait().unwrap();
        assert!(wait_gone(child_pid), "the engine's child survived");
        assert!(!entry.exists(), "the entry was kept");
        assert!(!alive(server_pid) || server.try_wait().unwrap().is_some());
    }

    /// An entry whose pid now belongs to another process (a different start time) is dropped
    /// without a signal; a registration dropped by a clean stop leaves no entry.
    #[test]
    fn sweep_spares_a_reused_pid_and_clean_stops_leave_nothing() {
        let root = tempfile::tempdir().unwrap();
        let mut bystander = Command::new("sleep").arg("60").spawn().unwrap();
        let dir = engines_dir(root.path());
        std::fs::create_dir_all(&dir).unwrap();
        let stale = Entry {
            pid: bystander.id(),
            start: 1,
            program: "llama-server".into(),
        };
        std::fs::write(
            dir.join(format!("{}.json", bystander.id())),
            serde_json::to_vec(&stale).unwrap(),
        )
        .unwrap();
        std::fs::write(dir.join("garbage.json"), b"not json").unwrap();
        assert_eq!(sweep(root.path()), 0);
        assert!(
            bystander.try_wait().unwrap().is_none(),
            "a bystander was signalled"
        );
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);

        let registration = Registration::in_dir(root.path(), bystander.id());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        drop(registration);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        bystander.kill().unwrap();
        bystander.wait().unwrap();
    }

    /// [`protect`]: a protected child is stopped when Harness is killed. Harness is played by
    /// this test binary, re-run as a fixture that starts the engine and is then SIGKILLed.
    #[test]
    fn a_protected_engine_dies_with_a_killed_parent() {
        let dir = tempfile::tempdir().unwrap();
        let pidfile = dir.path().join("engine.pid");
        // The test binary re-executed as the "parent": it spawns a protected engine and waits.
        let mut parent = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "orphans::tests::protected_engine_fixture",
                "--nocapture",
            ])
            .env("HARNESS_ORPHAN_FIXTURE", &pidfile)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let t = Instant::now();
        let engine = loop {
            if let Some(pid) = std::fs::read_to_string(&pidfile)
                .ok()
                .and_then(|s| s.trim().parse::<i32>().ok())
            {
                break pid;
            }
            assert!(t.elapsed() < Duration::from_secs(20), "no engine pid");
            std::thread::sleep(Duration::from_millis(20));
        };
        assert!(alive(engine));
        // SAFETY: SIGKILL to the fixture we started.
        unsafe { libc::kill(parent.id() as i32, libc::SIGKILL) };
        parent.wait().unwrap();
        assert!(wait_gone(engine), "the engine outlived its killed parent");
    }

    /// Run by [`a_protected_engine_dies_with_a_killed_parent`] only.
    #[test]
    fn protected_engine_fixture() {
        let Some(pidfile) = std::env::var_os("HARNESS_ORPHAN_FIXTURE") else {
            return;
        };
        let mut cmd = Command::new("sleep");
        cmd.arg("300")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        protect(&mut cmd);
        let mut engine = cmd.process_group(0).spawn().unwrap();
        std::fs::write(&pidfile, engine.id().to_string()).unwrap();
        engine.wait().unwrap();
    }
}
