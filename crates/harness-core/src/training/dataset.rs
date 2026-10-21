//! Dataset checks for a training job: JSONL in the chat (`messages`) or instruction
//! (`instruction`/`input`/`output`) format, the train/validation split, size and licence.
//!
//! The checks run in the Rust service before anything is queued; the worker re-reads the same
//! file (a snapshot in the job folder) and applies the same split.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::Path;

/// Largest dataset file a job accepts.
pub const MAX_BYTES: u64 = 256 * 1024 * 1024;
/// Longest single row (a row this long cannot be a chat turn pair for a small model).
const MAX_LINE_BYTES: usize = 1024 * 1024;
/// Issues listed in a report (the counts stay exact).
const MAX_ISSUES: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum DatasetFormat {
    /// `{"messages": [{"role": "user", "content": "..."}, ...]}`
    Chat,
    /// `{"instruction": "...", "input": "...", "output": "..."}`
    Instruction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum IssueLevel {
    Error,
    Warning,
}

/// One finding: an i18n key (`training.dataset_*`), the line it is about and its values.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DatasetIssue {
    pub level: IssueLevel,
    pub key: String,
    pub line: Option<u64>,
    pub params: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DatasetReport {
    pub path: String,
    pub format: Option<DatasetFormat>,
    pub rows: u64,
    pub train_rows: u64,
    pub val_rows: u64,
    pub bytes: u64,
    /// Rough token count (about four characters per token).
    pub estimated_tokens: u64,
    /// Longest row, in estimated tokens.
    pub max_row_tokens: u64,
    /// The `license` field of the rows, a `<file>.meta.json` next to the file, else `None`.
    pub license: Option<String>,
    pub issues: Vec<DatasetIssue>,
    /// No error: the job can use it.
    pub ok: bool,
}

fn issue(
    level: IssueLevel,
    key: &str,
    line: Option<u64>,
    params: &[(&str, String)],
) -> DatasetIssue {
    DatasetIssue {
        level,
        key: key.to_string(),
        line,
        params: params
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect(),
    }
}

/// Validation rows: the worker takes `max(1, floor(rows * fraction))` of the shuffled rows.
pub fn split(rows: u64, validation_fraction: f64) -> (u64, u64) {
    let fraction = validation_fraction.clamp(0.0, 0.5);
    let val = ((rows as f64 * fraction).floor() as u64).max(1).min(rows);
    (rows - val, val)
}

struct Row {
    format: DatasetFormat,
    chars: u64,
    license: Option<String>,
}

fn text(value: &Value) -> Option<&str> {
    value.as_str().filter(|s| !s.trim().is_empty())
}

fn parse_row(value: &Value) -> Result<Row, (&'static str, String)> {
    let object = value
        .as_object()
        .ok_or(("training.dataset_row_not_object", String::new()))?;
    let license = object
        .get("license")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    if let Some(messages) = object.get("messages") {
        let list = messages
            .as_array()
            .filter(|m| m.len() >= 2)
            .ok_or(("training.dataset_messages_invalid", String::new()))?;
        let mut chars = 0;
        for (i, message) in list.iter().enumerate() {
            let role = message.get("role").and_then(Value::as_str).unwrap_or("");
            let content = message
                .get("content")
                .and_then(text)
                .ok_or(("training.dataset_message_empty", i.to_string()))?;
            if !matches!(role, "system" | "user" | "assistant") {
                return Err(("training.dataset_role_invalid", role.to_string()));
            }
            chars += content.chars().count() as u64;
        }
        let last = list
            .last()
            .and_then(|m| m.get("role"))
            .and_then(Value::as_str);
        if last != Some("assistant") {
            return Err(("training.dataset_last_not_assistant", String::new()));
        }
        return Ok(Row {
            format: DatasetFormat::Chat,
            chars,
            license,
        });
    }
    let instruction = object.get("instruction").and_then(text);
    let output = object.get("output").and_then(text);
    match (instruction, output) {
        (Some(i), Some(o)) => {
            let input = object.get("input").and_then(Value::as_str).unwrap_or("");
            Ok(Row {
                format: DatasetFormat::Instruction,
                chars: (i.chars().count() + input.chars().count() + o.chars().count()) as u64,
                license,
            })
        }
        _ => Err(("training.dataset_row_unknown_format", String::new())),
    }
}

/// Reads `path` and reports on it. Never fails: a missing or unreadable file is an error issue.
pub fn inspect(path: &Path, validation_fraction: f64) -> DatasetReport {
    let mut report = DatasetReport {
        path: path.display().to_string(),
        format: None,
        rows: 0,
        train_rows: 0,
        val_rows: 0,
        bytes: 0,
        estimated_tokens: 0,
        max_row_tokens: 0,
        license: None,
        issues: Vec::new(),
        ok: false,
    };
    let fail = |report: &mut DatasetReport, key: &str, detail: String| {
        report
            .issues
            .push(issue(IssueLevel::Error, key, None, &[("error", detail)]));
    };
    let meta = match std::fs::metadata(path) {
        Ok(m) if m.is_file() => m,
        Ok(_) => {
            fail(&mut report, "training.dataset_not_file", String::new());
            return report;
        }
        Err(e) => {
            fail(&mut report, "training.dataset_unreadable", e.to_string());
            return report;
        }
    };
    report.bytes = meta.len();
    if meta.len() > MAX_BYTES {
        fail(
            &mut report,
            "training.dataset_too_large",
            format!("{} MB", MAX_BYTES / 1024 / 1024),
        );
        return report;
    }
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => {
            fail(&mut report, "training.dataset_unreadable", e.to_string());
            return report;
        }
    };
    let mut reader = BufReader::new(file);
    let mut licenses: Vec<String> = Vec::new();
    let mut without_license = 0u64;
    let mut bad = 0u64;
    let mut mixed = false;
    let mut line_no = 0u64;
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf) {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) => {
                fail(&mut report, "training.dataset_unreadable", e.to_string());
                return report;
            }
        }
        line_no += 1;
        let raw = String::from_utf8_lossy(&buf);
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let mut reject = |report: &mut DatasetReport, key: &str, detail: String| {
            bad += 1;
            if report.issues.len() < MAX_ISSUES {
                report.issues.push(issue(
                    IssueLevel::Error,
                    key,
                    Some(line_no),
                    &[("detail", detail)],
                ));
            }
        };
        if buf.len() > MAX_LINE_BYTES {
            reject(&mut report, "training.dataset_row_too_long", String::new());
            continue;
        }
        let value: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                reject(&mut report, "training.dataset_row_not_json", e.to_string());
                continue;
            }
        };
        match parse_row(&value) {
            Ok(row) => {
                match report.format {
                    None => report.format = Some(row.format),
                    Some(f) if f != row.format => mixed = true,
                    _ => {}
                }
                report.rows += 1;
                let tokens = row.chars / 4 + 1;
                report.estimated_tokens += tokens;
                report.max_row_tokens = report.max_row_tokens.max(tokens);
                match row.license {
                    Some(l) => {
                        if !licenses.contains(&l) {
                            licenses.push(l);
                        }
                    }
                    None => without_license += 1,
                }
            }
            Err((key, detail)) => reject(&mut report, key, detail),
        }
    }
    if bad > 0 {
        report.issues.push(issue(
            IssueLevel::Error,
            "training.dataset_bad_rows",
            None,
            &[("count", bad.to_string())],
        ));
    }
    if mixed {
        report.issues.push(issue(
            IssueLevel::Error,
            "training.dataset_mixed_formats",
            None,
            &[],
        ));
    }
    if report.rows < MIN_ROWS {
        report.issues.push(issue(
            IssueLevel::Error,
            "training.dataset_too_few_rows",
            None,
            &[
                ("rows", report.rows.to_string()),
                ("min", MIN_ROWS.to_string()),
            ],
        ));
    } else if report.rows < 20 {
        report.issues.push(issue(
            IssueLevel::Warning,
            "training.dataset_small",
            None,
            &[("rows", report.rows.to_string())],
        ));
    }
    let (train, val) = split(report.rows, validation_fraction);
    report.train_rows = train;
    report.val_rows = val;
    report.license = match licenses.len() {
        0 => sidecar_license(path),
        1 if without_license == 0 => licenses.pop(),
        1 => licenses.pop().map(|l| format!("{l} (not on every row)")),
        _ => Some(format!("mixed: {}", licenses.join(", "))),
    };
    if report.license.is_none() {
        report.issues.push(issue(
            IssueLevel::Warning,
            "training.dataset_license_unknown",
            None,
            &[],
        ));
    }
    report.ok = !report.issues.iter().any(|i| i.level == IssueLevel::Error);
    report
}

