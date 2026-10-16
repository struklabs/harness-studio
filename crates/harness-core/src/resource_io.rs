//! Fallible thread creation under temporary OS resource pressure.

use std::io;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// Resource exhaustion can clear after another request releases a handle or memory.
pub fn is_transient_resource_error(error: &io::Error) -> bool {
    if matches!(
        error.kind(),
        io::ErrorKind::OutOfMemory | io::ErrorKind::WouldBlock
    ) {
        return true;
    }
    // Windows system/working-set quotas and Winsock buffer exhaustion. Check raw
    // codes: ErrorKind does not consistently classify these as temporary.
    if matches!(error.raw_os_error(), Some(1450 | 1453 | 10055)) {
        return true;
    }
    #[cfg(unix)]
    if error.raw_os_error().is_some_and(|code| {
        [libc::ENOMEM, libc::EMFILE, libc::ENFILE, libc::ENOBUFS].contains(&code)
    }) {
        return true;
    }
    false
}

fn retry_with<T>(
    mut op: impl FnMut() -> io::Result<T>,
    mut sleep: impl FnMut(Duration),
) -> io::Result<T> {
    // Four attempts, at most 175 ms of backoff. Never spin or hide permanent errors.
    for delay in [25, 50, 100] {
        match op() {
            Err(e) if is_transient_resource_error(&e) => sleep(Duration::from_millis(delay)),
            result => return result,
        }
    }
    op()
}

/// Unlike `thread::spawn`, returns an I/O error when the OS refuses a thread.
/// The task (including any pipe handles) survives failed attempts and is dropped
/// if the retry budget is exhausted. Only a successful spawn can run it.
pub(crate) fn spawn_thread<F, T>(name: &str, task: F) -> io::Result<JoinHandle<T>>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    spawn_with(
        task,
        |task| std::thread::Builder::new().name(name.into()).spawn(task),
        std::thread::sleep,
    )
}

fn spawn_with<F, T>(
    task: F,
    mut spawn: impl FnMut(Box<dyn FnOnce() -> T + Send>) -> io::Result<JoinHandle<T>>,
    sleep: impl FnMut(Duration),
) -> io::Result<JoinHandle<T>>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let task = Arc::new(Mutex::new(Some(task)));
    retry_with(
        || {
            let task = task.clone();
            spawn(Box::new(move || {
                let task = task.lock().unwrap_or_else(|e| e.into_inner()).take();
                task.expect("only a successfully spawned thread takes the task")()
            }))
        },
        sleep,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_resource_errors() {
        for code in [1450, 1453, 10055] {
            assert!(is_transient_resource_error(&io::Error::from_raw_os_error(
                code
            )));
        }
        #[cfg(unix)]
        for code in [libc::ENOMEM, libc::EMFILE, libc::ENFILE, libc::ENOBUFS] {
            assert!(is_transient_resource_error(&io::Error::from_raw_os_error(
                code
            )));
        }
        for kind in [io::ErrorKind::OutOfMemory, io::ErrorKind::WouldBlock] {
            assert!(is_transient_resource_error(&io::Error::from(kind)));
        }
        for kind in [io::ErrorKind::PermissionDenied, io::ErrorKind::NotFound] {
            assert!(!is_transient_resource_error(&io::Error::from(kind)));
        }
        assert!(!is_transient_resource_error(&io::Error::from_raw_os_error(
            5
        )));
    }

    #[test]
    fn retries_then_succeeds_without_real_resource_exhaustion() {
        let mut attempts = 0;
        let mut delays = Vec::new();
        let result = retry_with(
            || {
                attempts += 1;
                if attempts < 3 {
                    Err(io::Error::from_raw_os_error(1450))
                } else {
                    Ok(42)
                }
            },
            |delay| delays.push(delay.as_millis()),
        );
        assert_eq!(result.unwrap(), 42);
        assert_eq!(attempts, 3);
        assert_eq!(delays, [25, 50]);
    }

    #[test]
    fn exhaustion_is_bounded_and_preserves_os_error() {
        let mut attempts = 0;
        let mut delays = Vec::new();
        let result: io::Result<()> = retry_with(
            || {
                attempts += 1;
                Err(io::Error::from_raw_os_error(1453))
            },
            |delay| delays.push(delay.as_millis()),
        );
        assert_eq!(result.unwrap_err().raw_os_error(), Some(1453));
        assert_eq!(attempts, 4);
        assert_eq!(delays, [25, 50, 100]);
    }

    #[test]
    fn permanent_errors_are_not_retried() {
        let mut attempts = 0;
        let result: io::Result<()> = retry_with(
            || {
                attempts += 1;
                Err(io::Error::from_raw_os_error(5))
            },
            |_| panic!("must not sleep"),
        );
        assert_eq!(result.unwrap_err().raw_os_error(), Some(5));
        assert_eq!(attempts, 1);
    }

    #[test]
    fn thread_runs_task_once() {
        let thread = spawn_thread("resource-io-test", || 42).unwrap();
        assert_eq!(thread.join().unwrap(), 42);
    }

    #[test]
    fn failed_spawns_preserve_the_task_until_success() {
        let captured = Arc::new(42);
        let weak = Arc::downgrade(&captured);
        let mut attempts = 0;
        let thread = spawn_with(
            move || *captured,
            |task| {
                attempts += 1;
                if attempts < 3 {
                    // Dropping this attempt must not drop the underlying task.
                    Err(io::Error::from_raw_os_error(1450))
                } else {
                    assert!(weak.upgrade().is_some());
                    std::thread::Builder::new().spawn(task)
                }
            },
            |_| {},
        )
        .unwrap();
        assert_eq!(attempts, 3);
        assert_eq!(thread.join().unwrap(), 42);
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn exhausted_spawns_release_captured_resources_without_running_task() {
        let captured = Arc::new(());
        let weak = Arc::downgrade(&captured);
        let result = spawn_with(
            move || {
                drop(captured);
                panic!("failed spawns must not run task")
            },
            |_| Err(io::Error::from_raw_os_error(1450)),
            |_| {},
        );
        assert_eq!(result.unwrap_err().raw_os_error(), Some(1450));
        assert!(
            weak.upgrade().is_none(),
            "failed task must release its pipes"
        );
    }
}
