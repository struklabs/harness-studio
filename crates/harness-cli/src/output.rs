//! stdout = data, stderr = progress and warnings.

use std::io::{IsTerminal, Write};

use serde_json::Value;

use crate::error::{CliError, CliResult};

#[derive(Debug, Clone, Copy)]
pub struct Out {
    pub json: bool,
    pub quiet: bool,
    pub yes: bool,
}

impl Out {
    /// Prints the command's result: JSON (pretty) or the human text.
    pub fn data(&self, value: &Value, human: impl FnOnce() -> String) {
        let mut stdout = std::io::stdout().lock();
        if self.json {
            let _ = writeln!(
                stdout,
                "{}",
                serde_json::to_string_pretty(value).unwrap_or_default()
            );
        } else {
            let text = human();
            if !text.is_empty() {
                let _ = writeln!(stdout, "{}", text.trim_end_matches('\n'));
            }
        }
        let _ = stdout.flush();
    }

    /// One JSON line (streams: `logs -f`, `serve` events).
    pub fn json_line(&self, value: &Value) {
        let mut stdout = std::io::stdout().lock();
        let _ = writeln!(
            stdout,
            "{}",
            serde_json::to_string(value).unwrap_or_default()
        );
        let _ = stdout.flush();
    }

    /// A JSON document that is the data itself in both modes (`profile export`).
    pub fn json_line_pretty(&self, value: &Value) {
        let mut stdout = std::io::stdout().lock();
        let _ = writeln!(
            stdout,
            "{}",
            serde_json::to_string_pretty(value).unwrap_or_default()
        );
        let _ = stdout.flush();
    }

    /// Progress / info on stderr (suppressed by --quiet).
    pub fn info(&self, msg: impl AsRef<str>) {
        if !self.quiet {
            eprintln!("{}", msg.as_ref());
        }
    }

    /// Warnings always go to stderr.
    pub fn warn(&self, msg: impl AsRef<str>) {
        eprintln!("warning: {}", msg.as_ref());
    }

    /// A progress line rewritten in place on a terminal (plain lines otherwise, at most one per
    /// call; the caller throttles).
    pub fn progress(&self, msg: &str) {
        if self.quiet {
            return;
        }
        let mut stderr = std::io::stderr().lock();
        if std::io::stderr().is_terminal() {
            let _ = write!(stderr, "\r\x1b[2K{}", msg);
        } else {
            let _ = writeln!(stderr, "{}", msg);
        }
        let _ = stderr.flush();
    }

    /// Ends an in-place progress line.
    pub fn progress_done(&self) {
        if !self.quiet && std::io::stderr().is_terminal() {
            eprintln!();
        }
    }

    /// Asks before a destructive change. `--yes` skips it; without a terminal it is an error
    /// (exit 2) instead of a silent yes.
    pub fn confirm(&self, question: &str) -> CliResult<()> {
        if self.yes {
            return Ok(());
        }
        if !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal() {
            return Err(CliError::from_code(
                crate::error::Exit::Usage,
                "cli.confirmation_required",
                Default::default(),
            ));
        }
        eprint!("{} [y/N] ", question);
        let _ = std::io::stderr().flush();
        let mut line = String::new();
        std::io::stdin()
            .read_line(&mut line)
            .map_err(|e| CliError::general("cli.io", e.to_string()))?;
        if matches!(line.trim().to_lowercase().as_str(), "y" | "yes") {
            Ok(())
        } else {
            Err(CliError::from_code(
                crate::error::Exit::General,
                "cli.aborted",
                Default::default(),
            ))
        }
    }

    /// Prints an error to stderr (JSON with --json).
    pub fn error(&self, err: &CliError) {
        if self.json {
            eprintln!(
                "{}",
                serde_json::to_string(&err.to_json()).unwrap_or_default()
            );
        } else {
            eprintln!("error: {}", err.message);
        }
    }
}

/// A plain text table: header plus rows, columns padded to the widest cell.
pub fn table(header: &[&str], rows: &[Vec<String>]) -> String {
    let cols = header.len();
    let mut width: Vec<usize> = header.iter().map(|h| h.chars().count()).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate().take(cols) {
            width[i] = width[i].max(cell.chars().count());
        }
    }
    let line = |cells: Vec<&str>| {
        let mut s = String::new();
        for (i, cell) in cells.iter().enumerate() {
            if i + 1 == cells.len() {
                s.push_str(cell);
            } else {
                s.push_str(cell);
                s.push_str(&" ".repeat(width[i] - cell.chars().count() + 2));
            }
        }
        s.trim_end().to_string()
    };
    let mut out = vec![line(header.to_vec())];
    for row in rows {
        out.push(line(row.iter().map(String::as_str).collect()));
    }
    out.join("\n")
}

pub fn bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut unit = 0;
    while v >= 1000.0 && unit + 1 < UNITS.len() {
        v /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} B", n)
    } else {
        format!("{:.1} {}", v, UNITS[unit])
    }
}

/// `"Running"` / `{"Error": "..."}` → `running` / `error: ...`.
pub fn status_text(status: &Value) -> String {
    match status {
        Value::String(s) => s.to_lowercase(),
        Value::Object(map) => map
            .iter()
            .next()
            .map(|(k, v)| {
                format!(
                    "{}: {}",
                    k.to_lowercase(),
                    crate::messages::core_text(v.as_str().unwrap_or_default())
                )
            })
            .unwrap_or_default(),
        _ => String::new(),
    }
}

pub fn str_of(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_and_sizes() {
        let t = table(
            &["A", "BB"],
            &[
                vec!["xxx".into(), "1".into()],
                vec!["y".into(), "22".into()],
            ],
        );
        assert_eq!(t, "A    BB\nxxx  1\ny    22");
        assert_eq!(bytes(999), "999 B");
        assert_eq!(bytes(2_500_000_000), "2.5 GB");
        assert_eq!(status_text(&serde_json::json!("Running")), "running");
        assert_eq!(
            status_text(&serde_json::json!({"Error": "boom"})),
            "error: boom"
        );
    }
}
