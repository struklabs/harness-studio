//! `read_file`, `file_glob_search`, `grep_search`, `write_file`, `edit_file`: llama-server's
//! file tools, confined to the workspace. Result texts follow llama-server's.

use std::fmt::Write as _;
use std::path::Path;

use serde_json::{json, Value};

use crate::tools::glob::{list_entries, Kind, PathGlob};
use crate::tools::registry::{args, ToolCtx};
use crate::tools::scope::Workspace;
use crate::tools::types::*;

const READ_FILE_MAX_SIZE: usize = 16 * 1024;
const SEARCH_MAX_RESULTS: usize = 100;
/// Files bigger than this are not searched by `grep_search`.
const GREP_MAX_FILE: u64 = 8 * 1024 * 1024;

/// `getline` semantics: split on `\n`; a final newline does not start another line.
fn lines_of(s: &str) -> Vec<&str> {
    let mut v: Vec<&str> = s.split('\n').collect();
    if s.is_empty() || s.ends_with('\n') {
        v.pop();
    }
    v
}

fn pretty(v: Value) -> String {
    serde_json::to_string_pretty(&v).unwrap_or_default()
}

/// An existing regular file inside the workspace; "cannot stat file" like llama-server when it
/// is not there.
fn existing_file(ws: &Workspace, raw: &str) -> Result<std::path::PathBuf, ToolError> {
    let cannot = || ToolError::new(ToolErrorCode::NotFound, format!("cannot stat file: {raw}"));
    let path = ws.resolve_existing(raw).map_err(|e| {
        if e.code == ToolErrorCode::NotFound {
            cannot()
        } else {
            e
        }
    })?;
    match std::fs::metadata(&path) {
        Ok(m) if m.is_file() => Ok(path),
        _ => Err(cannot()),
    }
}

fn io_err(what: &str, raw: &str, e: std::io::Error) -> ToolError {
    ToolError::new(ToolErrorCode::Io, format!("{what}: {raw} ({e})"))
}

pub fn read_file(a: &Value, ctx: &ToolCtx) -> Result<ToolOutput, ToolError> {
    let ws = ctx.workspace()?;
    let raw = args::string(a, "path")?;
    let start_line = args::opt_i64(a, "start_line").unwrap_or(1);
    let end_line = args::opt_i64(a, "end_line").unwrap_or(-1);
    let append_loc = args::opt_bool(a, "append_loc").unwrap_or(false);

    let path = existing_file(ws, &raw)?;
    let size = std::fs::metadata(&path)
        .map_err(|_| ToolError::new(ToolErrorCode::NotFound, format!("cannot stat file: {raw}")))?
        .len() as usize;
    if size > READ_FILE_MAX_SIZE && end_line == -1 {
        return Err(ToolError::failed(format!(
            "file too large ({size} bytes, max {READ_FILE_MAX_SIZE}). Use start_line/end_line to read a portion."
        )));
    }
    let bytes = std::fs::read(&path).map_err(|e| io_err("failed to open file", &raw, e))?;
    let content = String::from_utf8_lossy(&bytes);

    let mut result = String::new();
    for (i, line) in lines_of(&content).into_iter().enumerate() {
        let lineno = i as i64 + 1;
        if lineno < start_line {
            continue;
        }
        if end_line != -1 && lineno > end_line {
            break;
        }
        let out_line = if append_loc {
            format!("{lineno}\u{2192}{line}\n")
        } else {
            format!("{line}\n")
        };
        if result.len() + out_line.len() > READ_FILE_MAX_SIZE {
            result.push_str("[output truncated]");
            break;
        }
        result.push_str(&out_line);
    }
    Ok(ToolOutput::text(result))
}

fn compile_glob(pattern: &str) -> Result<PathGlob, ToolError> {
    PathGlob::new(pattern).map_err(ToolError::invalid)
}

