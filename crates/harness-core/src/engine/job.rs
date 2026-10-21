//! Job-kind runtimes: one process per request, progress from its output, cancellation,
//! files as the result. llama-bench runs on it (LlamaBenchEngine, the benchmark manager);
//! stable-diffusion.cpp is next. Cancelling kills the whole process tree
//! (a Windows Job Object, a Unix process group), as the shell tool does.

use std::path::PathBuf;
use std::process::Stdio;

use command_group::AsyncCommandGroup;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, BufReader, Lines};
use tokio::sync::watch;

use super::Engine;

/// One run of a job binary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobCommand {
    pub binary: PathBuf,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    /// Files the run must leave behind; a run that exits 0 without them failed.
    pub outputs: Vec<PathBuf>,
    /// GPU footprint (weights + context + overhead), estimated before enqueueing.
    /// CPU-only jobs explicitly reserve zero; unknown GPU footprints reserve u64::MAX.
    pub memory_bytes: u64,
    pub priority: crate::jobs::Priority,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobProgress {
    pub done: u64,
    pub total: Option<u64>,
    /// What the step is, when the output says (prompt run 1/5).
    #[serde(default)]
    pub phase: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum JobEvent {
    Progress(JobProgress),
    /// Any output line (stdout or stderr), for the log.
    Line(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum JobOutcome {
    Completed {
        files: Vec<PathBuf>,
    },
    Cancelled,
    Failed {
        /// i18n key (`jobs.*`).
        reason: String,
        exit_code: Option<i32>,
        /// Last output lines, newest last.
        tail: Vec<String>,
    },
}

/// A family that runs per request.
pub trait JobEngine: Engine {
    /// The job binary of a runtime build, found from the build's server binary (llama-bench
    /// ships next to llama-server). Errors when the build does not have it.
    fn job_binary(&self, server_binary: &std::path::Path) -> Result<PathBuf, String>;
    /// Progress from one output line, if it reports any.
    fn parse_progress(&self, line: &str) -> Option<JobProgress>;
}

const TAIL_LINES: usize = 20;
const CANCEL_DRAIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

fn record_line(
    line: String,
    parse: &impl Fn(&str) -> Option<JobProgress>,
    on_event: &mut impl FnMut(JobEvent),
    tail: &mut std::collections::VecDeque<String>,
) {
    if let Some(p) = parse(&line) {
        on_event(JobEvent::Progress(p));
    }
    if tail.len() == TAIL_LINES {
        tail.pop_front();
    }
    tail.push_back(line.clone());
    on_event(JobEvent::Line(line));
}

/// Preserve buffered output after killing, even if an escaped descendant keeps
/// a pipe open. The deadline covers both streams together and never resets.
async fn drain_cancelled_output(
    out: &mut Lines<impl AsyncBufRead + Unpin>,
    err: &mut Lines<impl AsyncBufRead + Unpin>,
    parse: &mut impl Fn(&str) -> Option<JobProgress>,
    on_event: &mut impl FnMut(JobEvent),
    tail: &mut std::collections::VecDeque<String>,
) -> JobOutcome {
    let deadline = tokio::time::sleep(CANCEL_DRAIN_TIMEOUT);
    tokio::pin!(deadline);
    let (mut out_open, mut err_open) = (true, true);
    while out_open || err_open {
        let line = tokio::select! {
            // Enforce the bound even when output is continuously ready.
            biased;
            _ = &mut deadline => break,
            l = out.next_line(), if out_open => match l {
                Ok(Some(l)) => l,
                _ => { out_open = false; continue; }
            },
            l = err.next_line(), if err_open => match l {
                Ok(Some(l)) => l,
                _ => { err_open = false; continue; }
            },
        };
        record_line(line, parse, on_event, tail);
    }
    JobOutcome::Cancelled
}

/// Runs `cmd` to completion, cancellation or failure. `parse` reads progress from each
/// output line (a [`JobEngine::parse_progress`]); `cancel` turning true kills the process.
pub async fn run_job(
    cmd: &JobCommand,
    parse: impl Fn(&str) -> Option<JobProgress>,
    cancel: watch::Receiver<bool>,
    on_event: impl FnMut(JobEvent),
) -> JobOutcome {
    run_job_on(crate::jobs::global().clone(), cmd, parse, cancel, on_event).await
}

async fn run_job_on(
    scheduler: std::sync::Arc<crate::jobs::JobScheduler>,
    cmd: &JobCommand,
    parse: impl Fn(&str) -> Option<JobProgress>,
    mut cancel: watch::Receiver<bool>,
    mut on_event: impl FnMut(JobEvent),
) -> JobOutcome {
    let ticket = scheduler.submit(
        cmd.binary
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        cmd.priority,
        cmd.memory_bytes,
    );
    let mut central_cancel = ticket.cancellation();
    tokio::select! {
        result = ticket.wait() => if result.is_err() { return JobOutcome::Cancelled; },
        _ = cancel.wait_for(|cancelled| *cancelled) => return JobOutcome::Cancelled,
    }
    if *cancel.borrow() || ticket.is_cancelled() {
        return JobOutcome::Cancelled;
    }
    let (combined, receiver) = watch::channel(false);
    let work = run_job_inner(cmd, parse, receiver, |event| {
        ticket.loaded();
        on_event(event);
    });
    tokio::pin!(work);
    tokio::select! {
        result = &mut work => result,
        _ = cancel.wait_for(|cancelled| *cancelled) => {
            combined.send_replace(true);
            work.await
        },
        _ = central_cancel.wait_for(|cancelled| *cancelled) => {
            combined.send_replace(true);
            work.await
        },
    }
}

async fn run_job_inner(
    cmd: &JobCommand,
    mut parse: impl Fn(&str) -> Option<JobProgress>,
    mut cancel: watch::Receiver<bool>,
    mut on_event: impl FnMut(JobEvent),
) -> JobOutcome {
    let failed =
        |reason: &str, exit_code, tail: &std::collections::VecDeque<String>| JobOutcome::Failed {
            reason: reason.to_string(),
            exit_code,
            tail: tail.iter().cloned().collect(),
        };
    let mut tail = std::collections::VecDeque::new();
    let mut command = tokio::process::Command::new(&cmd.binary);
    command
        .args(&cmd.args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = &cmd.cwd {
        command.current_dir(dir);
    }
    if crate::process_limits::prepare(command.as_std_mut()).is_err() {
        return failed("jobs.spawn_failed", None, &tail);
    }
    let mut group = command.group();
    group.kill_on_drop(true);
    #[cfg(windows)]
    group.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let mut child = match group.spawn() {
        Ok(c) => c,
        Err(_) => return failed("jobs.spawn_failed", None, &tail),
    };
    #[cfg(windows)]
    let _memory_job =
        match crate::process_limits::memory_job(child.inner().raw_handle().unwrap() as _) {
            Ok(job) => job,
            Err(_) => return failed("jobs.spawn_failed", None, &tail),
        };
    // Registered for the next start's sweep in case Harness is killed (`orphans`).
    let _registration = crate::orphans::Registration::new(child.id().unwrap());
    let _group_guard = crate::process_limits::GroupGuard(child.id().unwrap());
    let mut out = BufReader::new(child.inner().stdout.take().unwrap()).lines();
    let mut err = BufReader::new(child.inner().stderr.take().unwrap()).lines();
    let (mut out_open, mut err_open) = (true, true);
    if *cancel.borrow() {
        if child.kill().await.is_err() {
            return JobOutcome::Cancelled;
        }
        return drain_cancelled_output(&mut out, &mut err, &mut parse, &mut on_event, &mut tail)
            .await;
    }
    while out_open || err_open {
        let line = tokio::select! {
            changed = cancel.changed() => {
                if changed.is_err() || *cancel.borrow() {
                    if child.kill().await.is_err() {
                        return JobOutcome::Cancelled;
                    }
                    return drain_cancelled_output(
                        &mut out, &mut err, &mut parse, &mut on_event, &mut tail,
                    ).await;
                }
                continue;
            }
            l = out.next_line(), if out_open => match l {
                Ok(Some(l)) => l,
                _ => { out_open = false; continue; }
            },
            l = err.next_line(), if err_open => match l {
                Ok(Some(l)) => l,
                _ => { err_open = false; continue; }
            },
        };
        record_line(line, &parse, &mut on_event, &mut tail);
    }
    let status = tokio::select! {
        s = child.wait() => s,
        _ = cancel.wait_for(|c| *c) => {
            let _ = child.kill().await;
            return JobOutcome::Cancelled;
        }
    };
    match status {
        Ok(s) if s.success() => {
            if cmd.outputs.iter().all(|p| p.is_file()) {
                JobOutcome::Completed {
                    files: cmd.outputs.clone(),
                }
            } else {
                failed("jobs.missing_output", Some(0), &tail)
            }
        }
        Ok(s) => failed("jobs.exit_failed", s.code(), &tail),
        Err(_) => failed("jobs.wait_failed", None, &tail),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// Open writers simulate a descendant that survived the kill and inherited
    /// both pipes. Cancellation must keep available lines without waiting for EOF.
    #[tokio::test]
    async fn cancel_bounds_drain_when_pipes_stay_open() {
        use tokio::io::AsyncWriteExt;

        let (mut out_writer, out_reader) = tokio::io::duplex(1024);
        let (mut err_writer, err_reader) = tokio::io::duplex(1024);
        out_writer
            .write_all(b"step 1/2\nstdout-row\n")
            .await
            .unwrap();
        err_writer.write_all(b"stderr-row\n").await.unwrap();
        let mut out = BufReader::new(out_reader).lines();
        let mut err = BufReader::new(err_reader).lines();
        let mut tail = std::collections::VecDeque::new();
        let mut events = Vec::new();
        let start = Instant::now();
        let outcome = tokio::time::timeout(
            CANCEL_DRAIN_TIMEOUT + Duration::from_secs(2),
            drain_cancelled_output(
                &mut out,
                &mut err,
                &mut steps,
                &mut |e| events.push(e),
                &mut tail,
            ),
        )
        .await
        .expect("cancellation must return while writers remain open");
        assert_eq!(outcome, JobOutcome::Cancelled);
        assert!(start.elapsed() >= CANCEL_DRAIN_TIMEOUT);
        assert!(start.elapsed() < CANCEL_DRAIN_TIMEOUT + Duration::from_secs(2));
        for line in ["step 1/2", "stdout-row", "stderr-row"] {
            assert!(events.contains(&JobEvent::Line(line.to_string())));
            assert!(tail.iter().any(|l| l == line));
        }
        assert!(events.contains(&JobEvent::Progress(steps("step 1/2").unwrap())));
        // Keep both writers alive until after the bounded drain completes.
        drop((out_writer, err_writer));
    }

    /// A real child process through the platform shell.
    fn shell(script: &str, cwd: &std::path::Path, outputs: Vec<PathBuf>) -> JobCommand {
        #[cfg(windows)]
        let (binary, args) = ("cmd", vec!["/C".to_string(), script.to_string()]);
        #[cfg(not(windows))]
        let (binary, args) = ("sh", vec!["-c".to_string(), script.to_string()]);
        JobCommand {
            binary: PathBuf::from(binary),
            args,
            cwd: Some(cwd.to_path_buf()),
            outputs,
            memory_bytes: 0,
            priority: crate::jobs::Priority::Background,
        }
    }

    /// `step 2/3` lines, the shape sd.cpp and whisper.cpp print.
    fn steps(line: &str) -> Option<JobProgress> {
        let rest = line.trim().strip_prefix("step ")?;
        let (done, total) = rest.split_once('/')?;
        Some(JobProgress {
            done: done.parse().ok()?,
            total: total.parse().ok(),
            phase: None,
        })
    }

    #[tokio::test]
    async fn completes_with_progress_and_files() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out.txt");
        let cmd = shell(
            "echo step 1/2&& echo step 2/2&& echo done> out.txt",
            dir.path(),
            vec![out.clone()],
        );
        let (_tx, rx) = watch::channel(false);
        let mut progress = Vec::new();
        let outcome = run_job(&cmd, steps, rx, |e| {
            if let JobEvent::Progress(p) = e {
                progress.push(p.done)
            }
        })
        .await;
        assert_eq!(outcome, JobOutcome::Completed { files: vec![out] });
        assert_eq!(progress, vec![1, 2]);
    }

    #[tokio::test]
    async fn missing_output_and_exit_code_fail() {
        let dir = tempfile::tempdir().unwrap();
        let (_tx, rx) = watch::channel(false);
        let cmd = shell("echo hi", dir.path(), vec![dir.path().join("never.png")]);
        match run_job(&cmd, steps, rx, |_| {}).await {
            JobOutcome::Failed { reason, tail, .. } => {
                assert_eq!(reason, "jobs.missing_output");
                assert_eq!(tail.last().map(|s| s.trim()), Some("hi"));
            }
            o => panic!("{o:?}"),
        }
        let (_tx, rx) = watch::channel(false);
        let cmd = shell("exit 3", dir.path(), vec![]);
        match run_job(&cmd, steps, rx, |_| {}).await {
            JobOutcome::Failed {
                reason, exit_code, ..
            } => {
                assert_eq!(reason, "jobs.exit_failed");
                assert_eq!(exit_code, Some(3));
            }
            o => panic!("{o:?}"),
        }
    }

    /// Cancellation races with lines already in the pipe / BufReader. The marker
    /// proves the child wrote every line before we request cancellation.
    #[tokio::test]
    async fn cancel_keeps_buffered_output() {
        let dir = tempfile::tempdir().unwrap();
        let ready = dir.path().join("ready.txt");
        #[cfg(windows)]
        let cmd = shell(
            "(for /L %i in (1,1,64) do @echo row-%i)& echo stderr-row 1>&2& echo ready>ready.txt& ping -n 61 127.0.0.1 >NUL",
            dir.path(),
            vec![],
        );
        #[cfg(not(windows))]
        let cmd = shell(
            "i=1; while [ $i -le 64 ]; do echo row-$i; i=$((i+1)); done; echo stderr-row >&2; echo ready > ready.txt; sleep 60",
            dir.path(),
            vec![],
        );
        let (tx, rx) = watch::channel(false);
        let mut lines = Vec::new();
        let outcome = run_job(
            &cmd,
            |_| None,
            rx,
            |event| {
                if let JobEvent::Line(line) = event {
                    if lines.is_empty() {
                        let start = Instant::now();
                        while !ready.exists() {
                            assert!(start.elapsed() < Duration::from_secs(60));
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        tx.send(true).unwrap();
                    }
                    lines.push(line.trim().to_string());
                }
            },
        )
        .await;
        assert_eq!(outcome, JobOutcome::Cancelled);
        let expected: Vec<_> = (1..=64).map(|i| format!("row-{i}")).collect();
        assert_eq!(
            lines
                .iter()
                .filter(|l| l.starts_with("row-"))
                .cloned()
                .collect::<Vec<_>>(),
            expected
        );
        assert!(lines.iter().any(|l| l == "stderr-row"));
    }

    /// The grandchild reports progress, then writes `late.txt` about 15 seconds in; killing only
    /// the shell would let it.
    #[tokio::test]
    async fn central_cancel_kills_the_process_tree() {
        let dir = tempfile::tempdir().unwrap();
        // The grandchild's work is a script file: nested quotes do not survive argv quoting.
        #[cfg(windows)]
        let cmd = {
            std::fs::write(
                dir.path().join("late.cmd"),
                "@echo step 1/9\r\n@ping -n 16 127.0.0.1 >NUL\r\n@echo late> late.txt\r\n",
            )
            .unwrap();
            shell(r"cmd /C .\late.cmd", dir.path(), vec![])
        };
        #[cfg(not(windows))]
        let cmd = {
            std::fs::write(
                dir.path().join("late.sh"),
                "echo step 1/9\nsleep 15\necho late > late.txt\n",
            )
            .unwrap();
            // `; true` keeps the shell from exec-ing into the script.
            shell("sh late.sh; true", dir.path(), vec![])
        };
        let (_tx, rx) = watch::channel(false);
        let scheduler = std::sync::Arc::new(crate::jobs::JobScheduler::default());
        let cancel_scheduler = scheduler.clone();
        // Timed from the cancel, not the spawn: a loaded machine can be slow to start `cmd`.
        let cancelled_at = std::sync::Arc::new(std::sync::Mutex::new(None::<Instant>));
        let mark = cancelled_at.clone();
        let outcome = run_job_on(scheduler, &cmd, steps, rx, move |e| {
            if matches!(e, JobEvent::Progress(_)) {
                mark.lock().unwrap().get_or_insert_with(Instant::now);
                let id = cancel_scheduler.snapshot()[0].id;
                assert!(cancel_scheduler.cancel(id));
            }
        })
        .await;
        assert_eq!(outcome, JobOutcome::Cancelled);
        let since_cancel = cancelled_at
            .lock()
            .unwrap()
            .expect("progress seen")
            .elapsed();
        // The script writes late.txt about 15 s after its first step; the bound leaves room for a
        // loaded machine while still proving the tree died before that.
        assert!(since_cancel < Duration::from_secs(10), "{since_cancel:?}");
        tokio::time::sleep(Duration::from_secs(17).saturating_sub(since_cancel)).await;
        assert!(!dir.path().join("late.txt").exists());
    }
}
