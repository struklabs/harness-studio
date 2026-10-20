//! Resource ceilings supplement process groups; they do not restrict OS authority.
use std::io;

fn memory_bytes(value: Option<String>) -> io::Result<Option<usize>> {
    value
        .map(|s| {
            s.parse::<usize>()
                .ok()
                .filter(|n| *n > 0)
                .and_then(|n| n.checked_mul(1024 * 1024))
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "HARNESS_CHILD_MEMORY_MIB must be a positive MiB count",
                    )
                })
        })
        .transpose()
}

/// Set before constructing a process group. Never silently ignore invalid limits.
pub fn prepare(command: &mut std::process::Command) -> io::Result<()> {
    let memory = memory_bytes(std::env::var("HARNESS_CHILD_MEMORY_MIB").ok())?;
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Only async-signal-safe libc calls in the post-fork hook. Preserve lower inherited limits.
        unsafe {
            command.pre_exec(move || {
                for (resource, ceiling) in [(libc::RLIMIT_CORE, 0), (libc::RLIMIT_NOFILE, 1024)] {
                    let mut old = std::mem::zeroed::<libc::rlimit>();
                    if libc::getrlimit(resource, &mut old) != 0 {
                        return Err(io::Error::last_os_error());
                    }
                    let limit = libc::rlimit {
                        rlim_cur: old.rlim_cur.min(ceiling),
                        rlim_max: old.rlim_max.min(ceiling),
                    };
                    if libc::setrlimit(resource, &limit) != 0 {
                        return Err(io::Error::last_os_error());
                    }
                }
                if let Some(bytes) = memory {
                    let mut old = std::mem::zeroed::<libc::rlimit>();
                    if libc::getrlimit(libc::RLIMIT_AS, &mut old) != 0 {
                        return Err(io::Error::last_os_error());
                    }
                    let limit = libc::rlimit {
                        rlim_cur: old.rlim_cur.min(bytes as _),
                        rlim_max: old.rlim_max.min(bytes as _),
                    };
                    if libc::setrlimit(libc::RLIMIT_AS, &limit) != 0 {
                        return Err(io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
    }
    #[cfg(not(unix))]
    let _ = (command, memory);
    Ok(())
}

/// Optional supplemental Windows job. The primary command-group job already prevents
/// breakaway and kills descendants on handle closure. This job adds a commit ceiling.
#[cfg(windows)]
pub struct MemoryJob(windows::Win32::Foundation::HANDLE);
#[cfg(windows)]
unsafe impl Send for MemoryJob {}
#[cfg(windows)]
unsafe impl Sync for MemoryJob {}
#[cfg(windows)]
impl Drop for MemoryJob {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
#[cfg(windows)]
pub fn memory_job(handle: std::os::windows::io::RawHandle) -> io::Result<Option<MemoryJob>> {
    let Some(bytes) = memory_bytes(std::env::var("HARNESS_CHILD_MEMORY_MIB").ok())? else {
        return Ok(None);
    };
    memory_job_with_limit(handle, bytes).map(Some)
}

#[cfg(windows)]
fn memory_job_with_limit(
    handle: std::os::windows::io::RawHandle,
    bytes: usize,
) -> io::Result<MemoryJob> {
    use windows::Win32::{Foundation::HANDLE, System::JobObjects::*};
    unsafe {
        let job = MemoryJob(CreateJobObjectW(None, None)?);
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags =
            JOB_OBJECT_LIMIT_JOB_MEMORY | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        limits.JobMemoryLimit = bytes;
        SetInformationJobObject(
            job.0,
            JobObjectExtendedLimitInformation,
            &limits as *const _ as _,
            std::mem::size_of_val(&limits) as u32,
        )?;
        AssignProcessToJobObject(job.0, HANDLE(handle))?;
        Ok(job)
    }
}

/// Unix group ownership must survive dropped futures too; command-group's Unix
/// kill_on_drop does not implement group cleanup. Windows owns a kernel Job instead.
pub struct GroupGuard(pub u32);
impl Drop for GroupGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        unsafe {
            libc::kill(-(self.0 as i32), libc::SIGKILL);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn memory_option_is_checked() {
        assert_eq!(memory_bytes(None).unwrap(), None);
        assert_eq!(
            memory_bytes(Some("64".into())).unwrap(),
            Some(64 * 1024 * 1024)
        );
        for value in ["0", "-1", "bad", "18446744073709551615"] {
            assert!(memory_bytes(Some(value.into())).is_err());
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_job_has_memory_ceiling_without_breakaway() {
        use command_group::CommandGroup;
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::System::JobObjects::*;
        let mut command = std::process::Command::new("cmd.exe");
        command
            .args(["/d", "/c", "ping -n 30 127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let mut child = command
            .group()
            .kill_on_drop(true)
            .creation_flags(0x0800_0000)
            .spawn()
            .unwrap();
        let bytes = 256 * 1024 * 1024;
        let job = memory_job_with_limit(child.inner().as_raw_handle(), bytes).unwrap();
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        unsafe {
            QueryInformationJobObject(
                Some(job.0),
                JobObjectExtendedLimitInformation,
                &mut limits as *mut _ as _,
                std::mem::size_of_val(&limits) as u32,
                None,
            )
            .unwrap();
        }
        assert_eq!(limits.JobMemoryLimit, bytes);
        assert_eq!(
            limits.BasicLimitInformation.LimitFlags,
            JOB_OBJECT_LIMIT_JOB_MEMORY | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        );
        drop(job);
        // Windows does not promise a nonzero exit code for kill-on-job-close.
        let stopped = unsafe {
            windows::Win32::System::Threading::WaitForSingleObject(
                windows::Win32::Foundation::HANDLE(child.inner().as_raw_handle()),
                2000,
            )
        };
        assert_eq!(stopped, windows::Win32::Foundation::WAIT_OBJECT_0);
        child.wait().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn unix_child_receives_resource_ceilings() {
        let mut command = std::process::Command::new("sh");
        command.args(["-c", "ulimit -c; ulimit -n"]);
        prepare(&mut command).unwrap();
        let output = command.output().unwrap();
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        let values: Vec<_> = text.lines().collect();
        assert_eq!(values[0], "0");
        assert!(values[1].parse::<u64>().unwrap() <= 1024);
    }
}