pub fn file_glob_search(a: &Value, ctx: &ToolCtx) -> Result<ToolOutput, ToolError> {
    let ws = ctx.workspace()?;
    let raw = args::string(a, "path")?;
    let include = args::opt_string(a, "include").unwrap_or_else(|| "**".into());
    let exclude = args::opt_string(a, "exclude").unwrap_or_default();
    let ty = args::opt_string(a, "type").unwrap_or_else(|| "file".into());
    let max_depth = args::opt_i64(a, "max_depth").unwrap_or(0).max(0) as usize;
    let limit_req = args::opt_i64(a, "limit").unwrap_or(SEARCH_MAX_RESULTS as i64);
    if limit_req < 1 {
        return Err(ToolError::failed(format!(
            "invalid limit: {limit_req} (expected 1 or more)"
        )));
    }
    let limit = (limit_req as usize).min(SEARCH_MAX_RESULTS);
    let kind = match ty.as_str() {
        "file" => Kind::Files,
        "dir" => Kind::Dirs,
        "all" => Kind::All,
        other => {
            return Err(ToolError::failed(format!(
                "invalid type: {other} (expected \"file\", \"dir\" or \"all\")"
            )))
        }
    };
    let base = dir_arg(ws, &raw)?;
    let include_glob = compile_glob(&include)?;
    let exclude_glob = if exclude.is_empty() {
        None
    } else {
        Some(compile_glob(&exclude)?)
    };

    let listing = list_entries(&base, max_depth, kind, &ctx.cancel);
    let matches: Vec<_> = listing
        .entries
        .iter()
        .filter(|e| include_glob.matches(&e.rel))
        .filter(|e| exclude_glob.as_ref().is_none_or(|x| !x.matches(&e.rel)))
        .collect();
    let total = matches.len();
    let shown = total.min(limit);
    let mut out = String::new();
    for e in matches.iter().take(shown) {
        let _ = writeln!(out, "{}{}", e.rel, if e.is_dir { "/" } else { "" });
    }
    let _ = write!(out, "\n---\nTotal matches: {total}\n");
    if total > shown {
        let _ = writeln!(
            out,
            "[{shown} results limit reached ({total} total matches). Refine the glob pattern to narrow the search.]"
        );
    }
    if listing.truncated {
        out.push_str("[results truncated: time budget or unreadable directory]\n");
    }
    Ok(ToolOutput::text(out))
}

fn dir_arg(ws: &Workspace, raw: &str) -> Result<std::path::PathBuf, ToolError> {
    let not_dir = || {
        ToolError::new(
            ToolErrorCode::NotFound,
            format!("path does not exist or is not a directory: {raw}"),
        )
    };
    let path = ws.resolve_existing(raw).map_err(|e| {
        if e.code == ToolErrorCode::NotFound {
            not_dir()
        } else {
            e
        }
    })?;
    if path.is_dir() {
        Ok(path)
    } else {
        Err(not_dir())
    }
}

