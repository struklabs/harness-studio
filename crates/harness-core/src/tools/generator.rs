//! The tool generator: the user describes a tool and the hosts and folders
//! it may reach, the model they chat with writes the package, Harness tests it in the sandbox,
//! and the user decides. Nothing here starts by itself.
//!
//! 1. [`generate`] asks the chat model (one real chat completion per attempt) for a manifest JSON block
//!    followed by labeled JavaScript blocks (legacy single-JSON replies remain supported). A reply that does not parse, does not validate
//!    ([`package::parse_manifest`]), asks for more than the user allowed or fails its own test
//!    goes back to the model with the error, up to [`MAX_RETRIES`] times (the per-tool retry
//!    limit). The result is a [`Draft`] held in memory with the test report.
//! 2. [`approve`] (the user read the code, the permissions and the test) registers the draft in
//!    the [`ToolRegistry`] with source `generated`: same class, policy, approval and tool log as
//!    every other tool, gone when the app closes. At most [`MAX_GENERATED`] generated tools are
//!    registered at a time (the per-session generated tool limit).
//! 3. [`save`] writes it to `<data dir>/tools/<name>/`; from then on it is an installed tool.
//!
//! The permissions the user typed are an upper bound: a manifest that asks for a host, a folder
//! or a write the user did not list is refused, never widened ([`within_bounds`]). The model
//! cannot add `shell` either (the package format refuses it).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::installed::{self, InstalledTool};
use super::package::{self, Manifest, Permissions, MAX_FILE_BYTES};
use super::package_test::{self, TestReport};
use super::registry::ToolRegistry;
use super::types::ToolSource;
use crate::compaction::Endpoint;
use crate::error_key::err;

/// Tools generated and approved in one app session.
pub const MAX_GENERATED: usize = 5;
/// Fixes the model gets per tool after the first attempt.
pub const MAX_RETRIES: usize = 4;
/// Drafts waiting for the user's decision.
const MAX_DRAFTS: usize = 8;
const MAX_DESCRIPTION: usize = 2000;
const MAX_REPLY_TOKENS: u64 = 8192;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(600);

