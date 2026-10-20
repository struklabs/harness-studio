//! The test file of a tool package: a script that declares the cases the
//! entry file must pass, run in the same sandbox, with the same permissions and limits, as the
//! tool itself.
//!
//! ```js
//! const cases = [
//!   { name: "adds", args: { a: 1, b: 2 }, expect: { equals: "3" } },
//!   { args: { text: "Hello" }, expect: { contains: ["hello", "5"] } },
//!   { args: {}, expect: { error: "missing" } },
//!   { args: {}, http: [{ url: "https://example.com", status: 200, body: "ok" }], expect: { equals: "ok" } },
//! ];
//! ```
//!
//! `args` goes to `run(args)`. `expect` holds exactly one of
//! * `equals`: the result text matches after `trim_end` on both sides (non-strings use JSON text);
//!   leading and internal whitespace must match exactly,
//! * `contains`: the result text includes this text, or every text of an array,
//! * `matches`: a JavaScript RegExp (literal, pattern string or `/pattern/flags` string),
//!   evaluated in the sandbox; literals preserve source/flags before serialization (anchor for full matches),
//! * `length`: the exact Unicode character count of the result text,
//! * `error`: `run` has to fail; `true` for any failure, a text for a failure whose message
//!   includes it.
//!
//! If `http` mocks are provided for a case, network requests are intercepted and answered from
//! the mocks without contacting the real network (an unmatched request fails). Without `http`
//! mocks, a case runs against the real host functions (subject to manifest permissions). File
//! access goes to a throwaway folder that is removed afterwards, never to the user's workspace.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::js_engine;
use super::package::{self, Permissions};
use super::registry::ToolCtx;
use super::scope::Workspace;
use super::types::ToolError;
use crate::error_key::err;

/// The whole test run, all cases together.
const TEST_TIMEOUT: Duration = Duration::from_secs(30);

#[cfg(test)]
#[test]
fn review_catastrophic_regex_deadline() {
    // Only our disposable child is killed if native RegExp ignores the interrupt hook.
    // A Tokio timeout alone cannot stop a stuck spawn_blocking worker.
    const MODE: &str = "HARNESS_REVIEW_REGEX_CHILD";
    if let Ok(mode) = std::env::var(MODE) {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (code, test) = if mode == "matches" {
            (
                "function run() { return 'a'.repeat(35) + 'b'; }",
                "const cases = [{args:{},expect:{matches:/(a+)+$/}}];",
            )
        } else {
            (
                "function run() { return /(a+)+$/.test('a'.repeat(35) + 'b'); }",
                "const cases = [{args:{},expect:{equals:'false'}}];",
            )
        };
        let started = std::time::Instant::now();
        let report = runtime.block_on(run(code.into(), test.into(), Permissions::default()));
        assert!(started.elapsed() < TEST_TIMEOUT + Duration::from_secs(2));
        assert!(
            report
                .error
                .as_deref()
                .is_some_and(|e| e.contains("did not finish in time")),
            "{report:?}"
        );
        return;
    }
    let mut failures = Vec::new();
    for mode in ["matches", "run"] {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "tools::package_test::review_catastrophic_regex_deadline",
                "--nocapture",
            ])
            .env(MODE, mode)
            .spawn()
            .unwrap();
        let started = std::time::Instant::now();
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                if !status.success() {
                    failures.push(format!("{mode}: {status}"));
                }
                break;
            }
            if started.elapsed() > TEST_TIMEOUT + Duration::from_secs(5) {
                child.kill().unwrap();
                child.wait().unwrap();
                failures.push(format!(
                    "{mode}: RegExp exceeded TEST_TIMEOUT; child stopped"
                ));
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("; "));
}

/// The outcome of one case. `reason` is empty when it passed, else `contains`, `equals`,
/// `error_expected`, `error_mismatch`, `unexpected_error` or `bad_case`; `expected` and
/// `actual` carry the texts the reason talks about.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CaseReport {
    pub name: String,
    pub passed: bool,
    pub reason: String,
    pub expected: String,
    pub actual: String,
    /// Evidence from evaluated args/file contents, never names or expectations.
    #[serde(default)]
    pub unicode_input: bool,
}