pub fn grep_search(a: &Value, ctx: &ToolCtx) -> Result<ToolOutput, ToolError> {
    let ws = ctx.workspace()?;
    let raw = args::string(a, "path")?;
    let pat = args::string(a, "pattern")?;
    let include = args::opt_string(a, "include").unwrap_or_else(|| "**".into());
    let exclude = args::opt_string(a, "exclude").unwrap_or_default();
    let show_lineno = args::opt_bool(a, "return_line_numbers").unwrap_or(false);
    let literal = args::opt_bool(a, "literal").unwrap_or(false);
    let ignore_case = args::opt_bool(a, "ignore_case").unwrap_or(false);
    let ctx_lines = args::opt_i64(a, "context_lines").unwrap_or(0).max(0) as usize;

    let source = if literal { regex::escape(&pat) } else { pat };
    let re = regex::RegexBuilder::new(&source)
        .case_insensitive(ignore_case)
        .size_limit(10 << 20)
        .build()
        .map_err(|e| ToolError::failed(format!("invalid regex: {e}")))?;
    let include_glob = compile_glob(&include)?;
    let exclude_glob = if exclude.is_empty() {
        None
    } else {
        Some(compile_glob(&exclude)?)
    };

    // (file, name shown)
    let mut files: Vec<(std::path::PathBuf, String)> = Vec::new();
    let target = ws.resolve_existing(&raw).map_err(|e| {
        if e.code == ToolErrorCode::NotFound {
            ToolError::new(
                ToolErrorCode::NotFound,
                format!("path does not exist: {raw}"),
            )
        } else {
            e
        }
    })?;
    if target.is_file() {
        files.push((target, raw.clone()));
    } else if target.is_dir() {
        let listing = list_entries(&target, 0, Kind::Files, &ctx.cancel);
        for e in listing.entries {
            if !include_glob.matches(&e.rel) {
                continue;
            }
            if exclude_glob.as_ref().is_some_and(|x| x.matches(&e.rel)) {
                continue;
            }
            files.push((target.join(&e.rel), e.rel));
        }
    } else {
        return Err(ToolError::new(
            ToolErrorCode::NotFound,
            format!("path does not exist: {raw}"),
        ));
    }

    let show_num = show_lineno || ctx_lines > 0;
    let mut out = String::new();
    let mut total = 0usize;
    let mut limit_reached = false;
    'files: for (file, display) in &files {
        if ctx.cancel.is_cancelled() {
            return Err(ToolError::new(
                ToolErrorCode::Cancelled,
                "the search was cancelled",
            ));
        }
        if std::fs::metadata(file).map(|m| m.len()).unwrap_or(0) > GREP_MAX_FILE {
            continue;
        }
        let Ok(bytes) = std::fs::read(file) else {
            continue;
        };
        // A NUL in the first bytes: a binary file, nothing to show.
        if bytes.iter().take(8000).any(|b| *b == 0) {
            continue;
        }
        let content = String::from_utf8_lossy(&bytes);
        let lines = lines_of(&content);
        for (i, line) in lines.iter().enumerate() {
            if total >= SEARCH_MAX_RESULTS {
                limit_reached = true;
                break 'files;
            }
            if !re.is_match(line) {
                continue;
            }
            let (from, to) = if ctx_lines > 0 {
                (
                    i.saturating_sub(ctx_lines),
                    (i + ctx_lines).min(lines.len() - 1),
                )
            } else {
                (i, i)
            };
            for (j, text) in lines.iter().enumerate().take(to + 1).skip(from) {
                let is_match = j == i;
                let sep = if is_match { ':' } else { '-' };
                let _ = write!(out, "{display}{sep}");
                if show_num {
                    let _ = write!(out, "{}{sep}", j + 1);
                }
                let _ = writeln!(out, "{text}");
            }
            if ctx_lines > 0 {
                out.push_str("--\n");
            }
            total += 1;
        }
    }
    let _ = write!(out, "\n---\nTotal matches: {total}\n");
    if limit_reached {
        let _ = writeln!(
            out,
            "[{SEARCH_MAX_RESULTS} matches limit reached. Narrow the path/pattern/include to see more.]"
        );
    }
    Ok(ToolOutput::text(out))
}

pub fn write_file(a: &Value, ctx: &ToolCtx) -> Result<ToolOutput, ToolError> {
    let ws = ctx.workspace()?;
    let raw = args::string(a, "path")?;
    let content = args::string(a, "content")?;
    let path = ws.resolve_new(&raw)?;
    if path.is_dir() {
        return Err(ToolError::failed(format!(
            "failed to write file: {raw} (it is a directory)"
        )));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| io_err("failed to write file", &raw, e))?;
    }
    // Directories created above might (in a race) be links now: check the result once more.
    ws.resolve_new(&raw)?;
    let checkpoint = ctx.checkpoint("write_file", &path);
    if let Err(e) = std::fs::write(&path, content.as_bytes()) {
        if let Some(c) = checkpoint {
            c.abandon();
        }
        return Err(io_err("failed to write file", &raw, e));
    }
    if let Some(c) = checkpoint {
        c.commit(content.as_bytes());
    }
    Ok(ToolOutput::text(pretty(json!({
        "result": "file written successfully",
        "path": raw,
        "bytes": content.len(),
    }))))
}

// --- edit_file ---------------------------------------------------------------------------

/// Strips trailing whitespace and maps typographic quotes, dashes and spaces to ASCII, one
/// line at a time (llama-server's fuzzy match).
fn normalize_line(line: &str) -> String {
    line.trim_end_matches([' ', '\t', '\r'])
        .chars()
        .map(|c| match c {
            '\u{2018}'..='\u{201B}' => '\'',
            '\u{201C}'..='\u{201F}' => '"',
            '\u{2010}'..='\u{2015}' | '\u{2212}' => '-',
            '\u{00A0}' | '\u{2002}'..='\u{200A}' | '\u{202F}' | '\u{205F}' | '\u{3000}' => ' ',
            c => c,
        })
        .collect()
}

fn normalize_for_fuzzy(content: &str) -> String {
    content
        .split('\n')
        .map(normalize_line)
        .collect::<Vec<_>>()
        .join("\n")
}

struct Matched {
    edit_index: usize,
    /// Offset into the base content (normalized when a fuzzy match was needed).
    index: usize,
    len: usize,
    new_text: String,
}

