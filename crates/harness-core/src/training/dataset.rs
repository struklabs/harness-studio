//! Dataset checks for a training job. A dataset is a JSONL, JSON (array), or CSV file, or a
//! folder of such files. Rows may be chat (`messages`), ShareGPT (`conversations`),
//! instruction (`instruction`/`input`/`output`) or prompt/completion pairs; every format is
//! detected per row and normalized to chat messages in the job's snapshot.
//!
//! The checks run in the Rust service before anything is queued; the worker re-reads the
//! normalized snapshot (in the job folder) and applies the same train/validation split.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

/// Largest dataset (file, or all files of a folder) a job accepts.
pub const MAX_BYTES: u64 = 256 * 1024 * 1024;
/// Longest single row (a row this long cannot be a chat turn pair for a small model).
const MAX_LINE_BYTES: usize = 1024 * 1024;
/// Issues listed in a report (the counts stay exact).
const MAX_ISSUES: usize = 12;
/// Upper edges (estimated tokens) of the length histogram; one more bucket holds longer rows.
pub const HISTOGRAM_EDGES: [u64; 9] = [32, 64, 128, 256, 512, 1024, 2048, 4096, 8192];
/// Rendered sample rows in a report.
const SAMPLES: usize = 3;
/// Characters of a message kept in a sample.
const SAMPLE_CHARS: usize = 400;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum DatasetFormat {
    /// `{"messages": [{"role": "user", "content": "..."}, ...]}` (ChatML)
    Chat,
    /// `{"instruction": "...", "input": "...", "output": "..."}` (Alpaca)
    Instruction,
    /// `{"conversations": [{"from": "human", "value": "..."}, ...]}` (ShareGPT)
    Sharegpt,
    /// `{"prompt": "...", "completion": "..."}` (also question/answer, input/output)
    PromptCompletion,
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

/// One message of a sample row, as the model will see the roles.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DatasetSampleMessage {
    pub role: String,
    pub content: String,
}

/// A row after format detection and mapping (long messages are shortened).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DatasetSample {
    pub messages: Vec<DatasetSampleMessage>,
    /// Estimated tokens of the whole row.
    pub tokens: u64,
}

/// Which source field fills which role (`user`, `assistant`, `system`, or `messages`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DatasetMapping {
    pub role: String,
    pub field: String,
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
    /// Rows per length bucket (`HISTOGRAM_EDGES`, then one bucket for longer rows).
    #[serde(default)]
    pub histogram: Vec<u64>,
    /// The first rows as the model will be trained on them.
    #[serde(default)]
    pub samples: Vec<DatasetSample>,
    /// How the source fields map to roles (from the first valid row).
    #[serde(default)]
    pub mapping: Vec<DatasetMapping>,
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

/// A row after detection: the format, its messages and where each role came from.
struct Row {
    format: DatasetFormat,
    messages: Vec<(String, String)>,
    mapping: Vec<(String, String)>,
    license: Option<String>,
}

type RowError = (&'static str, String);

fn text(value: &Value) -> Option<&str> {
    value.as_str().filter(|s| !s.trim().is_empty())
}

/// The first of `names` that is a non-empty string in `object`.
fn pick<'a>(
    object: &'a serde_json::Map<String, Value>,
    names: &[&'static str],
) -> Option<(&'static str, &'a str)> {
    names
        .iter()
        .find_map(|n| object.get(*n).and_then(text).map(|t| (*n, t)))
}

fn sharegpt_role(from: &str) -> Option<&'static str> {
    match from.trim().to_ascii_lowercase().as_str() {
        "human" | "user" => Some("user"),
        "gpt" | "assistant" | "bot" | "model" | "chatgpt" => Some("assistant"),
        "system" => Some("system"),
        _ => None,
    }
}

