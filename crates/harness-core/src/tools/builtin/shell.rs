//! `exec_shell_command`: runs a command line with the platform shell in the working
//! directory (`cmd /c` on Windows, `sh -c` elsewhere, like llama-server), stdout and stderr
//! combined. A timeout or a cancel kills the whole process tree (a Windows Job Object, a Unix
//! process group), not just the shell.

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use command_group::CommandGroup;
use serde_json::Value;

use crate::tools::registry::{args, CancelToken, ToolCtx};
use crate::tools::scope::plain_path;
use crate::tools::types::*;

const DEFAULT_TIMEOUT_SECS: i64 = 10;
const MAX_TIMEOUT_SECS: i64 = 60;
const MAX_OUTPUT: usize = 16 * 1024;
/// How long the readers get to drain the pipes after the process ended.
const DRAIN_GRACE: Duration = Duration::from_millis(1500);

#[derive(Debug)]
pub struct Outcome {
    pub output: Vec<u8>,
    pub truncated: bool,
    pub exit_code: i32,
    pub timed_out: bool,
    pub cancelled: bool,
}

#[derive(Default)]
struct Sink {
    data: Vec<u8>,
    truncated: bool,
}

fn shell_command(command: &str) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let shell = std::env::var_os("ComSpec").unwrap_or_else(|| "cmd.exe".into());
        let mut c = Command::new(shell);
        // `/s` strips the outer quotes, so the command line reaches cmd untouched (Rust's own
        // argument quoting is not what cmd parses).
        c.args(["/d", "/s", "/c"]).raw_arg(format!("\"{command}\""));
        c
    }
    #[cfg(not(windows))]
    {
        let mut c = Command::new("sh");
        c.arg("-c").arg(command);
        crate::integrations::launch::apply_augmented_path(&mut c);
        c
    }
}

fn pump(mut pipe: impl Read + Send + 'static, sink: Arc<Mutex<Sink>>, cap: usize) {
    let mut buf = [0u8; 4096];
    loop {
        match pipe.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                // Keep draining past the cap: a child blocked on a full pipe never ends.
                let mut s = sink.lock().unwrap_or_else(|e| e.into_inner());
                if s.truncated {
                    continue;
                }
                let room = cap.saturating_sub(s.data.len());
                if n <= room {
                    s.data.extend_from_slice(&buf[..n]);
                } else {
                    s.data.extend_from_slice(&buf[..room]);
                    s.truncated = true;
                }
            }
        }
    }
}

/// `cmd.exe` (the Windows shell) cannot have a UNC current directory: it would fall back to
/// another directory without saying so.
fn cmd_cannot_run_in(cwd: &std::path::Path) -> bool {
    cfg!(windows) && cwd.to_string_lossy().starts_with(r"\\")
}

