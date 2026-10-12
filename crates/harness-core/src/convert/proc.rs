//! Runs a helper process (uv, the converter, `llama-quantize`) and follows its output line by
//! line: progress bars rewrite their line with `\r`, so both `\r` and `\n` end a line.

use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

/// Lines kept for the error message of a failed run.
const TAIL_LINES: usize = 40;

/// How a run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub success: bool,
    pub cancelled: bool,
    pub code: Option<i32>,
    /// The last lines the process wrote (progress-bar redraws left out), oldest first.
    pub tail: Vec<String>,
}

impl Outcome {
    /// The tail as one block for an error parameter.
    pub fn log(&self) -> String {
        self.tail.join("\n")
    }
}

/// A progress-bar redraw (`Writing:  21%|██ | 56.6M/269M`) is not worth keeping in a log tail.
pub fn is_bar_line(line: &str) -> bool {
    line.contains("%|") || line.contains("it/s]") || line.contains("byte/s]")
}

async fn pump<R: AsyncReadExt + Unpin>(
    mut reader: R,
    tx: tokio::sync::mpsc::UnboundedSender<String>,
) {
    let mut buf = [0u8; 8192];
    let mut line: Vec<u8> = Vec::new();
    loop {
        let n = match reader.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        for &b in &buf[..n] {
            if b == b'\n' || b == b'\r' {
                if !line.is_empty() {
                    let _ = tx.send(String::from_utf8_lossy(&line).trim_end().to_string());
                    line.clear();
                }
            } else {
                line.push(b);
            }
        }
    }
    if !line.is_empty() {
        let _ = tx.send(String::from_utf8_lossy(&line).trim_end().to_string());
    }
}

/// Runs `cmd` to its end, handing every output line (stdout and stderr) to `on_line`. When
/// `cancel` turns true the process is killed and the outcome says `cancelled`.
pub async fn run(
    mut cmd: Command,
    cancel: &Arc<AtomicBool>,
    mut on_line: impl FnMut(&str),
) -> std::io::Result<Outcome> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let mut child = cmd.spawn()?;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let out = child
        .stdout
        .take()
        .map(|r| tokio::spawn(pump(r, tx.clone())));
    let err = child
        .stderr
        .take()
        .map(|r| tokio::spawn(pump(r, tx.clone())));
    drop(tx);

    let mut tail: std::collections::VecDeque<String> = std::collections::VecDeque::new();
    let mut push = |line: String, on_line: &mut dyn FnMut(&str)| {
        on_line(&line);
        if !line.is_empty() && !is_bar_line(&line) {
            if tail.len() == TAIL_LINES {
                tail.pop_front();
            }
            tail.push_back(line);
        }
    };

    let mut cancelled = false;
    let mut status = None;
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(150));
    loop {
        tokio::select! {
            line = rx.recv() => match line {
                Some(l) => push(l, &mut on_line),
                None => break,
            },
            _ = tick.tick() => {
                if cancel.load(Ordering::SeqCst) && !cancelled {
                    cancelled = true;
                    let _ = child.start_kill();
                }
            }
        }
    }
    // Both pipes are closed: the process is over or about to be.
    if let Some(h) = out {
        let _ = h.await;
    }
    if let Some(h) = err {
        let _ = h.await;
    }
    if status.is_none() {
        status = Some(child.wait().await?);
    }
    let status = status.expect("waited");
    Ok(Outcome {
        success: status.success() && !cancelled,
        cancelled,
        code: status.code(),
        tail: tail.into_iter().collect(),
    })
}