fn apply_replacements(content: &str, reps: &[&Matched], offset: usize) -> String {
    let mut result = content.to_string();
    for r in reps.iter().rev() {
        let local = r.index - offset;
        result.replace_range(local..local + r.len, &r.new_text);
    }
    result
}

/// Like `apply_replacements`, but lines no edit touched come from the original text.
fn apply_preserving_unchanged_lines(original: &str, base: &str, reps: &[Matched]) -> String {
    let original_lines: Vec<&str> = original.split_inclusive('\n').collect();
    let mut spans: Vec<(usize, usize)> = Vec::new();
    let mut offset = 0;
    for line in base.split_inclusive('\n') {
        spans.push((offset, offset + line.len()));
        offset += line.len();
    }
    // Widen each replacement to the lines it touches; merge those that share lines.
    let mut groups: Vec<(usize, usize, Vec<&Matched>)> = Vec::new();
    for rep in reps {
        let end_byte = rep.index + rep.len;
        let start_line = spans
            .iter()
            .position(|(s, e)| rep.index >= *s && rep.index < *e)
            .unwrap_or(0);
        let mut end_line = start_line;
        while end_line < spans.len() && spans[end_line].1 < end_byte {
            end_line += 1;
        }
        let end_line = (end_line + 1).min(spans.len());
        match groups.last_mut() {
            Some(last) if start_line < last.1 => {
                last.1 = last.1.max(end_line);
                last.2.push(rep);
            }
            _ => groups.push((start_line, end_line, vec![rep])),
        }
    }
    let mut next_line = 0;
    let mut result = String::new();
    for (start, end, group) in groups {
        for line in
            &original_lines[next_line.min(original_lines.len())..start.min(original_lines.len())]
        {
            result.push_str(line);
        }
        let from = spans[start].0;
        let to = spans[end - 1].1;
        result.push_str(&apply_replacements(&base[from..to], &group, from));
        next_line = end;
    }
    for line in original_lines.iter().skip(next_line) {
        result.push_str(line);
    }
    result
}

pub fn edit_file(a: &Value, ctx: &ToolCtx) -> Result<ToolOutput, ToolError> {
    let ws = ctx.workspace()?;
    let raw = args::string(a, "path")?;
    let Some(edits_json) = a
        .get("edits")
        .and_then(Value::as_array)
        .filter(|e| !e.is_empty())
    else {
        return Err(ToolError::failed("\"edits\" must be a non-empty array"));
    };
    let mut edits: Vec<(String, String)> = Vec::with_capacity(edits_json.len());
    for (i, e) in edits_json.iter().enumerate() {
        let old = e.get("old_text").and_then(Value::as_str);
        let new = e.get("new_text").and_then(Value::as_str);
        let (Some(old), Some(new)) = (old, new) else {
            return Err(ToolError::invalid(format!(
                "edits[{i}] needs string old_text and new_text"
            )));
        };
        if old.is_empty() {
            return Err(ToolError::failed(format!(
                "edits[{i}].old_text must not be empty"
            )));
        }
        edits.push((old.to_string(), new.to_string()));
    }

    let path = existing_file(ws, &raw).map_err(|e| {
        if e.code == ToolErrorCode::NotFound {
            ToolError::new(
                ToolErrorCode::NotFound,
                format!("failed to open file: {raw}"),
            )
        } else {
            e
        }
    })?;
    let bytes = std::fs::read(&path).map_err(|e| io_err("failed to open file", &raw, e))?;
    let original = String::from_utf8(bytes)
        .map_err(|_| ToolError::failed(format!("the file is not valid UTF-8 text: {raw}")))?;

    // Does any old_text need the fuzzy match (no exact match)?
    let fuzzy_original = normalize_for_fuzzy(&original);
    let mut any_fuzzy = false;
    for (i, (old, _)) in edits.iter().enumerate() {
        if original.contains(old.as_str()) {
            continue;
        }
        if !fuzzy_original.contains(normalize_for_fuzzy(old).as_str()) {
            return Err(ToolError::failed(format!(
                "could not find edits[{i}].old_text in {raw}, it must match the file's current content exactly"
            )));
        }
        any_fuzzy = true;
    }
    let base: &str = if any_fuzzy {
        &fuzzy_original
    } else {
        &original
    };

    // Uniqueness is always judged on the normalized text: a whitespace-only duplicate counts.
    let mut matched: Vec<Matched> = Vec::with_capacity(edits.len());
    for (i, (old, new)) in edits.iter().enumerate() {
        let needle = if any_fuzzy {
            normalize_for_fuzzy(old)
        } else {
            old.clone()
        };
        let occurrences = fuzzy_original
            .matches(normalize_for_fuzzy(old).as_str())
            .count();
        if occurrences > 1 {
            return Err(ToolError::failed(format!(
                "found {occurrences} occurrences of edits[{i}].old_text in {raw}, it must be unique"
            )));
        }
        let index = base.find(&needle).ok_or_else(|| {
            ToolError::failed(format!(
                "could not find edits[{i}].old_text in {raw}, it must match the file's current content exactly"
            ))
        })?;
        matched.push(Matched {
            edit_index: i,
            index,
            len: needle.len(),
            new_text: new.clone(),
        });
    }
    matched.sort_by_key(|m| m.index);
    for w in matched.windows(2) {
        if w[0].index + w[0].len > w[1].index {
            return Err(ToolError::failed(format!(
                "edits[{}] and edits[{}] overlap in {raw}; merge them into one edit or target disjoint regions",
                w[0].edit_index, w[1].edit_index
            )));
        }
    }
    let new_content = if any_fuzzy {
        apply_preserving_unchanged_lines(&original, base, &matched)
    } else {
        let refs: Vec<&Matched> = matched.iter().collect();
        apply_replacements(base, &refs, 0)
    };
    if new_content == original {
        return Err(ToolError::failed(
            "no changes made: the replacement(s) produced identical content",
        ));
    }
    let checkpoint = ctx.checkpoint("edit_file", &path);
    if let Err(e) = write_back(&path, &raw, &new_content) {
        if let Some(c) = checkpoint {
            c.abandon();
        }
        return Err(e);
    }
    if let Some(c) = checkpoint {
        c.commit(new_content.as_bytes());
    }
    Ok(ToolOutput::text(pretty(json!({
        "result": "file edited successfully",
        "path": raw,
        "edits_applied": matched.len(),
    }))))
}