/// Fewest rows that leave one for validation and one for training.
pub const MIN_ROWS: u64 = 2;

/// `<file>.meta.json` with a `license` string (the dataset card of a downloaded dataset).
fn sidecar_license(path: &Path) -> Option<String> {
    let mut name = path.file_name()?.to_os_string();
    name.push(".meta.json");
    let value: Value =
        serde_json::from_str(&std::fs::read_to_string(path.with_file_name(name)).ok()?).ok()?;
    value
        .get("license")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, body: &str) -> std::path::PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn chat_rows_split_and_license() {
        let dir = tempfile::tempdir().unwrap();
        let mut body = String::new();
        for i in 0..25 {
            body.push_str(&format!(
                "{{\"messages\":[{{\"role\":\"user\",\"content\":\"q{i}\"}},{{\"role\":\"assistant\",\"content\":\"a{i}\"}}],\"license\":\"cc-by-4.0\"}}\n"
            ));
        }
        let p = write(dir.path(), "d.jsonl", &body);
        let r = inspect(&p, 0.1);
        assert!(r.ok, "{:?}", r.issues);
        assert_eq!(r.format, Some(DatasetFormat::Chat));
        assert_eq!((r.rows, r.train_rows, r.val_rows), (25, 23, 2));
        assert_eq!(r.license.as_deref(), Some("cc-by-4.0"));
    }

    #[test]
    fn instruction_rows_without_license_warn_and_sidecar_supplies_it() {
        let dir = tempfile::tempdir().unwrap();
        let body = "{\"instruction\":\"a\",\"output\":\"b\"}\n".repeat(30);
        let p = write(dir.path(), "d.jsonl", &body);
        let r = inspect(&p, 0.1);
        assert!(r.ok);
        assert_eq!(r.format, Some(DatasetFormat::Instruction));
        assert!(r.license.is_none());
        assert!(r
            .issues
            .iter()
            .any(|i| i.key == "training.dataset_license_unknown"));
        write(dir.path(), "d.jsonl.meta.json", "{\"license\":\"mit\"}");
        assert_eq!(inspect(&p, 0.1).license.as_deref(), Some("mit"));
    }

    #[test]
    fn bad_rows_are_reported_with_lines() {
        let dir = tempfile::tempdir().unwrap();
        let body = "{\"instruction\":\"a\",\"output\":\"b\"}\nnot json\n{\"messages\":[{\"role\":\"user\",\"content\":\"x\"}]}\n{\"instruction\":\"a\",\"output\":\"b\"}\n";
        let p = write(dir.path(), "d.jsonl", body);
        let r = inspect(&p, 0.1);
        assert!(!r.ok);
        assert_eq!(r.rows, 2);
        assert!(r.issues.iter().any(|i| i.line == Some(2)));
        assert!(r
            .issues
            .iter()
            .any(|i| i.key == "training.dataset_bad_rows"));
    }

    #[test]
    fn mixed_formats_missing_file_and_too_few_rows() {
        let dir = tempfile::tempdir().unwrap();
        let body = "{\"instruction\":\"a\",\"output\":\"b\"}\n{\"messages\":[{\"role\":\"user\",\"content\":\"x\"},{\"role\":\"assistant\",\"content\":\"y\"}]}\n";
        let p = write(dir.path(), "m.jsonl", body);
        assert!(inspect(&p, 0.1)
            .issues
            .iter()
            .any(|i| i.key == "training.dataset_mixed_formats"));
        let one = write(
            dir.path(),
            "o.jsonl",
            "{\"instruction\":\"a\",\"output\":\"b\"}\n",
        );
        assert!(inspect(&one, 0.1)
            .issues
            .iter()
            .any(|i| i.key == "training.dataset_too_few_rows"));
        assert!(!inspect(&dir.path().join("none.jsonl"), 0.1).ok);
        assert_eq!(split(50, 0.1), (45, 5));
        assert_eq!(split(3, 0.0), (2, 1));
    }
}