/// Runs `command` in `cwd`. Blocks; polls `cancel`.
pub fn run_shell(
    command: &str,
    cwd: &std::path::Path,
    timeout: Duration,
    max_output: usize,
    cancel: &CancelToken,
) -> Result<Outcome, ToolError> {
    let cwd = plain_path(cwd);
    if cmd_cannot_run_in(&cwd) {
        return Err(ToolError::new(
            ToolErrorCode::Io,
            format!(
                "the shell tool cannot run in a network (UNC) directory ({}): cmd.exe does not keep UNC working directories. Map the share to a drive letter first.",
                cwd.display()
            ),
        ));
    }
    let mut cmd = shell_command(command);
    cmd.current_dir(&cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    crate::process_limits::prepare(&mut cmd)
        .map_err(|e| ToolError::new(ToolErrorCode::Io, e.to_string()))?;
    let mut group = cmd.group();
    // Windows: the Job Object closes with the handle and takes every descendant with it. Unix:
    // the process group is killed explicitly below.
    #[cfg(windows)]
    group.kill_on_drop(true).creation_flags(0x0800_0000); // group spawn overrides cmd's flags
    let mut child = group
        .spawn()
        .map_err(|e| ToolError::new(ToolErrorCode::Io, format!("failed to spawn process: {e}")))?;

    #[cfg(windows)]
    let _memory_job = {
        use std::os::windows::io::AsRawHandle;
        crate::process_limits::memory_job(child.inner().as_raw_handle())
            .map_err(|e| ToolError::new(ToolErrorCode::Io, e.to_string()))?
    };

    let _group_guard = crate::process_limits::GroupGuard(child.id());
    // One buffer for both pipes, in the order the bytes arrive.
    let sink = Arc::new(Mutex::new(Sink::default()));
    let mut readers = Vec::new();
    let readers_started = (|| -> std::io::Result<()> {
        if let Some(out) = child.inner().stdout.take() {
            let s = sink.clone();
            readers.push(crate::resource_io::spawn_thread(
                "harness-shell-stdout",
                move || pump(out, s, max_output),
            )?);
        }
        if let Some(err) = child.inner().stderr.take() {
            let s = sink.clone();
            readers.push(crate::resource_io::spawn_thread(
                "harness-shell-stderr",
                move || pump(err, s, max_output),
            )?);
        }

        Ok(())
    })();
    if let Err(e) = readers_started {
        let _ = child.kill();
        let _ = child.wait();
        return Err(ToolError::new(
            ToolErrorCode::Io,
            String::from(crate::error_key::err("process.log_reader_failed").with("error", e)),
        ));
    }

    let deadline = Instant::now() + timeout;
    let mut timed_out = false;
    let mut cancelled = false;
    let exit_code = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code().unwrap_or(-1),
            Ok(None) => {}
            Err(_) => break -1,
        }
        if cancel.is_cancelled() {
            cancelled = true;
        } else if Instant::now() >= deadline {
            timed_out = true;
        }
        if cancelled || timed_out {
            let _ = child.kill();
            break child.wait().ok().and_then(|s| s.code()).unwrap_or(-1);
        }
        std::thread::sleep(Duration::from_millis(15));
    };
    // Windows: a command that left background processes behind takes them down with the job.
    #[cfg(windows)]
    drop(child);
    #[cfg(not(windows))]
    {
        let _ = child.kill();
        drop(child);
    }

    // The pipes close when every holder is gone; a descendant that escaped the group could keep
    // them open, so wait only a moment.
    let grace = Instant::now() + DRAIN_GRACE;
    for r in readers {
        while !r.is_finished() && Instant::now() < grace {
            std::thread::sleep(Duration::from_millis(5));
        }
        if r.is_finished() {
            let _ = r.join();
        }
    }
    let s = sink.lock().unwrap_or_else(|e| e.into_inner());
    Ok(Outcome {
        output: s.data.clone(),
        truncated: s.truncated,
        exit_code,
        timed_out,
        cancelled,
    })
}

/// Child output to text: UTF-8 as is; otherwise (Windows consoles) in the OEM code page.
pub fn console_text(bytes: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    #[cfg(windows)]
    {
        if let Some(s) = oem_to_string(bytes) {
            return s;
        }
    }
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(windows)]
fn oem_to_string(bytes: &[u8]) -> Option<String> {
    use windows::Win32::Globalization::{
        GetOEMCP, MultiByteToWideChar, MULTI_BYTE_TO_WIDE_CHAR_FLAGS,
    };
    // A chunk can end inside a multi-byte sequence; the OEM pages here are single-byte except
    // the East Asian ones, where a cut tail is replaced rather than guessed.
    let cp = unsafe { GetOEMCP() };
    let flags = MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0);
    let len = unsafe { MultiByteToWideChar(cp, flags, bytes, None) };
    if len <= 0 {
        return None;
    }
    let mut wide = vec![0u16; len as usize];
    let written = unsafe { MultiByteToWideChar(cp, flags, bytes, Some(&mut wide)) };
    if written <= 0 {
        return None;
    }
    Some(String::from_utf16_lossy(&wide[..written as usize]))
}