/// Unknown context: conservative 16K, never truncate the original request.
const DEFAULT_CONTEXT: u64 = 16_384;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub estimated: bool,
}
impl TokenUsage {
    pub fn add(&mut self, other: &Self) {
        self.prompt_tokens += other.prompt_tokens;
        self.completion_tokens += other.completion_tokens;
        self.estimated |= other.estimated;
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Attempt {
    pub number: u32,
    pub change: String,
    pub failed_cases: usize,
    pub total_cases: usize,
    pub feedback: String,
    pub failure_summary: String,
    pub usage: TokenUsage,
    pub repeated: bool,
    pub strategy: String,
    pub compacted: bool,
}

pub(crate) struct Reply {
    pub text: String,
    pub usage: TokenUsage,
    pub truncated: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TokenEstimate {
    pub low: u64,
    pub high: u64,
    pub prompt_per_tool: u64,
    pub reply_per_tool: u64,
    pub historical: bool,
}

/// Includes ideation once; 1.25–3.5 calls/tool allow for retries and growing feedback.
pub async fn estimate(
    req: &GenerateRequest,
    count: usize,
    ideation: &str,
    history: &[Attempt],
    cancel: &AtomicBool,
) -> Result<TokenEstimate, String> {
    let (prompt, _) = count_messages(
        &req.endpoint,
        &[
            json!({"content":SYSTEM_PROMPT}),
            json!({"content":user_prompt(req)}),
        ],
        cancel,
    )
    .await?;
    let measured: Vec<_> = history
        .iter()
        .filter(|a| !a.usage.estimated && a.usage.completion_tokens > 0)
        .collect();
    let reply = if measured.is_empty() {
        2_000
    } else {
        measured
            .iter()
            .map(|a| a.usage.completion_tokens)
            .sum::<u64>()
            / measured.len() as u64
    };
    let ideas = if ideation.is_empty() {
        0
    } else {
        count_messages(&req.endpoint, &[json!({"content":ideation})], cancel)
            .await?
            .0
            + count as u64 * 180
    };
    Ok(TokenEstimate {
        low: ideas + (prompt + reply) * count as u64 * 5 / 4,
        high: ideas + (prompt + reply) * count as u64 * 7 / 2,
        prompt_per_tool: prompt,
        reply_per_tool: reply,
        historical: !measured.is_empty(),
    })
}

fn change_note(reply: &str) -> String {
    strip_think(reply)
        .trim()
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("CHANGE:"))
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn normalized_code(code: &str) -> String {
    code.trim().to_string()
}

fn failed_signature(report: &TestReport) -> String {
    let mut cases: Vec<_> = report
        .cases
        .iter()
        .filter(|c| !c.passed)
        .map(|c| (&c.name, &c.actual))
        .collect();
    cases.sort();
    if cases.is_empty() && report.error.is_none() {
        String::new()
    } else {
        serde_json::to_string(&(cases, &report.error)).unwrap_or_default()
    }
}

fn compact_messages(messages: &mut Vec<Value>, history: &[Attempt]) -> bool {
    if messages.len() <= 4 {
        return false;
    }
    let latest = messages.iter().rposition(|m| m["role"] == "assistant");
    let Some(latest) = latest.filter(|i| *i > 2) else {
        return false;
    };
    let summary = history
        .iter()
        .take(history.len().saturating_sub(1))
        .map(|a| {
            format!(
                "Attempt {}: {}; failed: {}",
                a.number,
                a.change.chars().take(180).collect::<String>(),
                if a.failure_summary.is_empty() {
                    a.feedback.chars().take(240).collect::<String>()
                } else {
                    a.failure_summary.clone()
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    messages.splice(
        2..latest,
        [json!({"role":"user","content":format!("Earlier attempts (summary):\n{summary}")})],
    );
    true
}

async fn context_window(ep: &Endpoint, cancel: &AtomicBool) -> Result<u64, String> {
    let root = ep.base_url.trim_end_matches('/').trim_end_matches("/v1");
    let mut request = http()
        .get(format!("{root}/props"))
        .timeout(Duration::from_secs(5));
    for (k, v) in &ep.headers {
        request = request.header(k, v);
    }
    tokio::select! {
        result = async { request.send().await.ok()?.json::<Value>().await.ok() } => {
            Ok(result.and_then(|v| v["default_generation_settings"]["n_ctx"].as_u64().or_else(|| v["n_ctx"].as_u64())).filter(|n| *n > 0).unwrap_or(DEFAULT_CONTEXT))
        },
        _ = wait_cancel(cancel) => Err(err("tools.generator.cancelled").into()),
    }
}

async fn count_messages(
    ep: &Endpoint,
    messages: &[Value],
    cancel: &AtomicBool,
) -> Result<(u64, bool), String> {
    let tokenize = crate::compaction::TokenizeEndpoint {
        host_url: ep
            .base_url
            .trim_end_matches('/')
            .trim_end_matches("/v1")
            .into(),
        headers: ep.headers.clone(),
    };
    let mut total = 0;
    let mut exact = true;
    for m in messages {
        let (n, measured) = crate::compaction::count_text(
            ep,
            Some(&tokenize),
            m["content"].as_str().unwrap_or_default(),
            cancel,
        )
        .await?;
        total += n;
        exact &= measured;
    }
    Ok((total, exact))
}

/// Below this many parameters a model rarely writes a tool that works; the dialog warns.
pub const SMALL_MODEL_PARAMS: u64 = 7_000_000_000;

/// Parameter count of the GGUF at `path` (the tensor table, else its `size_label`); `None`
/// when the file cannot be read or says neither.
pub fn model_params(path: &std::path::Path) -> Option<u64> {
    let meta = crate::gguf::read_gguf_metadata(path).ok()?;
    meta.param_count.or_else(|| {
        meta.size_label
            .as_deref()
            .and_then(crate::library::params::parse_size_label)
    })
}

#[derive(Debug, Clone, Deserialize)]
pub struct GenerateRequest {
    /// What the tool should do, in the user's words.
    pub description: String,
    /// The hosts, readable folders and writable folders the user allows: the upper bound.
    #[serde(default)]
    pub network: Vec<String>,
    #[serde(default)]
    pub fs_read: Vec<String>,
    #[serde(default)]
    pub fs_write: Vec<String>,
    /// The chat model's endpoint (the same one the Playground sends its messages to).
    pub endpoint: Endpoint,
}

impl GenerateRequest {
    fn allowed(&self) -> Permissions {
        Permissions {
            network: self.network.clone(),
            fs: package::FsScope {
                read: self.fs_read.clone(),
                write: self.fs_write.clone(),
            },
            shell: false,
        }
    }
}

/// A generated package waiting for the user: everything the approval dialog shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Draft {
    pub id: String,
    /// Name, description and permissions as an installed tool lists them.
    pub info: InstalledTool,
    pub manifest_json: String,
    pub code: String,
    pub test: String,
    pub report: TestReport,
    /// Chat completions it took (1 = the first reply was right).
    pub attempts: u32,
    #[serde(default)]
    pub history: Vec<Attempt>,
    #[serde(default)]
    pub usage: TokenUsage,
    /// Set when the code changed between attempts but a case kept producing the same wrong-looking
    /// value: the test's expectation is probably what is wrong, not the code.
    #[serde(default)]
    pub suspect: Option<SuspectCase>,
}

/// A failing case whose actual output stayed the same while the code changed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SuspectCase {
    pub case: String,
    pub expected: String,
    pub actual: String,
}

/// Value mismatches (not crashes or malformed cases): the only failures where code and
/// expectation can disagree about a result.
fn is_value_mismatch(reason: &str) -> bool {
    matches!(reason, "equals" | "contains" | "matches" | "length")
}

/// The (case, actual) pairs of the value mismatches in a report.
fn mismatch_outputs(report: &TestReport) -> Vec<(String, String)> {
    report
        .cases
        .iter()
        .filter(|c| !c.passed && is_value_mismatch(&c.reason))
        .map(|c| (c.name.clone(), c.actual.clone()))
        .collect()
}

/// Code/test contradiction: a case that failed in the previous attempt returns exactly the same
/// value now although the code changed, so the code is stable and the expectation is suspect.
fn detect_suspect(
    previous: &[(String, String)],
    report: &TestReport,
    code_changed: bool,
) -> Option<SuspectCase> {
    if !code_changed {
        return None;
    }
    report
        .cases
        .iter()
        .filter(|c| !c.passed && is_value_mismatch(&c.reason))
        .find(|c| previous.iter().any(|(n, a)| *n == c.name && *a == c.actual))
        .map(|c| SuspectCase {
            case: c.name.clone(),
            expected: c.expected.clone(),
            actual: c.actual.clone(),
        })
}

/// Expected and actual values side by side, for the retry message.
fn side_by_side(report: &TestReport) -> String {
    report
        .cases
        .iter()
        .filter(|c| !c.passed && is_value_mismatch(&c.reason))
        .map(|c| {
            format!(
                "- case {:?}: test expects {:?} | code returned {:?}",
                c.name, c.expected, c.actual
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Appended to every test-failure retry: either side may be the wrong one.
const EITHER_WRONG: &str = "\nBoth the code AND the test expectation may be wrong. For each failing case, compute the correct result for its input by hand, step by step, then decide which side is wrong. If the test expectation is wrong, fix test.js (keep the case, correct its expected value, add a one-line comment why) and leave index.js alone; if the code is wrong, fix index.js. Never delete cases or weaken valid expectations.";

/// The files of a package, checked.
#[derive(Debug, Clone)]
struct Files {
    manifest: Manifest,
    manifest_json: String,
    code: String,
    test: String,
}

fn processes_text(description: &str, files: &Files) -> bool {
    let description = description.to_lowercase();
    files.manifest.category == "text"
        || [
            "text", "word", "string", "metin", "kelime", "markdown", "csv", "html",
        ]
        .iter()
        .any(|word| description.contains(word))
        || [".replace(", ".split(", ".match("]
            .iter()
            .any(|operation| files.code.contains(operation))
}

/// The tool takes text: a string anywhere in its input schema, or files to read.
fn accepts_text(files: &Files) -> bool {
    fn has_string(v: &Value) -> bool {
        match v {
            Value::Object(map) => {
                map.get("type").is_some_and(|t| {
                    t == "string"
                        || t.as_array()
                            .is_some_and(|a| a.iter().any(|x| x == "string"))
                }) || map.values().any(has_string)
            }
            Value::Array(items) => items.iter().any(has_string),
            _ => false,
        }
    }
    has_string(&files.manifest.input_schema) || !files.manifest.permissions.fs.read.is_empty()
}

/// Review actual executed input cases, so a Unicode name/comment/expectation cannot bypass it.
fn review_unicode(description: &str, files: &Files, report: &mut TestReport) {
    let text = processes_text(description, files);
    // A non-English description asks for non-ASCII input only where the tool can take
    // some: a string argument or files (S38: an integer-only `is_prime` was refused 5x).
    let required = text || (!description.is_ascii() && accepts_text(files));
    if required
        && report.passed
        && !report
            .cases
            .iter()
            .any(|case| case.unicode_input && case.passed)
    {
        report.passed = false;
        report.error = Some("Unicode coverage missing: add at least one successful case with non-ASCII letters in args or file content, in the user's description language (e.g. Turkish: bir iki iki üç üç üç). Preserve existing cases and fix the implementation if needed.".into());
    }
    if text
        && [r"\w", "[a-zA-Z]", "[A-Za-z]", "[a-z]", "[A-Z]"]
            .iter()
            .any(|class| files.code.contains(class))
    {
        // Treat as review feedback, not a blanket ban: ASCII identifiers/protocols are valid.
        report.warnings.push(r"ASCII-only regex classes detected. For natural-language text use \p{L}, \p{N} and the u flag; preserve Unicode letters.".into());
        if let Some(error) = &mut report.error {
            error.push_str(r" Review warning: ASCII-only regex classes detected. For natural-language text use \p{L}, \p{N} and the u flag; preserve Unicode letters.");
        }
    }
}

// ---- Permission upper bound ----

fn host_covered(allowed: &[String], wanted: &str) -> bool {
    if allowed == ["*"] {
        return true;
    }
    if wanted == "*" {
        return false;
    }
    let (wild, host) = match wanted.strip_prefix("*.") {
        Some(h) => (true, h),
        None => (false, wanted),
    };
    allowed.iter().any(|a| match a.strip_prefix("*.") {
        // `*.x.com` admits `a.x.com`, `*.a.x.com` and itself, not `x.com`.
        Some(suffix) => (wild && host == suffix) || host.ends_with(&format!(".{suffix}")),
        None => !wild && host == a,
    })
}

fn folder_parts(p: &str) -> Vec<&str> {
    p.split('/')
        .filter(|s| !s.is_empty() && *s != ".")
        .collect()
}

/// `wanted` is `allowed` itself or inside it (`.` is the whole workspace).
fn folder_covered(allowed: &[String], wanted: &str) -> bool {
    let want = folder_parts(wanted);
    allowed.iter().any(|a| want.starts_with(&folder_parts(a)))
}

/// Refuses a manifest that asks for more than the user allowed. Reads must be inside an allowed
/// read folder, writes inside an allowed write folder (a write grant does not imply a read).
pub fn within_bounds(asked: &Permissions, allowed: &Permissions) -> Result<(), String> {
    let over = |what: &str, item: &str| -> String {
        err("tools.generator.permissions_exceeded")
            .with("kind", what)
            .with("item", item)
            .into()
    };
    if let Some(h) = asked
        .network
        .iter()
        .find(|h| !host_covered(&allowed.network, h))
    {
        return Err(over("network", h));
    }
    if let Some(p) = asked
        .fs
        .read
        .iter()
        .find(|p| !folder_covered(&allowed.fs.read, p))
    {
        return Err(over("read", p));
    }
    if let Some(p) = asked
        .fs
        .write
        .iter()
        .find(|p| !folder_covered(&allowed.fs.write, p))
    {
        return Err(over("write", p));
    }
    Ok(())
}

// ---- The prompt and the reply ----

const SYSTEM_PROMPT: &str = r#"You write small tools for Harness Studio. A tool is JavaScript that runs in a sandbox (QuickJS). Reply with exactly three complete fenced blocks: first ```json containing ONLY the manifest object, then ```js index.js containing raw JavaScript, then ```js test.js containing raw JavaScript. Alternatively put index.js: / test.js: headings immediately before the corresponding ```js blocks. Do not JSON-escape JavaScript or include other prose. Use this exact layout, filling in all manifest fields and code:
```json
{}
```
index.js:
```js
function run(args) { /* implementation */ }
```
test.js:
```js
const cases = [ /* two to five meaningful cases */ ];
```
The index.js: and test.js: label lines are REQUIRED when using plain js fences.
If JavaScript contains a line of ``` (for example inside a template literal), wrap that file in ````js fences and close with ````. The closing fence must be at least as long as the opening fence.

manifest fields: category (file, web, calculation, datetime, text or other; default other), name (lowercase letters, digits and underscores, starts with a letter, 2-48 characters), version ("0.1.0"), description (one or two sentences: what the tool does and when to call it), input_schema (JSON Schema with "type": "object", "properties" and "required"), output (what the result text contains), permissions {"network": [host names], "fs": {"read": [folders], "write": [folders]}}. For network, use [] for no access, ["*"] for the entire public internet, or domain names for listed domains only; never combine "*" with other entries. Ask only for what the tool needs and never for more than the user allows (given below). Do not write entry, test, runtime or shell.

index_js defines `function run(args)` and returns a string (or a JSON-serializable value). Available globals include btoa, atob (Latin-1 binary strings), TextEncoder/TextDecoder (UTF-8 encode/decode), JSON, Math, Date and RegExp. For Unicode, convert TextEncoder's UTF-8 bytes to a binary string with String.fromCharCode before btoa. TextDecoder supports UTF-8 only, no streaming. There is no require, import, fetch, process, DOMParser, document or timers. The only way out of the sandbox is the global `harness`:
- harness.http.get(url) and harness.http.post(url, jsonBody) return {status, content_type, url, body}. They are synchronous and only work for hosts allowed by permissions.network. ["*"] permits any public host; [] denies all network. Private/local addresses and Harness ports are always blocked.
- harness.fs.read(path), harness.fs.write(path, text), harness.fs.list(path): paths are relative to the workspace folder and only work inside the folders listed in permissions.fs.
For text/data transformations (CSV/JSON, HTML-to-text, Markdown, base64), take content strings by default and return the transformed content; do not invent input/output paths or filesystem permissions unless the user explicitly requests file operations. For multiline markup, parse lines/blocks before inline replacements: distinguish heading levels, handle the last line without a trailing newline, and never wrap heading blocks in paragraph tags. Do not use unanchored overlapping heading replacements.
For natural-language text processing use Unicode-aware regex: \p{L} for letters, \p{N} for numbers and the u flag (for example /[^\p{L}\p{N}\s]/gu). Never discard non-ASCII letters with \w or [a-zA-Z]. If the user description contains non-ASCII characters OR the tool processes text, include at least one successful non-ASCII-letter input case in the description's language, with meaningful expected output (Turkish example: bir iki iki üç üç üç must count six words, including üç three times). A Unicode case name, comment or expectation alone does not count.
Throw an Error with a clear message for bad input. Keep code concise. In raw JavaScript: literal newlines cannot appear inside single/double quoted JS strings; use escaped \n or template literals.

test_js declares `const cases = [ ... ];` that run against index_js. Each case is {"name": "...", "args": {...}, "expect": {...}} where expect has exactly one of: "equals": "exact result text", "contains": "text" (or ["text", ...]), "matches": a JavaScript RegExp literal, pattern string, or "/pattern/flags" string (anchor with ^ and $ for a full match), "length": non-negative character count, "error": true (or "text the error message contains"). equals compares both sides after trimming trailing whitespace only; leading and internal whitespace must match exactly. Strings are compared as plain text; other values use JSON text; undefined is normalized to null. For random passwords, IDs or time-dependent results, check deterministic properties with matches/length, never exact random output or contains of an entire alphabet. Prefer regex literals to avoid string escapes: expect: { matches: /^[A-Za-z0-9]{12}$/ }. JavaScript RegExp syntax and flags apply, including lookahead. Validate zero separately from missing input (use ?? rather than || for defaults), and consider acronym boundaries in case conversion. Check that every expectation agrees with the stated contract; ISO dates without Z can be valid JavaScript dates. Write two to five cases that check the logic on given input and the error on bad input.

FILE TESTS: Every case runs in its own empty temporary workspace. For file input include fs: [{path: "input.csv", content: "name,age\nAlice,30"}] on that case. The runner writes these UTF-8 fixtures before run(args); manifest read/write permissions still apply. Paths must be relative with no . or .. segments; at most 32 files and 1 MiB total content per case. Files and writes never leak to another case. Use fs: [] for missing-file cases. There are no setup callbacks and HTTP mocks (including file:// URLs) cannot create files.
Example: {name: "read", args: {path: "input.txt"}, fs: [{path: "input.txt", content: "Hello"}], expect: {equals: "Hello"}}.

CRITICAL: Test cases MUST mock ALL network calls. If the tool uses harness.http, the case must include an `http` array of mocks. A mocked case intercepts harness.http calls matching the method and exact `url` or `url_prefix` and answers from the mock. Unmatched requests in a mocked case fail. The mock body can be a string or a JSON object.
Example: { name: "ok", args: {q: "x"}, http: [{ url_prefix: "https://api.example.com/", status: 200, body: {results: []} }], expect: { equals: "0" } }"#;

fn user_prompt(req: &GenerateRequest) -> String {
    let list = |v: &[String]| {
        if v.is_empty() {
            "none".to_string()
        } else {
            v.join(", ")
        }
    };
    format!(
        "Tool wanted: {}\n\nThe user allows at most: network hosts: {}; readable workspace folders: {}; writable workspace folders: {}.",
        req.description.trim(),
        list(&req.network),
        list(&req.fs_read),
        list(&req.fs_write),
    )
}

/// The unique JSON object carrying the expected keys, ignoring examples in prose/fences.
pub(crate) fn json_object(reply: &str, expected_keys: &[&str]) -> Option<Value> {
    json_object_with_repair(reply, expected_keys, false)
}

fn json_object_with_repair(reply: &str, expected_keys: &[&str], repair: bool) -> Option<Value> {
    let text = strip_think(reply);
    let text = text.trim();
    let matches = |value: &Value| {
        value.is_object() && expected_keys.iter().all(|key| value.get(*key).is_some())
    };
    if let Ok(value) = serde_json::from_str::<Value>(text) {
        return matches(&value).then_some(value);
    }
    // Deserialize one complete object at a time: braces inside JSON strings are handled
    // by serde, and trailing prose/fences need not be part of the JSON document.
    let mut candidate = None;
    let mut offset = 0;
    while let Some(start) = text[offset..].find('{') {
        let start = offset + start;
        // Reset quote state for each candidate, never including preceding model prose.
        let repaired;
        let candidate_len = json_candidate_len(&text[start..]);
        let source = if repair {
            repaired = repair_json_escapes(&text[start..start + candidate_len]);
            repaired.as_str()
        } else {
            &text[start..]
        };
        let mut stream = serde_json::Deserializer::from_str(source).into_iter::<Value>();
        if let Some(Ok(value)) = stream.next() {
            // Repair may add bytes, so its byte offset is not an offset in the original.
            offset = start
                + if repair {
                    candidate_len
                } else {
                    stream.byte_offset()
                };
            if matches(&value) {
                if candidate.is_some() {
                    return None;
                }
                candidate = Some(value);
            }
        } else {
            offset = start + 1;
        }
    }
    candidate
}

/// Find the end of a candidate before repair inserts bytes. Escaped quotes and braces
/// inside JSON strings cannot end the object; a truncated candidate extends to EOF.
fn json_candidate_len(text: &str) -> usize {
    let mut depth = 0;
    let mut quoted = false;
    let mut chars = text.char_indices();
    while let Some((i, ch)) = chars.next() {
        match ch {
            '\\' if quoted => {
                chars.next();
            }
            '"' => quoted = !quoted,
            '{' if !quoted => depth += 1,
            '}' if !quoted => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
    }
    text.len()
}

fn strip_think(text: &str) -> String {
    let mut out = text.to_string();
    while let (Some(a), Some(b)) = (out.find("<think>"), out.find("</think>")) {
        if b < a {
            break;
        }
        out.replace_range(a..b + "</think>".len(), "");
    }
    out
}

fn bad_reply(detail: &str) -> String {
    err("tools.generator.reply_invalid")
        .with("detail", detail)
        .into()
}

fn field<'a>(obj: &'a Value, names: &[&str]) -> Option<&'a Value> {
    names.iter().find_map(|n| obj.get(*n))
}

fn file_text(obj: &Value, names: &[&str], what: &str) -> Result<String, String> {
    let text = field(obj, names)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| bad_reply(&format!("{what} is missing or not a string")))?;
    if text.trim().is_empty() {
        return Err(bad_reply(&format!("{what} is empty")));
    }
    if text.len() as u64 > MAX_FILE_BYTES {
        return Err(err("tools.package.file_too_large")
            .with("detail", what)
            .into());
    }
    Ok(text)
}

/// Repairs only invalid JSON escapes within string literals, leaving valid escapes intact.
fn repair_json_escapes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut quoted = false;
    while let Some(ch) = chars.next() {
        if ch == '\"' {
            quoted = !quoted;
        }
        out.push(ch);
        if quoted && ch == '\\' {
            if let Some(next) = chars.next() {
                let valid_unicode = next == 'u'
                    && chars
                        .clone()
                        .take(4)
                        .filter(|c| c.is_ascii_hexdigit())
                        .count()
                        == 4;
                if !(matches!(next, '\"' | '\\' | '/' | 'b' | 'f' | 'n' | 'r' | 't')
                    || valid_unicode)
                {
                    out.push('\\');
                }
                out.push(next);
            }
        }
    }
    out
}

fn legacy_reply(reply: &str) -> Option<Value> {
    let keys = &["manifest", "index_js", "test_js"];
    if let Some(root) = json_object(reply, keys) {
        return Some(root);
    }
    // Only retry after serde specifically reports an invalid escape. Do not repair
    // syntax, truncation or valid escapes, and retain unique-object extraction.
    let text = strip_think(reply);
    let invalid_escape = text.match_indices('{').any(|(start, _)| {
        let mut stream = serde_json::Deserializer::from_str(&text[start..]).into_iter::<Value>();
        matches!(stream.next(), Some(Err(e)) if e.to_string().contains("invalid escape"))
    });
    invalid_escape
        .then(|| json_object_with_repair(&text, keys, true))
        .flatten()
}

fn fenced_reply(reply: &str) -> Result<Option<Value>, String> {
    let text = strip_think(reply);
    let mut manifest = None;
    let mut code = None;
    let mut test = None;
    let mut lines = text.lines();
    let mut heading = "";
    while let Some(line) = lines.next() {
        let line = line.trim();
        let fence_len = line.chars().take_while(|ch| *ch == '`').count();
        if fence_len < 3 {
            heading = line;
            continue;
        }
        let info = line[fence_len..].trim();
        let label = if info == "json" {
            "manifest"
        } else if info == "js index.js"
            || info == "javascript index.js"
            || ((info == "js" || info == "javascript") && heading == "index.js:")
        {
            "index"
        } else if info == "js test.js"
            || info == "javascript test.js"
            || ((info == "js" || info == "javascript") && heading == "test.js:")
        {
            "test"
        } else {
            "other"
        };
        let mut body = Vec::new();
        let mut closed = false;
        for line in lines.by_ref() {
            let trimmed = line.trim();
            if trimmed.len() >= fence_len && trimmed.chars().all(|ch| ch == '`') {
                closed = true;
                break;
            }
            body.push(line);
        }
        if !closed {
            return Err(err("create.model_reply_truncated").into());
        }
        let body = body.join("\n");
        let duplicate = match label {
            "manifest" => {
                // A fenced legacy package still goes through legacy parsing.
                if let Ok(value) = serde_json::from_str::<Value>(&body) {
                    if value.get("index_js").is_some() {
                        continue;
                    }
                    manifest.replace(value).is_some()
                } else if !body.contains("\"index_js\"") {
                    return Err(bad_reply("invalid manifest JSON block"));
                } else {
                    false
                }
            }
            "index" => code.replace(body).is_some(),
            "test" => test.replace(body).is_some(),
            _ => false,
        };
        if duplicate {
            return Err(bad_reply(&format!("duplicate {label} block")));
        }
        heading = "";
    }
    if manifest.is_none() && code.is_none() && test.is_none() {
        return Ok(None);
    }
    match (manifest, code, test) {
        (Some(manifest), Some(code), Some(test)) => Ok(Some(
            json!({"manifest": manifest, "index_js": code, "test_js": test}),
        )),
        _ => Err(bad_reply("missing labeled blocks: use a manifest-only json block, then index.js: followed by a js block, then test.js: followed by a js block. Unlabeled js blocks are not accepted")),
    }
}

/// Turns a model reply into checked files: the validated manifest, permission upper bound
/// and the two code files.
fn parse_reply(reply: &str, allowed: &Permissions) -> Result<Files, String> {
    let text = strip_think(reply);
    let text = text.trim();
    let reply = if text.starts_with("CHANGE:") {
        text.split_once('\n').map_or("", |(_, rest)| rest)
    } else {
        text
    };
    let root = match fenced_reply(reply)? {
        Some(root) => Some(root),
        None => legacy_reply(reply),
    }
    .ok_or_else(|| bad_reply("the reply is not a JSON object"))?;
    let mut manifest = match field(&root, &["manifest"]) {
        Some(Value::Object(o)) => Value::Object(o.clone()),
        Some(Value::String(s)) => {
            serde_json::from_str(s).map_err(|e| bad_reply(&format!("manifest: {e}")))?
        }
        _ => return Err(bad_reply("manifest is missing")),
    };
    let obj = manifest
        .as_object_mut()
        .ok_or_else(|| bad_reply("manifest must be an object"))?;
    obj.insert("runtime".into(), json!("js"));
    obj.insert("entry".into(), json!("index.js"));
    obj.insert("test".into(), json!("test.js"));
    obj.entry("version").or_insert_with(|| json!("0.1.0"));
    // A model that writes `"shell": null` or omits the block is not asking for anything.
    if let Some(p) = obj.get_mut("permissions").and_then(Value::as_object_mut) {
        p.retain(|_, v| !v.is_null());
    }
    if let Some(network) = manifest.pointer_mut("/permissions/network") {
        let mut hosts: Vec<String> = serde_json::from_value(network.clone())
            .map_err(|_| bad_reply("permissions.network must be a list of strings"))?;
        package::normalize_model_network(&mut hosts)?;
        *network = json!(hosts);
    }
    let parsed = package::parse_manifest(&manifest.to_string())?;
    within_bounds(&parsed.permissions, allowed)?;
    let code = file_text(&root, &["index_js", "index.js", "code"], "index_js")?;
    let test = file_text(&root, &["test_js", "test.js", "test"], "test_js")?;
    Ok(Files {
        manifest_json: serde_json::to_string_pretty(&parsed).unwrap_or_default(),
        manifest: parsed,
        code,
        test,
    })
}

/// A test review needs a subsequent code repair slot.
fn repeated_failure_strategy(attempt: u32) -> &'static str {
    if (1 + MAX_RETRIES as u32).saturating_sub(attempt) >= 2 {
        "test_first"
    } else {
        "rewrite"
    }
}

/// The text sent back to the model when its reply failed.
fn retry_message(problem: &str) -> String {
    format!(
        "That did not work: {problem}\n\nStart your reply with exactly one line CHANGE: <what you change and why>. Then the complete package. For bad_case, fix the test (pattern, flags or expectation shape). Otherwise inspect both implementation and expectations: fix code for valid expectations. If a test contradicts reasonable correct behavior or the requested contract, correct that test and add a one-line JS comment naming the case and explaining why. Never weaken valid expectations, delete cases, or reduce the case count below two. Escape newlines inside quoted JS strings or use template literals. Reply with the complete manifest-only ```json block, then ```js index.js and ```js test.js blocks; raw JavaScript, no JSON string escaping."
    )
}

// ---- The model request ----

fn http() -> reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .timeout(REQUEST_TIMEOUT)
                .no_proxy()
                .build()
                .unwrap_or_default()
        })
        .clone()
}

async fn wait_cancel(flag: &AtomicBool) {
    while !flag.load(Ordering::SeqCst) {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// One non-streaming chat completion; the reply text.
pub(crate) async fn ask(
    ep: &Endpoint,
    messages: &[Value],
    cancel: &AtomicBool,
    client: &str,
) -> Result<Reply, String> {
    let (prompt_tokens, _) = count_messages(ep, messages, cancel).await?;
    let url = format!("{}/chat/completions", ep.base_url.trim_end_matches('/'));
    let mut body = json!({
        "messages": messages,
        "stream": false,
        "max_tokens": MAX_REPLY_TOKENS,
        "temperature": 0.2,
        // Reasoning models would spend the budget on thinking.
        "chat_template_kwargs": { "enable_thinking": false },
    });
    if let Some(m) = ep.model.as_deref().filter(|m| !m.is_empty()) {
        body["model"] = json!(m);
    }
    let failed = |e: String| -> String {
        if let Some((request, context)) = context_exceeded(&e) {
            return err("tools.generator.context_exceeded")
                .with("request", request)
                .with("context", context)
                .into();
        }
        err("tools.generator.request_failed")
            .with("url", &url)
            .with("error", e)
            .into()
    };
    let mut req = http().post(&url).json(&body);
    for (k, v) in &ep.headers {
        if !k.eq_ignore_ascii_case("x-harness-client") {
            req = req.header(k, v);
        }
    }
    req = req.header("x-harness-client", client);
    let send = async {
        let res = req
            .send()
            .await
            .map_err(|e| crate::http_error::transport(&e, &url))?;
        let via_gateway = res.headers().contains_key("x-harness-request-id");
        let status = res.status();
        let text = res
            .text()
            .await
            .map_err(|e| crate::http_error::transport(&e, &url))?;
        if !status.is_success() {
            let detail: String = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
                .unwrap_or_else(|| text.chars().take(300).collect());
            return Err(failed(format!("HTTP {}: {detail}", status.as_u16())));
        }
        let v: Value = serde_json::from_str(&text).map_err(|e| failed(e.to_string()))?;
        let truncated = v["choices"][0]["finish_reason"] == "length";
        let text = v["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let input = v["usage"]["prompt_tokens"].as_u64();
        let output = v["usage"]["completion_tokens"].as_u64();
        let completion_tokens = if let Some(n) = output {
            n
        } else {
            count_messages(ep, &[json!({"content":text})], cancel)
                .await?
                .0
        };
        if let (false, Some(prompt), Some(completion)) = (via_gateway, input, output) {
            let _ =
                crate::analytics::direct::record_client(&crate::analytics::direct::DirectRecord {
                    endpoint: ep.base_url.clone(),
                    client: client.into(),
                    model: ep.model.clone().unwrap_or_default(),
                    prompt_tokens: prompt,
                    completion_tokens: completion,
                    cached_tokens: v["usage"]["prompt_tokens_details"]["cached_tokens"]
                        .as_u64()
                        .unwrap_or(0),
                });
        }
        Ok(Reply {
            text,
            truncated,
            usage: TokenUsage {
                prompt_tokens: input.unwrap_or(prompt_tokens),
                completion_tokens,
                estimated: input.is_none() || output.is_none(),
            },
        })
    };
    tokio::select! {
        r = send => r,
        _ = wait_cancel(cancel) => Err(err("tools.generator.cancelled").into()),
    }
}

// ---- Drafts and the session's generated tools ----

struct Stored {
    manifest: Manifest,
    manifest_json: String,
    code: String,
    test: String,
}

#[derive(Default)]
struct Store {
    /// Waiting for the user, oldest first.
    drafts: Vec<(String, Stored, TestReport)>,
    /// Registered this session, by tool name (what [`save`] writes).
    session: HashMap<String, Stored>,
}

fn store() -> std::sync::MutexGuard<'static, Store> {
    static STORE: OnceLock<Mutex<Store>> = OnceLock::new();
    STORE
        .get_or_init(|| Mutex::new(Store::default()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

fn generated_names(registry: &ToolRegistry) -> Vec<String> {
    registry
        .list()
        .into_iter()
        .filter(|d| d.source == ToolSource::Generated)
        .map(|d| d.name)
        .collect()
}

/// The name belongs to a built-in, MCP or installed tool: a generated tool must not shadow it.
fn name_taken(registry: &ToolRegistry, name: &str) -> bool {
    registry
        .get(name)
        .is_some_and(|t| t.def().source != ToolSource::Generated)
}

fn limit_reached(registry: &ToolRegistry, name: &str) -> bool {
    let names = generated_names(registry);
    names.len() >= MAX_GENERATED && !names.iter().any(|n| n == name)
}

/// Generated tools of this session and the limit.
pub fn session_tools(registry: &ToolRegistry) -> (Vec<String>, usize) {
    (generated_names(registry), MAX_GENERATED)
}

/// Writes the package, runs its test and keeps the result as a draft. `cancel` stops the model
/// request and sandbox test.
pub async fn generate(
    req: &GenerateRequest,
    registry: &ToolRegistry,
    cancel: &AtomicBool,
) -> Result<Draft, String> {
    generate_observed(req, registry, cancel, false, None, &|_| {}).await
}

/// Shared generator with observable attempts/stages. Autonomous tests have no external IO grants.
pub async fn generate_observed(
    req: &GenerateRequest,
    registry: &ToolRegistry,
    cancel: &AtomicBool,
    autonomous: bool,
    identity: Option<(&str, &str)>,
    progress: &(dyn Fn(&str) + Send + Sync),
) -> Result<Draft, String> {
    let description = req.description.trim();
    if description.is_empty() || description.chars().count() > MAX_DESCRIPTION {
        return Err(err("tools.generator.description_invalid")
            .num("max", MAX_DESCRIPTION as u64)
            .into());
    }
    let allowed = req.allowed();
    package::check_permissions(&allowed)?;
    if !autonomous && generated_names(registry).len() >= MAX_GENERATED {
        return Err(err("tools.generator.limit")
            .num("max", MAX_GENERATED as u64)
            .into());
    }
    let mut prompt = if autonomous {
        format!("{SYSTEM_PROMPT}\nTests must be offline: real network access is denied; filesystem tests run only in per-case temporary workspaces with fs fixtures and manifest permissions. No user files are available. Test pure logic, fixture inputs and permission-denied behavior. Use the requested name and category exactly.")
    } else {
        SYSTEM_PROMPT.into()
    };
    if let Some((name, category)) = identity {
        prompt.push_str(&format!("\nREQUIRED manifest identity: name must be {name:?}, category must be {category:?}. Do not invent or rename the tool. Use exactly these values in EVERY reply."));
    }
    let mut messages = vec![
        json!({"role": "system", "content": prompt}),
        json!({"role": "user", "content": user_prompt(req)}),
    ];
    let mut last_problem = String::new();
    let mut failed_draft: Option<(Files, TestReport, u32)> = None;
    let mut previous_code: Option<String> = None;
    let mut previous_failure = String::new();
    let mut previous_outputs: Vec<(String, String)> = Vec::new();
    let mut suspect: Option<SuspectCase> = None;
    let mut repetitions = 0;
    let mut strategy = "repair".to_string();
    let mut history = Vec::new();
    let mut usage = TokenUsage::default();
    let context = context_window(&req.endpoint, cancel).await?;
    // Keep the largest suite across all attempts; a rejected reduction must not become
    // the new baseline on the next retry.
    let mut previous_cases: Option<(String, usize)> = None;
    for attempt in 1..=(1 + MAX_RETRIES as u32) {
        progress(&format!("build:{attempt}"));
        let (tokens, _) = count_messages(&req.endpoint, &messages, cancel).await?;
        let compacted =
            tokens + MAX_REPLY_TOKENS > context && compact_messages(&mut messages, &history);
        if compacted {
            progress("context:compacted");
        }
        let reply = ask(
            &req.endpoint,
            &messages,
            cancel,
            if autonomous {
                "harness-auto-tools"
            } else {
                "harness-tool-creator"
            },
        )
        .await?;
        usage.add(&reply.usage);
        progress(&format!(
            "usage:{}",
            serde_json::to_string(&reply.usage).unwrap_or_default()
        ));
        let mut record = Attempt {
            number: attempt,
            change: change_note(&reply.text),
            usage: reply.usage,
            strategy: strategy.clone(),
            compacted,
            ..Default::default()
        };
        let reply_text = reply.text;
        if reply.truncated {
            previous_code = None;
            previous_failure.clear();
            previous_outputs.clear();
            last_problem = err("create.model_reply_truncated").into();
            record.feedback = last_problem.clone();
            progress(&format!(
                "attempt:{}",
                serde_json::to_string(&record).unwrap_or_default()
            ));
            history.push(record);
            messages.push(json!({"role":"assistant","content":reply_text}));
            messages.push(json!({"role":"user","content":retry_message("The model response reached the token limit and was incomplete. Shorten the code and tests.")}));
            continue;
        }
        let reply = reply_text;
        let checked = parse_reply(&reply, &allowed).and_then(|f| {
            if identity.is_some_and(|(name, category)| {
                f.manifest.name != name || f.manifest.category != category
            }) {
                Err(bad_reply("Use the exact requested name and category"))
            } else if name_taken(registry, &f.manifest.name) {
                Err(err("tools.package.name_taken")
                    .with("name", &f.manifest.name)
                    .into())
            } else {
                Ok(f)
            }
        });
        let mut problem = match checked {
            Ok(files) => {
                let code = normalized_code(&files.code);
                let code_repeated = previous_code.as_ref() == Some(&code);
                previous_code = Some(code);
                progress("test");
                let mut report = package_test::run_with_cancel(
                    files.code.clone(),
                    files.test.clone(),
                    files.manifest.permissions.clone(),
                    autonomous,
                    Some(cancel),
                )
                .await;
                if cancel.load(Ordering::SeqCst) {
                    return Err(err("tools.generator.cancelled").into());
                }
                if let Some((previous_test, previous_count)) = &previous_cases {
                    if report.error.is_none() && report.cases.len() < *previous_count {
                        let old = package_test::run_with_cancel(
                            files.code.clone(),
                            previous_test.clone(),
                            files.manifest.permissions.clone(),
                            autonomous,
                            Some(cancel),
                        )
                        .await;
                        report.passed = false;
                        report.error = Some(format!("Do not delete cases: case count decreased from {} to {}. Previous cases against new code: {}", previous_count, report.cases.len(), if old.passed { "all pass; restore them".into() } else { old.feedback() }));
                    }
                }
                if previous_cases
                    .as_ref()
                    .is_none_or(|(_, count)| report.cases.len() > *count)
                {
                    previous_cases = Some((files.test.clone(), report.cases.len()));
                }
                if report.error.is_none() && report.cases.len() < 2 {
                    report.passed = false;
                    report.error = Some("The test must contain at least two meaningful cases; restore or add cases.".into());
                }
                if strategy == "test_first"
                    && failed_draft.as_ref().is_some_and(|(old, _, _)| {
                        normalized_code(&old.code) != normalized_code(&files.code)
                    })
                {
                    report.passed = false;
                    report.error = Some(
                        "Test-first step: change only test.js; preserve index.js for this step."
                            .into(),
                    );
                }
                review_unicode(&req.description, &files, &mut report);
                record.total_cases = report.cases.len();
                record.failed_cases = report.cases.iter().filter(|c| !c.passed).count();
                record.failure_summary = report
                    .cases
                    .iter()
                    .filter(|c| !c.passed)
                    .map(|c| {
                        format!(
                            "{} (expected: {}; actual: {})",
                            c.name,
                            c.expected.chars().take(80).collect::<String>(),
                            c.actual.chars().take(80).collect::<String>()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                record.feedback = report.feedback();
                if report.passed {
                    progress(&format!(
                        "attempt:{}",
                        serde_json::to_string(&record).unwrap_or_default()
                    ));
                    history.push(record);
                    return Ok(keep_history(files, report, attempt, history, usage, None));
                }
                suspect = detect_suspect(&previous_outputs, &report, !code_repeated);
                previous_outputs = mismatch_outputs(&report);
                let signature = failed_signature(&report);
                let same_failure = !signature.is_empty() && previous_failure == signature;
                previous_failure = signature;
                record.repeated = code_repeated || same_failure;
                repetitions = if record.repeated { repetitions + 1 } else { 0 };
                let mut feedback = report.feedback();
                let pairs = side_by_side(&report);
                if !pairs.is_empty() {
                    feedback.push_str("\nExpected vs returned:\n");
                    feedback.push_str(&pairs);
                }
                feedback.push_str(EITHER_WRONG);
                if let Some(s) = &suspect {
                    feedback.push_str(&format!("\nSUSPECT TEST: case {:?} returned the identical value {:?} although you changed the code, so the code is stable and the test expectation {:?} is probably wrong. Recompute it by hand and correct test.js.", s.case, s.actual, s.expected));
                } else if record.repeated {
                    feedback.push_str("\nYou repeated the same code or the same failing cases with identical actual outputs; try a DIFFERENT approach. / Aynı kodu veya aynı hatayı tekrarladın; farklı bir yaklaşım dene.");
                }
                if strategy == "test_first" {
                    strategy = "rewrite".into();
                    feedback.push_str("\nTEST-FIRST step B: Now rewrite index.js FROM SCRATCH using the simplest correct approach. Preserve the reviewed test.js and all valid cases.");
                } else if repetitions >= 2 && strategy == "repair" {
                    strategy = repeated_failure_strategy(attempt).into();
                    progress(&format!("strategy:{strategy}"));
                    if strategy == "test_first" {
                        feedback.push_str("\nTEST-FIRST step A: First review and correct ONLY test.js for malformed or contradictory expectations. Keep index.js unchanged. Return the complete package. Next step will rewrite code from scratch. Never weaken valid expectations or remove cases.");
                    } else {
                        feedback.push_str("\nOnly one attempt remains: rewrite index.js FROM SCRATCH now using a different, simplest correct approach. Preserve all valid test cases and expectations; do not spend the final attempt only reviewing tests.");
                    }
                }
                if autonomous && feedback.contains("it is not in the tool's allowed hosts") {
                    feedback.push_str("\n\nNote: You must mock network calls using 'http' entries in your test cases. See the system prompt for an example.");
                }
                last_problem.clear();
                failed_draft = Some((files, report, attempt));
                format!("the test of your tool failed:\n{feedback}")
            }
            Err(e) => {
                previous_code = None;
                previous_failure.clear();
                previous_outputs.clear();
                if !crate::error_key::parse(&e)
                    .is_some_and(|(key, _)| key == "create.model_reply_truncated")
                {
                    failed_draft = None;
                }
                last_problem = e.clone();
                describe(&e)
            }
        };
        if attempt == MAX_RETRIES as u32 {
            problem.push_str("\nFor this final attempt, use the simplest correct implementation for valid expectations; fix malformed or contradictory tests with a one-line explanation. Preserve all valid expectations and cases.");
        }
        if record.feedback.is_empty() {
            record.feedback = problem.clone();
        }
        progress(&format!(
            "attempt:{}",
            serde_json::to_string(&record).unwrap_or_default()
        ));
        history.push(record);
        messages.push(json!({"role": "assistant", "content": reply}));
        messages.push(json!({"role": "user", "content": retry_message(&problem)}));
    }
    // Out of fixes: a package that only fails its test is still shown (the user sees the
    // failure and cannot approve it); one that never validated is an error.
    match failed_draft {
        Some((files, mut report, attempts)) => {
            if !last_problem.is_empty() {
                report.error = Some(last_problem);
            }
            Ok(keep_history(
                files, report, attempts, history, usage, suspect,
            ))
        }
        None => Err(err("tools.generator.failed")
            .with("detail", last_problem)
            .into()),
    }
}

/// llama-server's "request (N tokens) exceeds the available context size (M tokens)" 400 (also
/// passed through by the gateway): the two token counts, so the error can be shown translated.
pub(crate) fn context_exceeded(detail: &str) -> Option<(u64, u64)> {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(
            r"(?i)request \((\d+) tokens?\) exceeds the available context size \((\d+) tokens?\)",
        )
        .unwrap()
    });
    let c = re.captures(detail)?;
    Some((c[1].parse().ok()?, c[2].parse().ok()?))
}

/// A keyed error as the model should read it (key and parameters).
fn describe(error: &str) -> String {
    match crate::error_key::parse(error) {
        Some((key, params)) => {
            let detail: Vec<String> = params
                .iter()
                .map(|(k, v)| format!("{k}={}", v.as_str().map_or(v.to_string(), str::to_string)))
                .collect();
            format!("{key} ({})", detail.join(", "))
        }
        None => error.to_string(),
    }
}

fn keep_history(
    files: Files,
    report: TestReport,
    attempts: u32,
    history: Vec<Attempt>,
    usage: TokenUsage,
    suspect: Option<SuspectCase>,
) -> Draft {
    let attempts = history.last().map_or(attempts, |a| a.number);
    let id = uuid::Uuid::new_v4().to_string();
    let info = InstalledTool::from_manifest(&files.manifest, None, true);
    let draft = Draft {
        id: id.clone(),
        info,
        manifest_json: files.manifest_json.clone(),
        code: files.code.clone(),
        test: files.test.clone(),
        report: report.clone(),
        attempts,
        history,
        usage,
        suspect: if report.passed { None } else { suspect },
    };
    let mut s = store();
    if s.drafts.len() >= MAX_DRAFTS {
        s.drafts.remove(0);
    }
    s.drafts.push((
        id,
        Stored {
            manifest: files.manifest,
            manifest_json: files.manifest_json,
            code: files.code,
            test: files.test,
        },
        report,
    ));
    draft
}

/// Drops a draft the user does not want.
pub fn discard(id: &str) {
    store().drafts.retain(|(d, _, _)| d != id);
}

/// The user approved the draft: it joins the registry for this session. A draft whose test
/// failed cannot be approved.
pub fn approve(id: &str, registry: &ToolRegistry) -> Result<InstalledTool, String> {
    let mut s = store();
    let at = s
        .drafts
        .iter()
        .position(|(d, _, _)| d == id)
        .ok_or_else(|| String::from(err("tools.generator.draft_gone")))?;
    if !s.drafts[at].2.passed {
        return Err(err("tools.generator.test_failed").into());
    }
    let name = s.drafts[at].1.manifest.name.clone();
    if name_taken(registry, &name) {
        return Err(err("tools.package.name_taken").with("name", &name).into());
    }
    if limit_reached(registry, &name) {
        return Err(err("tools.generator.limit")
            .num("max", MAX_GENERATED as u64)
            .into());
    }
    let (_, stored, _) = s.drafts.remove(at);
    registry.register(installed::in_memory_tool(&stored.manifest, &stored.code));
    let info = InstalledTool::from_manifest(&stored.manifest, None, true);
    s.session.insert(name, stored);
    Ok(info)
}

/// Writes a tool generated in this session to `<root>/<name>/` (it becomes an installed tool;
/// the caller re-runs [`installed::sync`], which replaces the in-memory one).
pub fn save(
    root: &std::path::Path,
    name: &str,
    registry: &ToolRegistry,
) -> Result<InstalledTool, String> {
    let stored = {
        let s = store();
        let t = s
            .session
            .get(name)
            .ok_or_else(|| String::from(err("tools.generator.not_generated").with("name", name)))?;
        (
            t.manifest.clone(),
            t.manifest_json.clone(),
            t.code.clone(),
            t.test.clone(),
        )
    };
    let (manifest, manifest_json, code, test) = stored;
    let staging = std::env::temp_dir().join(format!("harness-save-{}", uuid::Uuid::new_v4()));
    let io = |e: std::io::Error| -> String { err("tools.package.io").with("error", e).into() };
    std::fs::create_dir_all(&staging).map_err(io)?;
    let written = (|| {
        std::fs::write(staging.join(package::MANIFEST_FILE), &manifest_json)?;
        std::fs::write(staging.join("index.js"), &code)?;
        std::fs::write(staging.join("test.js"), &test)
    })();
    let result = written.map_err(io).and_then(|()| {
        // `install` refuses a name a non-installed tool holds: the in-memory copy steps aside
        // for the write and comes back when it fails.
        registry.unregister(name);
        installed::install(root, &staging, registry).inspect_err(|_| {
            registry.register(installed::in_memory_tool(&manifest, &code));
        })
    });
    let _ = std::fs::remove_dir_all(&staging);
    let info = result?;
    store().session.remove(name);
    Ok(info)
}

/// Removes a tool generated in this session from the registry.
pub fn remove(name: &str, registry: &ToolRegistry) -> bool {
    let mut s = store();
    s.session.remove(name);
    registry
        .get(name)
        .is_some_and(|t| t.def().source == ToolSource::Generated)
        && registry.unregister(name)
}

#[cfg(test)]
mod tests;