fn parse_row(value: &Value) -> Result<Row, RowError> {
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
        let mut out = Vec::new();
        for (i, message) in list.iter().enumerate() {
            let role = message.get("role").and_then(Value::as_str).unwrap_or("");
            let content = message
                .get("content")
                .and_then(text)
                .ok_or(("training.dataset_message_empty", i.to_string()))?;
            if !matches!(role, "system" | "user" | "assistant") {
                return Err(("training.dataset_role_invalid", role.to_string()));
            }
            out.push((role.to_string(), content.to_string()));
        }
        if out.last().map(|m| m.0.as_str()) != Some("assistant") {
            return Err(("training.dataset_last_not_assistant", String::new()));
        }
        return Ok(Row {
            format: DatasetFormat::Chat,
            messages: out,
            mapping: vec![("messages".into(), "messages".into())],
            license,
        });
    }
    if let Some(conversations) = object.get("conversations") {
        let list = conversations
            .as_array()
            .filter(|m| m.len() >= 2)
            .ok_or(("training.dataset_messages_invalid", String::new()))?;
        let mut out = Vec::new();
        for (i, turn) in list.iter().enumerate() {
            let from = turn.get("from").and_then(Value::as_str).unwrap_or("");
            let content = turn
                .get("value")
                .and_then(text)
                .ok_or(("training.dataset_message_empty", i.to_string()))?;
            let role =
                sharegpt_role(from).ok_or(("training.dataset_role_invalid", from.to_string()))?;
            out.push((role.to_string(), content.to_string()));
        }
        if out.last().map(|m| m.0.as_str()) != Some("assistant") {
            return Err(("training.dataset_last_not_assistant", String::new()));
        }
        return Ok(Row {
            format: DatasetFormat::Sharegpt,
            messages: out,
            mapping: vec![("messages".into(), "conversations".into())],
            license,
        });
    }
    let system = pick(object, &["system", "system_prompt"]);
    let mut messages = Vec::new();
    let mut mapping = Vec::new();
    if let Some((field, s)) = system {
        messages.push(("system".to_string(), s.to_string()));
        mapping.push(("system".to_string(), field.to_string()));
    }
    // Alpaca: instruction (+ optional input) and output.
    if let Some(instruction) = object.get("instruction").and_then(text) {
        let (field, output) = pick(object, &["output", "response", "answer", "completion"])
            .ok_or(("training.dataset_row_unknown_format", String::new()))?;
        let input = object
            .get("input")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let (prompt, user_field) = match input {
            Some(i) => (
                format!("{}\n{}", instruction.trim(), i),
                "instruction + input",
            ),
            None => (instruction.trim().to_string(), "instruction"),
        };
        messages.push(("user".into(), prompt));
        messages.push(("assistant".into(), output.trim().to_string()));
        mapping.push(("user".into(), user_field.into()));
        mapping.push(("assistant".into(), field.into()));
        return Ok(Row {
            format: DatasetFormat::Instruction,
            messages,
            mapping,
            license,
        });
    }
    // Prompt / completion pairs under the usual column names.
    let prompt = pick(object, &["prompt", "question", "query", "input"]);
    let completion = pick(
        object,
        &["completion", "response", "answer", "output", "target"],
    );
    match (prompt, completion) {
        (Some((pf, p)), Some((cf, c))) => {
            messages.push(("user".into(), p.trim().to_string()));
            messages.push(("assistant".into(), c.trim().to_string()));
            mapping.push(("user".into(), pf.into()));
            mapping.push(("assistant".into(), cf.into()));
            Ok(Row {
                format: DatasetFormat::PromptCompletion,
                messages,
                mapping,
                license,
            })
        }
        _ => Err(("training.dataset_row_unknown_format", String::new())),
    }
}

fn row_tokens(row: &Row) -> u64 {
    let chars: usize = row.messages.iter().map(|m| m.1.chars().count()).sum();
    chars as u64 / 4 + 1
}

// --- reading the sources -----------------------------------------------------------------------