pub fn exec_shell_command(a: &Value, ctx: &ToolCtx) -> Result<ToolOutput, ToolError> {
    let ws = ctx.workspace()?;
    let command = args::string(a, "command")?;
    let timeout = args::opt_i64(a, "timeout")
        .unwrap_or(DEFAULT_TIMEOUT_SECS)
        .clamp(1, MAX_TIMEOUT_SECS) as u64;
    let max_output = (args::opt_i64(a, "max_output_size").unwrap_or(MAX_OUTPUT as i64))
        .clamp(0, MAX_OUTPUT as i64) as usize;

    let outcome = run_shell(
        &command,
        ws.root(),
        Duration::from_secs(timeout),
        max_output,
        &ctx.cancel,
    )?;
    if outcome.cancelled {
        return Err(ToolError::new(
            ToolErrorCode::Cancelled,
            "the command was cancelled",
        ));
    }
    let mut text = console_text(&outcome.output);
    if outcome.truncated {
        text.push_str("\n[output truncated]");
    }
    text.push_str(&format!("\n[exit code: {}]", outcome.exit_code));
    if outcome.timed_out {
        text.push_str(" [exit due to timed out]");
    }
    Ok(ToolOutput::text(text))
}

#[cfg(test)]
mod tests {
    #[test]
    fn unc_cwd_is_refused_on_windows_only() {
        let unc = std::path::Path::new(r"\\server\share\dir");
        assert_eq!(super::cmd_cannot_run_in(unc), cfg!(windows));
        assert!(!super::cmd_cannot_run_in(std::path::Path::new(r"C:\work")));
    }

    use super::super::test_support::{run_in, tree};
    use crate::tools::registry::{RunLimits, ToolCtx, ToolRegistry};
    use crate::tools::{ToolErrorCode, ToolInvocation, ToolOrigin, Workspace};
    use serde_json::json;
    use std::time::{Duration, Instant};

    #[tokio::test]
    async fn runs_a_command_in_the_workspace_with_combined_output() {
        // Explicit timeouts: under heavy load cmd.exe alone can take several seconds to start.
        let d = tree(&[("marker.txt", "m")]);
        let (list, both) = if cfg!(windows) {
            ("dir /b", "echo out& echo err 1>&2")
        } else {
            ("ls", "echo out; echo err >&2")
        };
        let r = run_in(
            d.path(),
            "exec_shell_command",
            json!({"command": list, "timeout": 60}),
        )
        .await;
        assert!(!r.is_error(), "{}", r.content);
        assert!(r.content.contains("marker.txt"), "{}", r.content);
        assert!(r.content.ends_with("\n[exit code: 0]"), "{}", r.content);

        let r = run_in(
            d.path(),
            "exec_shell_command",
            json!({"command": both, "timeout": 60}),
        )
        .await;
        assert!(
            r.content.contains("out") && r.content.contains("err"),
            "{}",
            r.content
        );

        // Quotes reach the shell as written.
        let r = run_in(
            d.path(),
            "exec_shell_command",
            json!({"command": "echo \"two words\" \"and 'more'\"", "timeout": 60}),
        )
        .await;
        assert!(r.content.contains("two words"), "{}", r.content);
        assert!(r.content.contains("and"), "{}", r.content);

        // A failing command: its exit code, not a tool error.
        let r = run_in(
            d.path(),
            "exec_shell_command",
            json!({"command": "exit 3", "timeout": 60}),
        )
        .await;
        assert!(!r.is_error());
        assert!(r.content.ends_with("[exit code: 3]"), "{}", r.content);
    }

    #[tokio::test]
    async fn output_is_cut_at_max_output_size() {
        let d = tree(&[]);
        let cmd = if cfg!(windows) {
            "for /L %i in (1,1,400) do @echo 0123456789abcdef0123456789abcdef"
        } else {
            "yes 0123456789abcdef0123456789abcdef | head -n 400"
        };
        let r = run_in(
            d.path(),
            "exec_shell_command",
            json!({"command": cmd, "max_output_size": 500, "timeout": 60}),
        )
        .await;
        assert!(r.content.contains("[output truncated]"), "{}", r.content);
        assert!(r.content.len() < 700, "{}", r.content.len());
    }