fn has_unicode_letters(value: &Value) -> bool {
    match value {
        Value::String(s) => s.chars().any(|c| !c.is_ascii() && c.is_alphabetic()),
        Value::Array(a) => a.iter().any(has_unicode_letters),
        Value::Object(o) => o.values().any(has_unicode_letters),
        _ => false,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TestReport {
    /// Every case passed (and there was at least one).
    pub passed: bool,
    pub cases: Vec<CaseReport>,
    /// The test could not run at all: the code does not load, the test file declares no cases,
    /// or the run hit the time limit. Plain engine text, not a key.
    pub error: Option<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

impl TestReport {
    fn broken(error: String) -> Self {
        Self {
            passed: false,
            cases: Vec::new(),
            error: Some(error),
            warnings: Vec::new(),
        }
    }

    /// The failures as text a model can fix them from (what the generator sends back).
    pub fn feedback(&self) -> String {
        if let Some(e) = &self.error {
            let explanation = if crate::error_key::parse(e)
                .is_some_and(|(key, _)| key == "create.js_invalid_string")
            {
                "Generated JavaScript has an unterminated string (unexpected end of string). Escape literal newlines in quoted strings or use template literals.".to_string()
            } else {
                e.clone()
            };
            return format!("The test could not run: {explanation}");
        }
        self.cases
            .iter()
            .filter(|c| !c.passed)
            .map(|c| {
                let what = match c.reason.as_str() {
                    "contains" => format!("expected the result to contain {:?}", c.expected),
                    "equals" => format!(
                        "expected the result to be exactly {:?} (ignoring trailing whitespace)",
                        c.expected
                    ),
                    "matches" => format!("expected the result to match regex {:?}", c.expected),
                    "length" => format!("expected the result character count to be {}", c.expected),
                    "error_expected" => "expected run() to fail but it returned a result".into(),
                    "error_mismatch" => {
                        format!("expected the failure message to contain {:?}", c.expected)
                    }
                    "unexpected_error" if c.actual.contains("path does not exist:") =>
                        "input file missing in the isolated test workspace; add fs: [{path, content}] to this case, or use a content string if the contract is a text transformation. HTTP mocks do not create files; setup callbacks are unsupported".into(),
                    "unexpected_error" => "run() failed although a result was expected".into(),
                    _ => format!("the case is malformed: {}", c.expected),
                };
                format!(
                    "- case {:?} [{}]: {what}; got {:?}{}",
                    c.name,
                    c.reason,
                    c.actual,
                    if c.reason == "bad_case" {
                        "; fix the test"
                    } else {
                        "; inspect code and expectation against the contract"
                    }
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn texts(v: &Value) -> Option<Vec<String>> {
    match v {
        Value::String(s) => Some(vec![s.clone()]),
        Value::Array(a) if !a.is_empty() => {
            a.iter().map(|x| x.as_str().map(str::to_string)).collect()
        }
        _ => None,
    }
}

/// Compares what `run` gave for one case with the case's `expect`.
pub fn judge(index: usize, case: &Value, outcome: &Result<String, ToolError>) -> CaseReport {
    judge_with_match(index, case, outcome, None)
}

fn judge_with_match(
    index: usize,
    case: &Value,
    outcome: &Result<String, ToolError>,
    matched: Option<&Result<bool, String>>,
) -> CaseReport {
    let name = case
        .get("name")
        .and_then(Value::as_str)
        .filter(|n| !n.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("#{}", index + 1));
    let actual = match outcome {
        Ok(text) => text.clone(),
        Err(e) => e.message.clone(),
    };
    let report = |reason: &str, expected: String| CaseReport {
        name: name.clone(),
        passed: reason.is_empty(),
        reason: reason.to_string(),
        expected,
        actual: actual.clone(),
        unicode_input: outcome.is_ok()
            && (case.get("args").is_some_and(has_unicode_letters)
                || case
                    .get("fs")
                    .and_then(Value::as_array)
                    .is_some_and(|files| {
                        files
                            .iter()
                            .any(|file| file.get("content").is_some_and(has_unicode_letters))
                    })),
    };
    let Some(expect) = case.get("expect").and_then(Value::as_object) else {
        return report("bad_case", "expect must be an object".into());
    };
    if expect.len() != 1 {
        return report(
            "bad_case",
            "expect needs exactly one of equals, contains, matches, length, error".into(),
        );
    }
    let (kind, want) = expect.iter().next().expect("one entry");
    let evaluated = (kind == "matches" && matched.is_none())
        .then(|| js_engine::match_regexp(want, outcome.as_deref().unwrap_or_default()));
    let matched = matched.or(evaluated.as_ref());
    if kind == "matches" {
        if let Some(Err(error)) = matched {
            return report(
                "bad_case",
                format!("matches must be a valid JavaScript RegExp: {error}"),
            );
        }
    }
    match (kind.as_str(), outcome) {
        ("error", _) if !matches!(want, Value::Bool(true) | Value::String(_)) => {
            report("bad_case", "error must be true or a text".into())
        }
        ("error", Err(_)) => match want {
            Value::String(s) if !actual.contains(s.as_str()) => report("error_mismatch", s.clone()),
            _ => report("", String::new()),
        },
        ("error", Ok(_)) => report("error_expected", String::new()),
        ("equals" | "contains" | "matches" | "length", Err(_)) => {
            report("unexpected_error", String::new())
        }
        ("equals", Ok(text)) => {
            let want = match want {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            if text.trim_end() == want.trim_end() {
                report("", want)
            } else {
                report("equals", want)
            }
        }
        ("contains", Ok(text)) => match texts(want) {
            Some(list) => match list.iter().find(|w| !text.contains(w.as_str())) {
                None => report("", list.join(", ")),
                Some(missing) => report("contains", missing.clone()),
            },
            None => report(
                "bad_case",
                "contains must be a text or a list of texts".into(),
            ),
        },
        ("matches", Ok(_)) => match matched.expect("matches was evaluated") {
            Ok(pass) => report(
                if *pass { "" } else { "matches" },
                match want.as_str() {
                    Some(pattern) => pattern.into(),
                    None => format!(
                        "/{}/{}",
                        want["source"].as_str().unwrap_or_default(),
                        want["flags"].as_str().unwrap_or_default()
                    ),
                },
            ),
            Err(error) => report(
                "bad_case",
                format!("matches must be a valid JavaScript RegExp: {error}"),
            ),
        },
        ("length", Ok(text)) => match want.as_u64() {
            Some(length) => report(
                if text.chars().count() as u64 == length {
                    ""
                } else {
                    "length"
                },
                length.to_string(),
            ),
            None => report("bad_case", "length must be a non-negative integer".into()),
        },
        (other, _) => report("bad_case", format!("unknown expectation {other:?}")),
    }
}

/// Each case gets a fresh directory. Fixtures are test data, not filesystem grants.
pub(super) fn case_workspace(case: &Value) -> Result<(tempfile::TempDir, Workspace), ToolError> {
    let invalid =
        |detail: &str| ToolError::invalid(format!("Invalid file fixture; fix test.js: {detail}"));
    if case.get("setup").is_some() {
        return Err(invalid(
            "setup callbacks are unsupported; use fs: [{path, content}]",
        ));
    }
    let dir = tempfile::Builder::new()
        .prefix("harness-toolcase-")
        .tempdir()
        .map_err(|e| ToolError::failed(e.to_string()))?;
    let ws = Workspace::new(dir.path())?;
    if let Some(files) = case.get("fs") {
        let files = files
            .as_array()
            .filter(|files| files.len() <= 32)
            .ok_or_else(|| invalid("fs must be an array of at most 32 files"))?;
        let mut bytes = 0usize;
        let mut paths = std::collections::BTreeSet::new();
        for file in files {
            let path = file
                .get("path")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid("path must be a relative string"))?;
            let directory = file.get("dir").and_then(Value::as_bool).unwrap_or(false);
            let content = file
                .get("content")
                .and_then(Value::as_str)
                .or(if directory { Some("") } else { None })
                .ok_or_else(|| invalid("content must be a UTF-8 string"))?;
            if directory && file.get("content").is_some() {
                return Err(invalid("directory fixtures cannot have content"));
            }
            // Reject both Windows and Unix absolute/traversing paths on every host.
            let normalized = path.replace('\\', "/");
            if normalized
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
                || normalized.contains(':')
                || normalized.contains('\0')
            {
                return Err(invalid(
                    "path must be relative, with no empty, . or .. segments",
                ));
            }
            if !paths.insert(normalized.clone()) {
                return Err(invalid("duplicate fixture path"));
            }
            bytes = bytes.saturating_add(content.len());
            if bytes > 1024 * 1024 {
                return Err(invalid("total fixture content exceeds 1 MiB"));
            }
            let target = ws.resolve_new(&normalized)?;
            if directory {
                std::fs::create_dir_all(target).map_err(|e| ToolError::failed(e.to_string()))?;
                continue;
            }
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| ToolError::failed(e.to_string()))?;
            }
            std::fs::write(target, content).map_err(|e| ToolError::failed(e.to_string()))?;
        }
    }
    Ok((dir, ws))
}

/// Runs the test of a package: `code` is the entry file, `test` the test file.
pub async fn run(code: String, test: String, perms: Permissions) -> TestReport {
    run_with_cancel(code, test, perms, false, None).await
}

/// Shares the generator's stop flag with the sandbox, including blocking JS and host calls.
pub async fn run_with_cancel(
    code: String,
    test: String,
    perms: Permissions,
    offline: bool,
    cancel: Option<&AtomicBool>,
) -> TestReport {
    let scratch = std::env::temp_dir().join(format!("harness-tooltest-{}", uuid::Uuid::new_v4()));
    let workspace = std::fs::create_dir_all(&scratch)
        .map_err(|e| ToolError::failed(e.to_string()))
        .and_then(|()| Workspace::new(&scratch));
    let workspace = match workspace {
        Ok(w) => w,
        Err(e) => return TestReport::broken(e.message),
    };
    let mut ctx = ToolCtx::new(Some(workspace));
    ctx.timeout = TEST_TIMEOUT;
    let handle = tokio::runtime::Handle::current();
    let token = ctx.cancel.clone();
    let mut task = tokio::task::spawn_blocking(move || {
        js_engine::run_cases_blocking(&code, &test, &perms, &ctx, handle, offline)
    });
    let ran = if let Some(cancel) = cancel {
        tokio::select! {
            result = &mut task => result,
            _ = async {
                while !cancel.load(Ordering::SeqCst) {
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
            } => {
                token.cancel();
                // Await the interrupted engine before deleting its workspace.
                task.await
            }
        }
    } else {
        task.await
    };
    let _ = std::fs::remove_dir_all(&scratch);
    match ran {
        Ok(Ok(runs)) => {
            let cases: Vec<CaseReport> = runs
                .iter()
                .enumerate()
                .map(|(i, (case, outcome, matched))| {
                    judge_with_match(i, case, outcome, matched.as_ref())
                })
                .collect();
            TestReport {
                passed: !cases.is_empty() && cases.iter().all(|c| c.passed),
                cases,
                error: None,
                warnings: Vec::new(),
            }
        }
        Ok(Err(e)) => TestReport::broken(if e.message.contains("unexpected end of string") {
            err("create.js_invalid_string")
                .with("detail", &e.message)
                .into()
        } else {
            e.message
        }),
        Err(e) => TestReport::broken(format!("the test crashed: {e}")),
    }
}

/// Runs the test of the package folder `dir` (`tools.package.no_test` when the manifest
/// declares none).
pub async fn run_package(dir: &Path) -> Result<TestReport, String> {
    let pkg = package::load_dir(dir)?;
    let Some(test) = package::read_test(&pkg)? else {
        return Err(err("tools.package.no_test").into());
    };
    let code = package::read_entry(&pkg)?;
    Ok(run(code, test, pkg.manifest.permissions).await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn read_directory_accepts_empty_and_nested_directory_fixtures() {
        let report = run(
            "function run(a) { return harness.fs.list(a.path).map(e => e.name + (e.dir ? '/' : '')).join(','); }".into(),
            r#"const cases = [
              {args:{path:'.'}, fs:[{path:'empty',dir:true},{path:'nested/a.txt',content:'A'},{path:'b.txt',content:'B'}], expect:{equals:'b.txt,empty/,nested/'}},
              {args:{path:'empty'}, fs:[{path:'empty',dir:true}], expect:{equals:''}},
              {args:{path:'.'}, expect:{equals:''}},
              {args:{path:'missing'}, expect:{error:'path does not exist'}},
              {args:{path:'../outside'}, expect:{error:true}}
            ];"#.into(),
            Permissions { fs: package::FsScope { read: vec![".".into()], write: vec![] }, ..Default::default() }
        ).await;
        assert!(report.passed, "{}", report.feedback());
    }

    #[tokio::test]
    async fn csv_file_fixtures_are_created_and_isolated_per_case() {
        let perms = Permissions {
            fs: package::FsScope {
                read: vec![".".into()],
                write: vec![".".into()],
            },
            ..Default::default()
        };
        let report = run_with_cancel(
            r#"function run(args) {
                const rows = harness.fs.read(args.path).split('\n');
                const headers = rows.shift().split(',');
                harness.fs.write('leak.txt', 'previous case');
                return rows.map(row => Object.fromEntries(headers.map((key, i) => [key, row.split(',')[i]])));
            }"#.into(),
            r#"const cases = [
                {args:{path:'input.csv'}, fs:[{path:'input.csv',content:'name,age\nAlice,30'}], expect:{equals:[{name:'Alice',age:'30'}]}},
                {args:{path:'nested/input.csv'}, fs:[{path:'nested/input.csv',content:'name,age\nBob,25'}], expect:{equals:[{name:'Bob',age:'25'}]}},
                {args:{path:'input.csv'}, expect:{error:'path does not exist'}},
                {args:{path:'leak.txt'}, expect:{error:'path does not exist'}}
            ];"#.into(), perms, true, None,
        ).await;
        assert!(report.passed, "{report:?}");
    }

    fn reported_packages() -> Value {
        serde_json::from_str(include_str!(
            "../../tests/data/tool-generator-reported-failures.json"
        ))
        .unwrap()
    }

    #[tokio::test]
    async fn replay_reported_file_errors_and_markdown_regex_failure() {
        let packages = reported_packages();
        for (name, args, path) in [
            ("csv_to_json", json!({"csv_path":"input.csv"}), "input.csv"),
            (
                "html_to_text",
                json!({"html_path":"input.html"}),
                "input.html",
            ),
            (
                "base64_encode",
                json!({"file_path":"input.txt"}),
                "input.txt",
            ),
        ] {
            let saved = &packages[name];
            let manifest =
                package::parse_manifest(saved["manifest_json"].as_str().unwrap()).unwrap();
            let test = format!(
                "const cases = [{}];",
                json!({"args":args,"expect":{"equals":"result"}})
            );
            let report = run_with_cancel(
                saved["code"].as_str().unwrap().into(),
                test,
                manifest.permissions,
                true,
                None,
            )
            .await;
            assert_eq!(
                report.cases[0].actual,
                format!("path does not exist: {path}")
            );
            assert!(report.feedback().contains("add fs: [{path, content}]"));
        }
        let saved = &packages["markdown_to_html"];
        let report = run(
            saved["code"].as_str().unwrap().into(),
            saved["test"].as_str().unwrap().into(),
            Permissions::default(),
        )
        .await;
        assert!(!report.passed);
        let case = &report.cases[0];
        assert_eq!(case.reason, "equals");
        assert_eq!(
            case.actual,
            "<p><h1>Header 1</h1><br>#<h1>Header 2</h1><br>### Header 3</p>"
        );
        assert!(report.feedback().contains(&case.name));
    }

    #[tokio::test]
    async fn reported_base64_file_code_passes_with_real_fixture() {
        let saved = &reported_packages()["base64_encode"];
        let manifest = package::parse_manifest(saved["manifest_json"].as_str().unwrap()).unwrap();
        let report = run_with_cancel(saved["code"].as_str().unwrap().into(),
            r#"const cases = [{args:{file_path:'input.txt'},fs:[{path:'input.txt',content:'Sample text content.'}],expect:{equals:'U2FtcGxlIHRleHQgY29udGVudC4='}}];"#.into(),
            manifest.permissions, true, None).await;
        assert!(report.passed, "{report:?}");
    }

    #[tokio::test]
    async fn reported_html_fixture_exposes_unsupported_dom_parser() {
        let saved = &reported_packages()["html_to_text"];
        let manifest = package::parse_manifest(saved["manifest_json"].as_str().unwrap()).unwrap();
        let report = run_with_cancel(
            saved["code"].as_str().unwrap().into(),
            saved["test"].as_str().unwrap().into(),
            manifest.permissions,
            true,
            None,
        )
        .await;
        assert!(report.cases[0].actual.contains("DOMParser"), "{report:?}");
        assert!(!report.passed);
    }

    #[test]
    fn fixtures_reject_escaping_paths_invalid_shapes_and_excess_content() {
        for path in [
            "../outside",
            "a/../outside",
            "/absolute",
            "C:\\outside",
            "\\server\\share",
            "a\\..\\outside",
            "",
            ".",
        ] {
            assert!(
                case_workspace(&json!({"fs":[{"path":path,"content":"text"}]})).is_err(),
                "{path}"
            );
        }
        for files in [
            json!({}),
            json!([{"path":"x"}]),
            json!([{"path":"x","content":1}]),
            json!([{"path":"x","content":"a"},{"path":"x","content":"b"}]),
            json!([{"path":"x","content":"a".repeat(1024*1024+1)}]),
            json!(vec![json!({"path":"x","content":""}); 33]),
        ] {
            assert!(case_workspace(&json!({"fs":files})).is_err());
        }
        let (dir, _) =
            case_workspace(&json!({"fs":[{"path":"nested/a.txt","content":"ş🙂"}]})).unwrap();
        let root = dir.path().to_path_buf();
        assert_eq!(
            std::fs::read_to_string(root.join("nested/a.txt")).unwrap(),
            "ş🙂"
        );
        drop(dir);
        assert!(!root.exists());
    }

    #[tokio::test]
    async fn setup_named_input_data_is_preserved() {
        let report = run(
            "function run(args){return args.setup}".into(),
            "const cases=[{args:{setup:'ordinary input'},expect:{equals:'ordinary input'}}];"
                .into(),
            Permissions::default(),
        )
        .await;
        assert!(report.passed, "{report:?}");
    }

    #[tokio::test]
    async fn fixtures_do_not_grant_permissions_and_setup_is_not_silently_ignored() {
        let report = run("function run(){return harness.fs.read('input.txt')}".into(),
            "const cases=[{fs:[{path:'input.txt',content:'secret'}],expect:{error:'no file access'}}];".into(), Permissions::default()).await;
        assert!(report.passed, "{report:?}");
        let report = run(
            "function run(){return 'ok'}".into(),
            "const cases=[{setup:()=>{harness.fs.write('x','y')},expect:{equals:'ok'}}];".into(),
            Permissions::default(),
        )
        .await;
        assert!(!report.passed);
        assert!(report
            .feedback()
            .contains("setup callbacks are unsupported"));
    }

    fn ok(s: &str) -> Result<String, ToolError> {
        Ok(s.to_string())
    }

    fn fail(s: &str) -> Result<String, ToolError> {
        Err(ToolError::failed(s))
    }

    #[test]
    fn equals_and_contains_compare_the_result_text() {
        let eq = json!({"name": "sum", "args": {}, "expect": {"equals": "3"}});
        assert!(judge(0, &eq, &ok("3")).passed);
        let r = judge(0, &eq, &ok("33"));
        assert!(!r.passed);
        assert_eq!((r.reason.as_str(), r.expected.as_str()), ("equals", "3"));
        let num = json!({"args": {}, "expect": {"equals": 3}});
        assert!(judge(0, &num, &ok("3")).passed);
        let has = json!({"args": {}, "expect": {"contains": ["Hello", "5"]}});
        assert!(judge(0, &has, &ok("Hello, 5 chars")).passed);
        let r = judge(0, &has, &ok("Hello"));
        assert_eq!((r.reason.as_str(), r.expected.as_str()), ("contains", "5"));
        assert_eq!(r.name, "#1");
    }

    #[test]
    fn equals_ignores_only_trailing_whitespace_and_preserves_report_text() {
        let case = json!({"expect": {"equals": "{\n  \"a\": 1\n}\n"}});
        for actual in [
            "{\n  \"a\": 1\n}",
            "{\n  \"a\": 1\n}\r\n \t",
            "{\n  \"a\": 1\n}\u{2003}",
        ] {
            let report = judge(0, &case, &ok(actual));
            assert!(report.passed, "{report:?}");
            assert_eq!(report.actual, actual);
            assert_eq!(report.expected, "{\n  \"a\": 1\n}\n");
        }
        for actual in [" {\n  \"a\": 1\n}", "{\n \"a\": 1\n}", "{\n  \"a\":  1\n}"] {
            assert_eq!(judge(0, &case, &ok(actual)).reason, "equals");
        }
        let plain = json!({"expect": {"equals": "abc"}});
        assert!(judge(0, &plain, &ok("abc\n")).passed);
        assert!(!judge(0, &json!({"expect": {"contains": "abc\n"}}), &ok("abc")).passed);
        assert!(!judge(0, &json!({"expect": {"length": 3}}), &ok("abc\n")).passed);
    }

    #[test]
    fn error_cases_need_a_failure() {
        let any = json!({"args": {}, "expect": {"error": true}});
        assert!(judge(0, &any, &fail("boom")).passed);
        assert_eq!(judge(0, &any, &ok("fine")).reason, "error_expected");
        let text = json!({"args": {}, "expect": {"error": "missing"}});
        assert!(judge(0, &text, &fail("a field is missing")).passed);
        assert_eq!(judge(0, &text, &fail("other")).reason, "error_mismatch");
        let eq = json!({"args": {}, "expect": {"equals": "x"}});
        let r = judge(1, &eq, &fail("boom"));
        assert_eq!(
            (r.reason.as_str(), r.actual.as_str()),
            ("unexpected_error", "boom")
        );
    }

    #[test]
    fn a_malformed_case_is_reported_not_passed() {
        for expect in [
            json!({}),
            json!({"equals": "a", "contains": "a"}),
            json!({"contains": []}),
            json!({"contains": [1]}),
            json!({"error": 5}),
            json!({"matches": "["}),
            json!("a"),
        ] {
            let case = json!({"args": {}, "expect": expect});
            assert_eq!(judge(0, &case, &ok("a")).reason, "bad_case", "{expect}");
        }
        assert_eq!(judge(0, &json!({"args": {}}), &ok("a")).reason, "bad_case");
    }

    #[test]
    fn feedback_names_the_failing_cases() {
        let report = TestReport {
            passed: false,
            cases: vec![
                judge(
                    0,
                    &json!({"name": "a", "expect": {"equals": "1"}}),
                    &ok("1"),
                ),
                judge(
                    1,
                    &json!({"name": "b", "expect": {"equals": "2"}}),
                    &ok("3"),
                ),
            ],
            error: None,
            warnings: Vec::new(),
        };
        let text = report.feedback();
        assert!(text.contains("case \"b\"") && !text.contains("case \"a\""));
        assert!(text.contains("ignoring trailing whitespace"));
        assert!(text.contains("exactly \"2\"") && text.contains("got \"3\""));
        assert!(TestReport::broken("no cases".into())
            .feedback()
            .contains("no cases"));
    }

    #[test]
    fn deterministic_properties_validate_random_results() {
        let regex = json!({"expect": {"matches": "^[A-Za-z0-9]{12}$"}});
        assert!(judge(0, &regex, &ok("Ab1cD2eF3gH4")).passed);
        assert_eq!(
            judge(0, &regex, &ok("ABCDEFGHIJKLMNOPQRSTUVWXYZ")).reason,
            "matches"
        );
        assert_eq!(judge(0, &regex, &fail("boom")).reason, "unexpected_error");
        let length = json!({"expect": {"length": 2}});
        assert!(judge(0, &length, &ok("\u{00fc}\u{1f642}")).passed);
        assert_eq!(judge(0, &length, &ok("abc")).reason, "length");
        for expect in [
            json!({"matches": 1}),
            json!({"matches": "["}),
            json!({"length": -1}),
            json!({"length": 1.5}),
        ] {
            assert_eq!(
                judge(0, &json!({"expect": expect}), &ok("a")).reason,
                "bad_case"
            );
        }
    }

    #[tokio::test]
    async fn json_values_and_promises_keep_their_result_representation() {
        let code = "function run(a) { return a.async ? Promise.resolve(a.value) : a.value; }";
        let test = r#"const cases = [
            { args: {value: null}, expect: {equals: null} },
            { args: {}, expect: {equals: null} },
            { args: {value: true}, expect: {equals: true} },
            { args: {value: false}, expect: {equals: false} },
            { args: {value: 42}, expect: {equals: 42} },
            { args: {value: {a: 1}}, expect: {equals: {a: 1}} },
            { args: {value: [1, null]}, expect: {equals: [1, null]} },
            { args: {value: ""}, expect: {equals: ""} },
            { args: {value: null, async: true}, expect: {equals: "null"} },
            { args: {async: true}, expect: {equals: "null"} }
        ];"#;
        let report = run(code.into(), test.into(), Permissions::default()).await;
        assert!(report.passed, "{report:?}");
    }

    #[tokio::test]
    async fn literal_newlines_in_quoted_js_strings_explain_the_syntax_error() {
        let report = run(
            "function run() { return 'ok'; }".into(),
            "const cases = [{args: {}, expect: {equals: 'line one\nline two'}}];".into(),
            Permissions::default(),
        )
        .await;
        let error = report.error.unwrap();
        assert_eq!(
            crate::error_key::parse(&error).unwrap().0,
            "create.js_invalid_string"
        );
        assert!(error.contains("unexpected end of string"));
    }

    const CODE: &str = "function run(a) { if (!a.text) throw new Error('text is missing'); return a.text.toUpperCase(); }";

    #[tokio::test]
    async fn the_cases_run_in_the_sandbox_against_the_entry_file() {
        let test = r#"const cases = [
            { name: "upper", args: { text: "abc" }, expect: { equals: "ABC" } },
            { name: "no text", args: {}, expect: { error: "missing" } },
            { args: { text: "x" }, expect: { contains: "X" } },
        ];"#;
        let r = run(CODE.into(), test.into(), Permissions::default()).await;
        assert!(r.passed, "{r:?}");
        assert_eq!(r.cases.len(), 3);
        let bad = r#"const cases = [{ args: { text: "abc" }, expect: { equals: "abc" } }];"#;
        let r = run(CODE.into(), bad.into(), Permissions::default()).await;
        assert!(!r.passed);
        assert_eq!(r.cases[0].reason, "equals");
        assert_eq!(r.cases[0].actual, "ABC");
    }

    #[tokio::test]
    async fn a_test_without_cases_or_with_broken_code_cannot_pass() {
        for test in ["const x = 1;", "const cases = [];", "const cases = ;"] {
            let r = run(CODE.into(), test.into(), Permissions::default()).await;
            assert!(!r.passed && r.error.is_some(), "{test}: {r:?}");
        }
        let test = r#"const cases = [{ args: {}, expect: { error: true } }];"#;
        let r = run(
            "function run(a) {".into(),
            test.into(),
            Permissions::default(),
        )
        .await;
        assert!(!r.passed && r.error.is_some());
    }

    #[tokio::test]
    async fn the_sandbox_limits_apply_to_tests_too() {
        // No `network` permission: the host function refuses, the case sees an error.
        let code = "function run(a) { return harness.http.get('http://example.com/').body; }";
        let test = r#"const cases = [{ args: {}, expect: { error: "may not contact" } }];"#;
        let r = run(code.into(), test.into(), Permissions::default()).await;
        assert!(r.passed, "{r:?}");
    }

    #[tokio::test]
    async fn tests_can_mock_network_calls() {
        let code = r#"function run(a) { 
            let r = harness.http.get('https://api.test.com/data'); 
            let r2 = harness.http.post('https://api.test.com/user', {name: "x"});
            return r.body + ' ' + r2.body; 
        }"#;
        // Mock matches exactly and prefix, handles methods, allows json and string bodies
        let test = r#"const cases = [{ 
            args: {}, 
            http: [
                { url: "https://api.test.com/data", method: "GET", body: "abc" },
                { url_prefix: "https://api.test.com/", method: "POST", body: {ok: true} }
            ],
            expect: { equals: "abc {\"ok\":true}" } 
        }];"#;
        let mut perms = Permissions::default();
        perms.network.push("api.test.com".into());
        let r = run(code.into(), test.into(), perms.clone()).await;
        assert!(r.passed, "{r:?}");
        let any = Permissions {
            network: vec!["*".into()],
            ..Default::default()
        };
        let r = run(code.into(), test.into(), any).await;
        assert!(
            r.passed,
            "entire internet accepts unlisted public hosts: {r:?}"
        );

        // Unmatched request error
        let code2 =
            "function run(a) { return harness.http.get('https://api.test.com/other').status; }";
        let test2 = r#"const cases = [{ args: {}, http: [{url: "https://api.test.com/data", body: "1"}], expect: { error: "no mock for https://api.test.com/other" } }];"#;
        let r2 = run(code2.into(), test2.into(), perms.clone()).await;
        assert!(r2.passed, "{r2:?}");

        // Mock for a host outside the allow-list is rejected exactly like real network
        let code3 = "function run(a) { return harness.http.get('https://evil.com/data').body; }";
        let test3 = r#"const cases = [{ args: {}, http: [{url: "https://evil.com/data", body: "bad"}], expect: { error: "it is not in the tool's allowed hosts" } }];"#;
        let r3 = run(code3.into(), test3.into(), perms).await;
        assert!(r3.passed, "{r3:?}");
    }
}