fn extension(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// A dataset file or the files of a folder (JSONL, JSON, CSV), in name order.
fn source_files(path: &Path) -> Result<Vec<PathBuf>, RowError> {
    if path.is_dir() {
        let mut files: Vec<PathBuf> = std::fs::read_dir(path)
            .map_err(|e| ("training.dataset_unreadable", e.to_string()))?
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.is_file() && matches!(extension(p).as_str(), "jsonl" | "ndjson" | "json" | "csv")
            })
            .collect();
        files.sort();
        if files.is_empty() {
            return Err(("training.dataset_folder_empty", String::new()));
        }
        return Ok(files);
    }
    Ok(vec![path.to_path_buf()])
}

/// Splits CSV text into records (RFC 4180: quoted fields, doubled quotes, newlines inside quotes).
pub fn parse_csv(input: &str) -> Vec<Vec<String>> {
    let input = input.strip_prefix('\u{feff}').unwrap_or(input);
    let mut rows = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = input.chars().peekable();
    let mut any = false;
    while let Some(c) = chars.next() {
        any = true;
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    quoted = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => quoted = true,
            ',' => row.push(std::mem::take(&mut field)),
            '\r' => {}
            '\n' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
                any = false;
            }
            _ => field.push(c),
        }
    }
    if any || !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows.retain(|r| !(r.len() == 1 && r[0].trim().is_empty()));
    rows
}

/// Calls `f(line, row)` for every row of every source file. `line` counts per file (the
/// record number for CSV and the element number for a JSON array).
fn for_each_row(
    path: &Path,
    f: &mut dyn FnMut(u64, Result<Value, RowError>),
) -> Result<(), RowError> {
    for file in source_files(path)? {
        match extension(&file).as_str() {
            "parquet" => return Err(("training.dataset_parquet_unsupported", String::new())),
            "csv" => {
                let bytes = std::fs::read(&file)
                    .map_err(|e| ("training.dataset_unreadable", e.to_string()))?;
                let rows = parse_csv(&String::from_utf8_lossy(&bytes));
                let mut it = rows.into_iter();
                let Some(header) = it.next() else {
                    continue;
                };
                let header: Vec<String> = header
                    .iter()
                    .map(|h| h.trim().to_ascii_lowercase())
                    .collect();
                for (i, record) in it.enumerate() {
                    let mut o = serde_json::Map::new();
                    for (name, value) in header.iter().zip(record) {
                        o.insert(name.clone(), Value::String(value));
                    }
                    f(i as u64 + 2, Ok(Value::Object(o)));
                }
            }
            "json" => {
                let bytes = std::fs::read(&file)
                    .map_err(|e| ("training.dataset_unreadable", e.to_string()))?;
                match serde_json::from_slice::<Value>(&bytes) {
                    Ok(Value::Array(items)) => {
                        for (i, v) in items.into_iter().enumerate() {
                            f(i as u64 + 1, Ok(v));
                        }
                    }
                    Ok(_) => return Err(("training.dataset_json_not_array", String::new())),
                    Err(e) => return Err(("training.dataset_row_not_json", e.to_string())),
                }
            }
            _ => {
                let handle = std::fs::File::open(&file)
                    .map_err(|e| ("training.dataset_unreadable", e.to_string()))?;
                let mut reader = BufReader::new(handle);
                let mut buf = Vec::new();
                let mut line_no = 0u64;
                loop {
                    buf.clear();
                    match reader.read_until(b'\n', &mut buf) {
                        Ok(0) => break,
                        Ok(_) => {}
                        Err(e) => return Err(("training.dataset_unreadable", e.to_string())),
                    }
                    line_no += 1;
                    let raw = String::from_utf8_lossy(&buf);
                    let line = raw.trim();
                    if line.is_empty() {
                        continue;
                    }
                    if buf.len() > MAX_LINE_BYTES {
                        f(
                            line_no,
                            Err(("training.dataset_row_too_long", String::new())),
                        );
                        continue;
                    }
                    match serde_json::from_str::<Value>(line) {
                        Ok(v) => f(line_no, Ok(v)),
                        Err(e) => f(
                            line_no,
                            Err(("training.dataset_row_not_json", e.to_string())),
                        ),
                    }
                }
            }
        }
    }
    Ok(())
}