fn write_back(path: &Path, raw: &str, content: &str) -> Result<(), ToolError> {
    std::fs::write(path, content.as_bytes()).map_err(|e| io_err("failed to write file", raw, e))
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{run_in, tree};
    use crate::tools::ToolErrorCode;
    use serde_json::json;

    #[tokio::test]
    async fn read_file_ranges_line_numbers_and_limits() {
        let d = tree(&[("a.txt", "one\ntwo\nthree\n"), ("sub/b.txt", "b")]);
        let r = run_in(d.path(), "read_file", json!({"path": "a.txt"})).await;
        assert_eq!(r.content, "one\ntwo\nthree\n");
        let r = run_in(
            d.path(),
            "read_file",
            json!({"path": "a.txt", "start_line": 2, "end_line": 3, "append_loc": true}),
        )
        .await;
        assert_eq!(r.content, "2\u{2192}two\n3\u{2192}three\n");
        // Models send numbers as text sometimes.
        let r = run_in(
            d.path(),
            "read_file",
            json!({"path": "a.txt", "start_line": "3"}),
        )
        .await;
        assert_eq!(r.content, "three\n");

        let r = run_in(d.path(), "read_file", json!({"path": "nope.txt"})).await;
        assert_eq!(r.error, Some(ToolErrorCode::NotFound));
        assert_eq!(r.content, "cannot stat file: nope.txt");
        // A directory is not a file.
        let r = run_in(d.path(), "read_file", json!({"path": "sub"})).await;
        assert_eq!(r.content, "cannot stat file: sub");

        // Over 16 KB without a line range: refused with a hint; with a range: read.
        let big = (0..2000).map(|i| format!("line {i}\n")).collect::<String>();
        std::fs::write(d.path().join("big.txt"), &big).unwrap();
        let r = run_in(d.path(), "read_file", json!({"path": "big.txt"})).await;
        assert_eq!(r.error, Some(ToolErrorCode::Failed));
        assert!(
            r.content.contains("Use start_line/end_line"),
            "{}",
            r.content
        );
        let r = run_in(
            d.path(),
            "read_file",
            json!({"path": "big.txt", "start_line": 10, "end_line": 12}),
        )
        .await;
        assert_eq!(r.content, "line 9\nline 10\nline 11\n");
        // A range that is itself over 16 KB is cut (the tool's own limit is 16 384 bytes, a
        // little above the registry's 16 000 characters, so the registry's marker shows).
        let r = run_in(
            d.path(),
            "read_file",
            json!({"path": "big.txt", "end_line": 2000}),
        )
        .await;
        assert!(
            r.truncated && r.content.contains("…[truncated"),
            "{}",
            r.content.len()
        );
    }

    #[tokio::test]
    async fn file_tools_cannot_leave_the_workspace() {
        let d = tree(&[("in.txt", "x")]);
        let secret = tree(&[("secret.txt", "TOP SECRET")]);
        let abs = secret
            .path()
            .join("secret.txt")
            .to_string_lossy()
            .to_string();
        for (tool, args) in [
            ("read_file", json!({"path": "../x"})),
            ("read_file", json!({"path": abs})),
            ("write_file", json!({"path": "../evil.txt", "content": "x"})),
            (
                "write_file",
                json!({"path": secret.path().join("evil.txt").to_string_lossy(), "content": "x"}),
            ),
            ("file_glob_search", json!({"path": ".."})),
            ("grep_search", json!({"path": "..", "pattern": "x"})),
            (
                "edit_file",
                json!({"path": "../secret.txt", "edits": [{"old_text": "a", "new_text": "b"}]}),
            ),
        ] {
            let r = run_in(d.path(), tool, args.clone()).await;
            assert_eq!(
                r.error,
                Some(ToolErrorCode::OutsideWorkspace),
                "{tool} {args}"
            );
        }
        assert_eq!(
            std::fs::read_to_string(secret.path().join("secret.txt")).unwrap(),
            "TOP SECRET"
        );
        assert!(!secret.path().join("evil.txt").exists());
    }

    #[tokio::test]
    async fn symlinked_directories_cannot_be_used_to_leave() {
        let d = tree(&[("in.txt", "x")]);
        let secret = tree(&[("secret.txt", "TOP SECRET")]);
        assert!(
            crate::tools::scope::link_dir(&d.path().join("link"), secret.path()),
            "could neither create a symlink nor a junction"
        );
        let r = run_in(d.path(), "read_file", json!({"path": "link/secret.txt"})).await;
        assert_eq!(r.error, Some(ToolErrorCode::OutsideWorkspace));
        let r = run_in(
            d.path(),
            "write_file",
            json!({"path": "link/evil.txt", "content": "x"}),
        )
        .await;
        assert_eq!(r.error, Some(ToolErrorCode::OutsideWorkspace));
        assert!(!secret.path().join("evil.txt").exists());
        // Searches do not walk into the link.
        let r = run_in(
            d.path(),
            "grep_search",
            json!({"path": ".", "pattern": "SECRET"}),
        )
        .await;
        assert!(r.content.contains("Total matches: 0"), "{}", r.content);
        let r = run_in(d.path(), "file_glob_search", json!({"path": "."})).await;
        assert!(!r.content.contains("secret.txt"), "{}", r.content);
    }

    #[tokio::test]
    async fn glob_search_on_a_tree() {
        let d = tree(&[
            ("src/main.rs", "fn main() {}"),
            ("src/lib/util.rs", "pub fn u() {}"),
            ("README.md", "# hi"),
            ("node_modules/x/index.js", "x"),
            ("docs/a.md", "a"),
        ]);
        let r = run_in(
            d.path(),
            "file_glob_search",
            json!({"path": ".", "include": "*.rs"}),
        )
        .await;
        assert_eq!(
            r.content,
            "src/lib/util.rs\nsrc/main.rs\n\n---\nTotal matches: 2\n"
        );
        // node_modules is skipped; a pattern with `/` is matched against the path.
        let r = run_in(
            d.path(),
            "file_glob_search",
            json!({"path": ".", "include": "**/*.js"}),
        )
        .await;
        assert!(r.content.contains("Total matches: 0"));
        let r = run_in(
            d.path(),
            "file_glob_search",
            json!({"path": ".", "include": "src/*.rs"}),
        )
        .await;
        assert!(r.content.starts_with("src/main.rs\n"), "{}", r.content);
        // Exclude, type=dir (suffix `/`), depth and limit.
        let r = run_in(
            d.path(),
            "file_glob_search",
            json!({"path": ".", "include": "*.md", "exclude": "README.*"}),
        )
        .await;
        assert_eq!(r.content, "docs/a.md\n\n---\nTotal matches: 1\n");
        let r = run_in(
            d.path(),
            "file_glob_search",
            json!({"path": ".", "type": "dir", "max_depth": 1}),
        )
        .await;
        assert_eq!(
            r.content,
            "docs/\nnode_modules/\nsrc/\n\n---\nTotal matches: 3\n"
        );
        let r = run_in(
            d.path(),
            "file_glob_search",
            json!({"path": "src", "limit": 1}),
        )
        .await;
        assert!(r
            .content
            .contains("[1 results limit reached (2 total matches)"));
        let r = run_in(
            d.path(),
            "file_glob_search",
            json!({"path": ".", "limit": 0}),
        )
        .await;
        assert_eq!(r.content, "invalid limit: 0 (expected 1 or more)");
        let r = run_in(
            d.path(),
            "file_glob_search",
            json!({"path": ".", "type": "x"}),
        )
        .await;
        assert!(r.content.starts_with("invalid type: x"));
        let r = run_in(d.path(), "file_glob_search", json!({"path": "README.md"})).await;
        assert_eq!(r.error, Some(ToolErrorCode::NotFound));
    }

    #[tokio::test]
    async fn grep_on_a_tree() {
        let d = tree(&[
            ("src/a.rs", "fn alpha() {}\nlet Beta = 1;\nfn gamma() {}\n"),
            ("src/b.txt", "alpha beta\n"),
            (".gitignore", "ignored.txt\n"),
            ("ignored.txt", "alpha hidden\n"),
        ]);
        std::fs::write(d.path().join("bin.dat"), b"alpha\0binary").unwrap();
        let r = run_in(
            d.path(),
            "grep_search",
            json!({"path": ".", "pattern": "alpha"}),
        )
        .await;
        assert_eq!(
            r.content,
            "src/a.rs:fn alpha() {}\nsrc/b.txt:alpha beta\n\n---\nTotal matches: 2\n"
        );
        // Line numbers, include filter, case-insensitive, literal.
        let r = run_in(
            d.path(),
            "grep_search",
            json!({"path": ".", "pattern": "beta", "ignore_case": true, "include": "*.rs", "return_line_numbers": true}),
        )
        .await;
        assert_eq!(
            r.content,
            "src/a.rs:2:let Beta = 1;\n\n---\nTotal matches: 1\n"
        );
        let r = run_in(
            d.path(),
            "grep_search",
            json!({"path": "src/a.rs", "pattern": "fn (", "literal": true}),
        )
        .await;
        assert!(r.content.contains("Total matches: 0"));
        let r = run_in(
            d.path(),
            "grep_search",
            json!({"path": "src/a.rs", "pattern": "fn \\w+"}),
        )
        .await;
        assert!(r.content.starts_with("src/a.rs:fn alpha() {}\n"));
        // Context lines: matches use `:`, context `-`, groups end with `--`.
        let r = run_in(
            d.path(),
            "grep_search",
            json!({"path": "src/a.rs", "pattern": "Beta", "context_lines": 1}),
        )
        .await;
        assert_eq!(
            r.content,
            "src/a.rs-1-fn alpha() {}\nsrc/a.rs:2:let Beta = 1;\nsrc/a.rs-3-fn gamma() {}\n--\n\n---\nTotal matches: 1\n"
        );
        // A bad regex is the tool's failure, not a crash.
        let r = run_in(
            d.path(),
            "grep_search",
            json!({"path": ".", "pattern": "("}),
        )
        .await;
        assert_eq!(r.error, Some(ToolErrorCode::Failed));
        assert!(r.content.starts_with("invalid regex"));
        // The match limit.
        let many = "hit\n".repeat(150);
        std::fs::write(d.path().join("many.txt"), many).unwrap();
        let r = run_in(
            d.path(),
            "grep_search",
            json!({"path": "many.txt", "pattern": "hit"}),
        )
        .await;
        assert!(r.content.contains("Total matches: 100"));
        assert!(r.content.contains("[100 matches limit reached."));
    }

    #[tokio::test]
    async fn write_file_creates_parents_and_replaces() {
        let d = tree(&[("keep.txt", "k")]);
        let r = run_in(
            d.path(),
            "write_file",
            json!({"path": "a/b/c.txt", "content": "héllo"}),
        )
        .await;
        assert!(!r.is_error(), "{}", r.content);
        let v: serde_json::Value = serde_json::from_str(&r.content).unwrap();
        assert_eq!(v["result"], "file written successfully");
        assert_eq!(v["path"], "a/b/c.txt");
        assert_eq!(v["bytes"], 6);
        assert_eq!(
            std::fs::read_to_string(d.path().join("a/b/c.txt")).unwrap(),
            "héllo"
        );
        run_in(
            d.path(),
            "write_file",
            json!({"path": "a/b/c.txt", "content": "new"}),
        )
        .await;
        assert_eq!(
            std::fs::read_to_string(d.path().join("a/b/c.txt")).unwrap(),
            "new"
        );
        // Writing onto a directory, or below a file, fails cleanly.
        let r = run_in(d.path(), "write_file", json!({"path": "a", "content": "x"})).await;
        assert!(r.is_error());
        let r = run_in(
            d.path(),
            "write_file",
            json!({"path": "keep.txt/x", "content": "x"}),
        )
        .await;
        assert!(r.is_error());
    }

    async fn edit(
        d: &std::path::Path,
        content: &str,
        edits: serde_json::Value,
    ) -> (String, String) {
        std::fs::write(d.join("f.txt"), content).unwrap();
        let r = run_in(d, "edit_file", json!({"path": "f.txt", "edits": edits})).await;
        (r.content, std::fs::read_to_string(d.join("f.txt")).unwrap())
    }

    #[tokio::test]
    async fn edit_file_semantics() {
        let d = tree(&[]);
        let p = d.path();
        // One exact edit.
        let (msg, after) = edit(p, "a\nb\nc\n", json!([{"old_text": "b", "new_text": "B"}])).await;
        assert_eq!(after, "a\nB\nc\n");
        let v: serde_json::Value = serde_json::from_str(&msg).unwrap();
        assert_eq!(
            (v["result"].as_str(), v["edits_applied"].as_i64()),
            (Some("file edited successfully"), Some(1))
        );
        // Several edits match the original, not incrementally: the second sees the first's text
        // as it was, and edits may swap content.
        let (_, after) = edit(
            p,
            "one two three",
            json!([{"old_text": "one", "new_text": "two"}, {"old_text": "two", "new_text": "one"}]),
        )
        .await;
        assert_eq!(after, "two one three");
        // Not found, not unique, overlap, empty, no change: the file stays as it was.
        let (msg, after) = edit(p, "abc", json!([{"old_text": "zzz", "new_text": "y"}])).await;
        assert!(
            msg.starts_with("could not find edits[0].old_text in f.txt"),
            "{msg}"
        );
        assert_eq!(after, "abc");
        let (msg, after) = edit(p, "x x", json!([{"old_text": "x", "new_text": "y"}])).await;
        assert_eq!(
            msg,
            "found 2 occurrences of edits[0].old_text in f.txt, it must be unique"
        );
        assert_eq!(after, "x x");
        let (msg, _) = edit(
            p,
            "abcdef",
            json!([{"old_text": "abcd", "new_text": "1"}, {"old_text": "cdef", "new_text": "2"}]),
        )
        .await;
        assert!(msg.contains("overlap"), "{msg}");
        let (msg, _) = edit(p, "abc", json!([{"old_text": "", "new_text": "1"}])).await;
        assert_eq!(msg, "edits[0].old_text must not be empty");
        let (msg, _) = edit(p, "abc", json!([])).await;
        assert_eq!(msg, "\"edits\" must be a non-empty array");
        let (msg, _) = edit(p, "abc", json!([{"old_text": "a", "new_text": "a"}])).await;
        assert!(msg.starts_with("no changes made"), "{msg}");
        // A missing file.
        let r = run_in(
            p,
            "edit_file",
            json!({"path": "none.txt", "edits": [{"old_text": "a", "new_text": "b"}]}),
        )
        .await;
        assert_eq!(r.error, Some(ToolErrorCode::NotFound));
    }

    #[tokio::test]
    async fn edit_file_fuzzy_match_keeps_untouched_lines() {
        let d = tree(&[]);
        // The file has a typographic apostrophe and trailing spaces; the model wrote ASCII.
        let original = "keep  \nit\u{2019}s here   \nlast\u{00A0}line  \n";
        let (msg, after) = edit(
            d.path(),
            original,
            json!([{"old_text": "it's here", "new_text": "it's there"}]),
        )
        .await;
        assert!(msg.contains("file edited successfully"), "{msg}");
        // The touched line is rewritten from the normalized text; the others are verbatim.
        assert_eq!(after, "keep  \nit's there\nlast\u{00A0}line  \n");
    }
}