    #[tokio::test]
    async fn a_command_that_takes_too_long_is_killed() {
        let d = tree(&[]);
        // The command runs ~60 s; the bound proves the kill while leaving room for a first run
        // right after a build (an antivirus scan can hold new processes for many seconds).
        let cmd = if cfg!(windows) {
            "ping -n 60 127.0.0.1"
        } else {
            "sleep 60"
        };
        let t = Instant::now();
        let r = run_in(
            d.path(),
            "exec_shell_command",
            json!({"command": cmd, "timeout": 1}),
        )
        .await;
        assert!(t.elapsed() < Duration::from_secs(40), "{:?}", t.elapsed());
        assert!(
            r.content.contains("[exit due to timed out]"),
            "{}",
            r.content
        );
    }

    /// The command starts a grandchild that would write `done.txt` after a few seconds. After the
    /// call is cancelled the file must never appear: the process tree is gone, not just the
    /// shell.
    #[tokio::test]
    async fn cancel_kills_the_whole_process_tree() {
        let d = tree(&[]);
        let dir = d.path().to_path_buf();
        let cmd = if cfg!(windows) {
            // cmd -> powershell (the grandchild).
            "powershell -NoProfile -Command \"Set-Content started.txt 1; Start-Sleep 4; Set-Content done.txt 1\""
        } else {
            "sh -c 'echo 1 > started.txt; sleep 4; echo 1 > done.txt'"
        };
        let reg = ToolRegistry::with_builtin();
        let ctx = ToolCtx::new(Some(Workspace::new(&dir).unwrap()));
        let token = ctx.cancel.clone();
        let inv = ToolInvocation::new(
            "exec_shell_command",
            json!({"command": cmd, "timeout": 60}),
            ToolOrigin::new("test"),
        );
        let watcher_dir = dir.clone();
        let watcher = tokio::spawn(async move {
            let t = Instant::now();
            while !watcher_dir.join("started.txt").exists() {
                assert!(
                    // PowerShell's first start after a build can take this long (antivirus).
                    t.elapsed() < Duration::from_secs(60),
                    "the grandchild never started"
                );
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            token.cancel();
        });
        let r = reg.run(&inv, &ctx, &RunLimits::default()).await;
        watcher.await.unwrap();
        assert_eq!(r.error, Some(ToolErrorCode::Cancelled), "{}", r.content);
        // Longer than the grandchild's sleep.
        tokio::time::sleep(Duration::from_secs(5)).await;
        assert!(dir.join("started.txt").exists());
        assert!(
            !dir.join("done.txt").exists(),
            "the grandchild survived the cancel"
        );
    }

    /// Dropping the call's future (the HTTP client went away, the outer timeout fired) stops the
    /// process too.
    #[tokio::test]
    async fn dropping_the_call_kills_the_process_tree() {
        let d = tree(&[]);
        let dir = d.path().to_path_buf();
        let cmd = if cfg!(windows) {
            "powershell -NoProfile -Command \"Set-Content started.txt 1; Start-Sleep 4; Set-Content done.txt 1\""
        } else {
            "sh -c 'echo 1 > started.txt; sleep 4; echo 1 > done.txt'"
        };
        let reg = ToolRegistry::with_builtin();
        let ctx = ToolCtx::new(Some(Workspace::new(&dir).unwrap()));
        let inv = ToolInvocation::new(
            "exec_shell_command",
            json!({"command": cmd, "timeout": 60}),
            ToolOrigin::new("test"),
        );
        let limits = RunLimits::default();
        let started = dir.join("started.txt");
        let call = reg.run(&inv, &ctx, &limits);
        let waiting = async {
            let t = Instant::now();
            while !started.exists() {
                assert!(
                    // PowerShell's first start after a build can take this long (antivirus).
                    t.elapsed() < Duration::from_secs(60),
                    "the grandchild never started"
                );
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        };
        tokio::select! {
            _ = call => panic!("the command should still be running"),
            _ = waiting => {}
        }
        // `call` was dropped by the select.
        tokio::time::sleep(Duration::from_secs(5)).await;
        assert!(
            !dir.join("done.txt").exists(),
            "the grandchild survived the drop"
        );
    }
}