fn total_bytes(path: &Path) -> Result<u64, RowError> {
    let mut sum = 0;
    for file in source_files(path)? {
        sum += std::fs::metadata(&file)
            .map_err(|e| ("training.dataset_unreadable", e.to_string()))?
            .len();
    }
    Ok(sum)
}

/// Writes the normalized snapshot (`{"messages": [...]}` per line) of every valid row of
/// `src` to `dest` and returns the row count. The worker trains on this file.
pub fn write_snapshot(src: &Path, dest: &Path) -> Result<u64, String> {
    let write_failed = |e: std::io::Error| -> String {
        crate::error_key::err("fs.write_failed")
            .with("path", dest.display())
            .with("error", e)
            .into()
    };
    let file = std::fs::File::create(dest).map_err(write_failed)?;
    let mut out = std::io::BufWriter::new(file);
    let mut rows = 0u64;
    let mut failure: Option<std::io::Error> = None;
    for_each_row(src, &mut |_, value| {
        let Ok(value) = value else { return };
        let Ok(row) = parse_row(&value) else { return };
        let messages: Vec<Value> = row
            .messages
            .iter()
            .map(|(r, c)| json!({"role": r, "content": c}))
            .collect();
        if let Err(e) = writeln!(out, "{}", json!({ "messages": messages })) {
            failure.get_or_insert(e);
        } else {
            rows += 1;
        }
    })
    .map_err(|(key, detail)| String::from(crate::error_key::err(key).with("error", detail)))?;
    if let Some(e) = failure.or_else(|| out.flush().err()) {
        return Err(write_failed(e));
    }
    Ok(rows)
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
        histogram: vec![0; HISTOGRAM_EDGES.len() + 1],
        samples: Vec::new(),
        mapping: Vec::new(),
    };
    let fail = |report: &mut DatasetReport, key: &str, detail: String| {
        report
            .issues
            .push(issue(IssueLevel::Error, key, None, &[("error", detail)]));
    };
    if let Err(e) = std::fs::metadata(path) {
        fail(&mut report, "training.dataset_unreadable", e.to_string());
        return report;
    }
    report.bytes = match total_bytes(path) {
        Ok(b) => b,
        Err((key, detail)) => {
            fail(&mut report, key, detail);
            return report;
        }
    };
    if report.bytes > MAX_BYTES {
        fail(
            &mut report,
            "training.dataset_too_large",
            format!("{} MB", MAX_BYTES / 1024 / 1024),
        );
        return report;
    }
    let mut licenses: Vec<String> = Vec::new();
    let mut without_license = 0u64;
    let mut bad = 0u64;
    let mut mixed = false;
    let walked = for_each_row(
        path,
        &mut |line_no, value| match value.and_then(|v| parse_row(&v)) {
            Ok(row) => {
                match report.format {
                    None => {
                        report.format = Some(row.format);
                        report.mapping = row
                            .mapping
                            .iter()
                            .map(|(r, f)| DatasetMapping {
                                role: r.clone(),
                                field: f.clone(),
                            })
                            .collect();
                    }
                    Some(f) if f != row.format => mixed = true,
                    _ => {}
                }
                report.rows += 1;
                let tokens = row_tokens(&row);
                report.estimated_tokens += tokens;
                report.max_row_tokens = report.max_row_tokens.max(tokens);
                let bucket = HISTOGRAM_EDGES
                    .iter()
                    .position(|e| tokens <= *e)
                    .unwrap_or(HISTOGRAM_EDGES.len());
                report.histogram[bucket] += 1;
                if report.samples.len() < SAMPLES {
                    report.samples.push(DatasetSample {
                        messages: row
                            .messages
                            .iter()
                            .map(|(r, c)| DatasetSampleMessage {
                                role: r.clone(),
                                content: shorten(c),
                            })
                            .collect(),
                        tokens,
                    });
                }
                match row.license {
                    Some(l) => {
                        if !licenses.contains(&l) {
                            licenses.push(l);
                        }
                    }
                    None => without_license += 1,
                }
            }
            Err((key, detail)) => {
                bad += 1;
                if report.issues.len() < MAX_ISSUES {
                    report.issues.push(issue(
                        IssueLevel::Error,
                        key,
                        Some(line_no),
                        &[("detail", detail)],
                    ));
                }
            }
        },
    );
    if let Err((key, detail)) = walked {
        fail(&mut report, key, detail);
        return report;
    }
    if bad > 0 {
        report.issues.push(issue(
            IssueLevel::Error,
            "training.dataset_bad_rows",
            None,
            &[("count", bad.to_string())],
        ));
    }
    // Every row is normalized to chat messages for the snapshot, so mixed formats train fine.
    if mixed {
        report.issues.push(issue(
            IssueLevel::Warning,
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
    // The license names stay as written; how they cover the rows is a (localized) warning.
    let coverage = match licenses.len() {
        0 => None,
        1 if without_license == 0 => None,
        1 => Some("training.dataset_license_partial"),
        _ => Some("training.dataset_license_mixed"),
    };
    report.license = match licenses.len() {
        0 => sidecar_license(path),
        _ => Some(licenses.join(", ")),
    };
    if let Some(key) = coverage {
        report
            .issues
            .push(issue(IssueLevel::Warning, key, None, &[]));
    }
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

fn shorten(s: &str) -> String {
    if s.chars().count() <= SAMPLE_CHARS {
        return s.to_string();
    }
    let mut out: String = s.chars().take(SAMPLE_CHARS).collect();
    out.push_str("...");
    out
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
        let mixed = inspect(&p, 0.1);
        assert!(mixed
            .issues
            .iter()
            .any(|i| i.key == "training.dataset_mixed_formats" && i.level == IssueLevel::Warning));
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

    #[test]
    fn license_coverage_is_a_keyed_warning() {
        let dir = tempfile::tempdir().unwrap();
        let row = |l: &str| {
            format!(
                "{{\"prompt\":\"q\",\"completion\":\"a\"{l}}}
"
            )
        };
        let partial = write(
            dir.path(),
            "p.jsonl",
            &(row(",\"license\":\"mit\"") + &row("")),
        );
        let r = inspect(&partial, 0.5);
        assert_eq!(r.license.as_deref(), Some("mit"));
        assert!(r
            .issues
            .iter()
            .any(|i| i.key == "training.dataset_license_partial"));
        let mixed = write(
            dir.path(),
            "m.jsonl",
            &(row(",\"license\":\"mit\"") + &row(",\"license\":\"cc-by-4.0\"")),
        );
        let r = inspect(&mixed, 0.5);
        assert_eq!(r.license.as_deref(), Some("mit, cc-by-4.0"));
        assert!(r
            .issues
            .iter()
            .any(|i| i.key == "training.dataset_license_mixed"));
    }

    fn user_assistant(r: &Row) -> (String, String) {
        (
            r.messages[0].1.clone(),
            r.messages.last().unwrap().1.clone(),
        )
    }

    #[test]
    fn formats_are_detected_and_mapped_to_messages() {
        let alpaca =
            parse_row(&json!({"instruction": "Translate", "input": "hi", "output": "salut"}))
                .unwrap();
        assert_eq!(alpaca.format, DatasetFormat::Instruction);
        assert_eq!(
            user_assistant(&alpaca),
            ("Translate\nhi".into(), "salut".into())
        );
        let share = parse_row(&json!({"conversations": [
            {"from": "system", "value": "Be brief"},
            {"from": "human", "value": "q"}, {"from": "gpt", "value": "a"}]}))
        .unwrap();
        assert_eq!(share.format, DatasetFormat::Sharegpt);
        let roles: Vec<&str> = share.messages.iter().map(|m| m.0.as_str()).collect();
        assert_eq!(roles, ["system", "user", "assistant"]);
        let chatml = parse_row(&json!({"messages": [
            {"role": "user", "content": "q"}, {"role": "assistant", "content": "a"}]}))
        .unwrap();
        assert_eq!(chatml.format, DatasetFormat::Chat);
        let qa = parse_row(&json!({"question": "q", "answer": "a"})).unwrap();
        assert_eq!(qa.format, DatasetFormat::PromptCompletion);
        assert_eq!(qa.mapping[0], ("user".to_string(), "question".to_string()));
        assert!(parse_row(&json!({"text": "plain"})).is_err());
        assert!(parse_row(&json!({"conversations": [
            {"from": "human", "value": "q"}, {"from": "human", "value": "a"}]}))
        .is_err());
    }

    #[test]
    fn csv_is_parsed_with_quotes_and_mapped() {
        let rows = parse_csv("\u{feff}Prompt,Answer\r\n\"a, b\",\"say \"\"hi\"\"\nnow\"\nx,y\n");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1], ["a, b", "say \"hi\"\nnow"]);
        let dir = tempfile::tempdir().unwrap();
        let mut body = String::from("prompt,completion\n");
        for i in 0..6 {
            body.push_str(&format!("q{i},a{i}\n"));
        }
        let p = write(dir.path(), "d.csv", &body);
        let r = inspect(&p, 0.2);
        assert!(r.ok, "{:?}", r.issues);
        assert_eq!(r.format, Some(DatasetFormat::PromptCompletion));
        assert_eq!((r.rows, r.train_rows, r.val_rows), (6, 5, 1));
        assert_eq!(r.samples.len(), 3);
        assert_eq!(r.histogram.iter().sum::<u64>(), 6);
        assert_eq!(r.mapping[1].field, "completion");
    }

    #[test]
    fn folder_snapshot_is_normalized_and_parquet_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "a.jsonl",
            "{\"instruction\":\"a\",\"output\":\"b\"}\n{\"instruction\":\"c\",\"output\":\"d\"}\n",
        );
        write(dir.path(), "b.csv", "instruction,output\ne,f\n");
        write(dir.path(), "notes.txt", "ignored");
        let r = inspect(dir.path(), 0.1);
        assert!(r.ok, "{:?}", r.issues);
        assert_eq!(r.rows, 3);
        let out = dir.path().join("snap.out");
        assert_eq!(write_snapshot(dir.path(), &out).unwrap(), 3);
        let text = std::fs::read_to_string(&out).unwrap();
        let first: Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
        assert_eq!(first["messages"][1]["content"], "b");
        let empty = tempfile::tempdir().unwrap();
        assert!(inspect(empty.path(), 0.1)
            .issues
            .iter()
            .any(|i| i.key == "training.dataset_folder_empty"));
        let pq = write(empty.path(), "x.parquet", "PAR1");
        assert!(inspect(&pq, 0.1)
            .issues
            .iter()
            .any(|i| i.key == "training.dataset_parquet_unsupported"));
    }

    #[test]
    fn json_array_and_histogram_buckets() {
        let dir = tempfile::tempdir().unwrap();
        let long = "x".repeat(400);
        let body = json!([
            {"prompt": "q", "completion": "a"},
            {"prompt": "q", "completion": long},
            {"prompt": "q", "completion": "a"}
        ]);
        let p = write(dir.path(), "d.json", &body.to_string());
        let r = inspect(&p, 0.1);
        assert!(r.ok, "{:?}", r.issues);
        assert_eq!(r.histogram[0], 2);
        let bucket = HISTOGRAM_EDGES.iter().position(|e| *e >= 101).unwrap();
        assert_eq!(r.histogram[bucket], 1);
    }
}