#[cfg(test)]
mod js_regex_tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn regex_literals_strings_flags_and_js_syntax_survive_serialization() {
        let code = "function run() { return 'ABC 12.[{}]'; }";
        let test = r#"const cases = [
            {name:'literal', expect:{matches:/^(?=ABC)\w+\s\d+\.\[\{\}\]$/}},
            {name:'flags', expect:{matches:'/^abc/i'}},
            {name:'pattern', expect:{matches:'^ABC'}},
            {name:'global1', expect:{matches:/ABC/g}},
            {name:'global2', expect:{matches:/ABC/g}}
        ];"#;
        let r = run(code.into(), test.into(), Permissions::default()).await;
        assert!(r.passed, "{r:?}");
    }

    #[tokio::test]
    async fn invalid_patterns_flags_and_shapes_are_bad_cases() {
        let test = r#"const cases = [
            {expect:{matches:'['}}, {expect:{matches:'/a/zz'}},
            {expect:{matches:{source:'a',flags:1}}}, {expect:{matches:42}}
        ];"#;
        let r = run(
            "function run(){return 'a'}".into(),
            test.into(),
            Permissions::default(),
        )
        .await;
        assert_eq!(r.cases.len(), 4);
        assert!(r.cases.iter().all(|c| c.reason == "bad_case"), "{r:?}");
        assert!(r.feedback().contains("fix the test"));
        assert_eq!(
            judge(
                0,
                &json!({"expect":{"matches":"["}}),
                &Err(ToolError::failed("run failed"))
            )
            .reason,
            "bad_case"
        );
        assert!(
            judge(
                0,
                &json!({"expect":{"matches":"/^(?=a)a$/"}}),
                &Ok("a".into())
            )
            .passed
        );
    }

    #[tokio::test]
    async fn syntax_feedback_identifies_the_file_to_fix() {
        let r = run(
            "function run(){return 'a'}".into(),
            "const cases=[{expect:{matches:/[/}}];".into(),
            Permissions::default(),
        )
        .await;
        assert!(
            r.feedback()
                .contains("test.js failed to load (fix the test)"),
            "{r:?}"
        );
        let r = run(
            "function run( {".into(),
            "const cases=[];".into(),
            Permissions::default(),
        )
        .await;
        assert!(
            r.feedback()
                .contains("index.js failed to load (fix the implementation)"),
            "{r:?}"
        );
    }
}
