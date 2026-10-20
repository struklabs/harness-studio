//! Generator logic with real strings: reply parsing, the permission upper bound, the retry loop
//! (against a local HTTP endpoint that answers like a chat server, with replies that are wrong
//! in the ways the loop has to handle) and the approve / save flow. The run against a real
//! llama-server and model is `e2e_a_real_model_writes_a_working_tool` (`#[ignore]`).
//!
//! ```text
//! cargo test -p harness-core tools::generator -- --ignored --nocapture
//! ```

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::*;
use crate::error_key::parse as parse_key;
use crate::tools::PermissionClass;

#[tokio::test]
async fn read_directory_recovers_from_test_setup_failure_with_actionable_history() {
    let code = "function run(a) { return harness.fs.list(a.path).map(e => e.name + (e.dir ? '/' : '')).join(','); }";
    let mut manifest = manifest("read_directory");
    manifest["description"] = json!("List directory entries");
    manifest["permissions"]["fs"]["read"] = json!(["."]);
    manifest["input_schema"] =
        json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]});
    let broken = "const cases = [{args:{path:'.'},setup:()=>{},expect:{equals:''}}];";
    let fixed = "const cases = [{args:{path:'.'},fs:[{path:'empty',dir:true}],expect:{equals:'empty/'}},{args:{path:'empty'},fs:[{path:'empty',dir:true}],expect:{equals:''}}];";
    let (endpoint, seen) = chat_endpoint(vec![
        reply_with(manifest.clone(), code, broken),
        reply_with(manifest, code, fixed),
    ])
    .await;
    let mut req = request_for(endpoint, &[]);
    req.fs_read = vec![".".into()];
    req.description = "List directory entries".into();
    let draft = generate(&req, &ToolRegistry::new(), &AtomicBool::new(false))
        .await
        .unwrap();
    assert!(draft.report.passed, "{}", draft.report.feedback());
    assert_eq!(draft.attempts, 2);
    assert_eq!(draft.history[0].total_cases, 0);
    assert!(draft.history[0]
        .failure_summary
        .contains("setup callbacks are unsupported"));
    assert!(last_user_message(&seen).contains("setup callbacks are unsupported"));
    assert!(seen.lock().unwrap()[0]["messages"][0]["content"]
        .as_str()
        .unwrap()
        .contains("{name: string, dir: boolean, size: number}"));
    discard(&draft.id);
}
#[tokio::test]
async fn nonconsecutive_code_does_not_trigger_repetition_strategy() {
    let saved: Value = serde_json::from_str(include_str!(
        "../../../tests/data/tool-generator-reported-failures.json"
    ))
    .unwrap();
    let bad = saved["markdown_to_html"]["code"].as_str().unwrap();
    let test = r#"const cases = [
        {name:'headings',args:{markdown:'# Header 1\n## Header 2\n### Header 3'},expect:{equals:'<h1>Header 1</h1><h2>Header 2</h2><h3>Header 3</h3>'}},
        {name:'last_line',args:{markdown:'## Üç'},expect:{equals:'<h2>Üç</h2>'}}
    ];"#;
    let fixed = r#"function run(args) { return args.markdown.split('\n').map(line => {
        const match = /^(#{1,6}) (.*)$/.exec(line);
        return match ? '<h'+match[1].length+'>'+match[2]+'</h'+match[1].length+'>' : line;
    }).join(''); }"#;
    let package = |code: &str| reply_with(manifest("markdown_retry"), code, test);
    let (ep, seen) = chat_endpoint(vec![
        package(bad),
        package("function run(){return 'different failure'}"),
        package(bad),
        package(bad),
        package(fixed),
    ])
    .await;
    let draft = generate(
        &request_for(ep, &[]),
        &ToolRegistry::new(),
        &AtomicBool::new(false),
    )
    .await
    .unwrap();
    assert!(draft.report.passed, "{:?}", draft.report);
    assert_eq!(draft.attempts, 5);
    assert_eq!(draft.history[4].strategy, "repair");
    assert!(!draft.history[2].repeated);
    assert!(draft.history[3].repeated);
    let requests = seen.lock().unwrap();
    let messages = requests[4]["messages"].as_array().unwrap();
    let feedback = messages.last().unwrap()["content"].as_str().unwrap();
    assert!(!feedback.contains("Only one attempt remains: rewrite index.js"));
    assert!(feedback.contains("Header 2") && feedback.contains("### Header 3"));
    assert!(!feedback.contains("correct ONLY test.js"));
    discard(&draft.id);
}

#[tokio::test]
async fn repeated_failures_switch_to_test_first_then_rewrite() {
    let bad = "function run(){return 'wrong'}";
    let test = "const cases=[{name:'a',args:{},expect:{equals:'correct'}},{name:'b',args:{x:1},expect:{equals:'correct'}}];";
    let package = |code: &str| {
        format!(
            "CHANGE: Review behavior and expectations\n{}",
            reply_with(manifest("smart_retry"), code, test)
        )
    };
    let (mut ep, seen) = chat_endpoint(vec![
        package(bad),
        package("function  run(){return 'wrong'}"),
        package(bad),
        package(bad),
        package("function run(){return 'correct'}"),
    ])
    .await;
    ep.headers
        .insert("X-Harness-Client".into(), "old-client".into());
    let draft = generate(
        &request_for(ep, &[]),
        &ToolRegistry::new(),
        &AtomicBool::new(false),
    )
    .await
    .unwrap();
    assert!(draft.report.passed);
    assert_eq!(draft.attempts, 5);
    assert_eq!(draft.history.len(), 5);
    assert_eq!(draft.history[3].strategy, "test_first");
    assert_eq!(draft.history[4].strategy, "rewrite");
    assert_eq!(draft.history[1].failed_cases, 2);
    assert!(draft.history[1].repeated);
    assert_eq!(draft.history[4].change, "Review behavior and expectations");
    assert!(draft.usage.estimated);
    assert_eq!(
        draft.usage.prompt_tokens,
        draft
            .history
            .iter()
            .map(|a| a.usage.prompt_tokens)
            .sum::<u64>()
    );
    let seen = seen.lock().unwrap();
    assert!(seen.iter().all(|r| r["_client"] == true));
    assert!(seen[3]["messages"].to_string().contains("ONLY test.js"));
    assert!(seen[4]["messages"].to_string().contains("FROM SCRATCH"));
    discard(&draft.id);
}

#[test]
fn repeated_failures_include_actual_output_and_ignore_order() {
    let case = |name: &str, actual: &str| package_test::CaseReport {
        name: name.into(),
        passed: false,
        reason: "equals".into(),
        expected: "ok".into(),
        actual: actual.into(),
        unicode_input: false,
    };
    let mut report = TestReport {
        passed: false,
        cases: vec![case("a", "wrong"), case("b", "bad")],
        error: None,
        warnings: Vec::new(),
    };
    let key = failed_signature(&report);
    report.cases.reverse();
    assert_eq!(key, failed_signature(&report));
    report.cases[0].actual = "different".into();
    assert_ne!(key, failed_signature(&report));
}

#[tokio::test]
async fn fake_context_summarizes_old_attempts_without_trimming_request_or_last_reply() {
    let bad = TEST.replace("cba", "wrong");
    let reply = format!(
        "CHANGE: Preserve valid tests\n{}\n{}",
        "explanation ".repeat(650),
        reply_with(manifest("compact_retry"), CODE, &bad)
    );
    let responses = vec![json!({"choices":[{"message":{"content":reply}}]})];
    let (ep, seen) = chat_endpoint_context(responses, Some(16_000)).await;
    let req = request_for(ep, &[]);
    let original = user_prompt(&req);
    let d = generate(&req, &ToolRegistry::new(), &AtomicBool::new(false))
        .await
        .unwrap();
    assert!(d.history.iter().any(|a| a.compacted));
    let seen = seen.lock().unwrap();
    let compacted = seen
        .iter()
        .find(|r| {
            r["messages"]
                .to_string()
                .contains("Earlier attempts (summary)")
        })
        .unwrap();
    let msgs = compacted["messages"].as_array().unwrap();
    assert_eq!(msgs[0]["role"], "system");
    assert_eq!(msgs[1]["content"], original);
    assert_eq!(msgs[3]["content"], reply);
    assert!(msgs[2]["content"]
        .as_str()
        .unwrap()
        .contains("Preserve valid tests"));
    assert_eq!(msgs[4]["role"], "user");
    discard(&d.id);
}

#[tokio::test]
async fn reported_usage_and_truncated_completion_are_accounted() {
    let (ep, _) = chat_endpoint_responses(vec![
        json!({"usage":{"prompt_tokens":23,"completion_tokens":11},"choices":[{"finish_reason":"length","message":{"content":"partial"}}]}),
        json!({"usage":{"prompt_tokens":42,"completion_tokens":17},"choices":[{"message":{"content":good_reply("usage_test")}}]}),
    ]).await;
    let d = generate(
        &request_for(ep, &[]),
        &ToolRegistry::new(),
        &AtomicBool::new(false),
    )
    .await
    .unwrap();
    assert_eq!(
        (
            d.usage.prompt_tokens,
            d.usage.completion_tokens,
            d.usage.estimated
        ),
        (65, 28, false)
    );
    assert_eq!(d.history.len(), 2);
    assert_eq!(d.history[0].usage.completion_tokens, 11);
    discard(&d.id);
}

#[tokio::test]
async fn estimate_uses_actual_history_and_scales_tool_count() {
    let (ep, _) = chat_endpoint(vec![good_reply("unused")]).await;
    let req = request_for(ep, &[]);
    let history = vec![Attempt {
        usage: TokenUsage {
            prompt_tokens: 200,
            completion_tokens: 800,
            estimated: false,
        },
        ..Default::default()
    }];
    let estimate = estimate(&req, 6, "Plan tools", &history, &AtomicBool::new(false))
        .await
        .unwrap();
    assert!(estimate.historical);
    assert_eq!(estimate.reply_per_tool, 800);
    assert!(estimate.high > estimate.low && estimate.low > 6 * 800);
}

#[tokio::test]
async fn review_syntax_error_survives_case_count_guard() {
    let bad = TEST.replace("cba", "wrong");
    for broken in ["const cases = [;", "throw new Error('test setup broke');"] {
        let (ep, seen) = chat_endpoint(vec![
            reply_with(manifest("syntax_guard"), CODE, &bad),
            reply_with(manifest("syntax_guard"), CODE, broken),
            good_reply("syntax_guard"),
        ])
        .await;
        let d = generate(
            &request_for(ep, &[]),
            &ToolRegistry::new(),
            &AtomicBool::new(false),
        )
        .await
        .unwrap();
        let feedback = last_user_message(&seen);
        assert!(feedback.contains("test.js failed to load"), "{feedback}");
        assert!(!feedback.contains("Do not delete cases"), "{feedback}");
        assert!(d.report.passed);
        discard(&d.id);
    }
}

#[test]
fn review_legacy_escape_repair_ignores_prose_quotes() {
    let code = r"function run(a) { return /\w/.test(a.text); }";
    let invalid = reply_with(manifest("prose_escape"), code, TEST).replace(r"\\w", r"\w");
    let reply = format!("Explanation with an unmatched \" quote\n{invalid}");
    assert_eq!(
        parse_reply(&reply, &Permissions::default()).unwrap().code,
        code
    );
    let quoted_code = r#"function run(a) { return "quoted" + /\w/.test(a.text); }"#;
    let invalid = reply_with(manifest("quoted_escape"), quoted_code, TEST).replace(r"\\w", r"\w");
    assert_eq!(
        parse_reply(&invalid, &Permissions::default()).unwrap().code,
        quoted_code
    );
    assert!(parse_reply(&format!("{reply}\n{invalid}"), &Permissions::default()).is_err());
}

#[test]
fn review_invalid_unicode_escapes_are_literalized() {
    for escape in [r"\u", r"\u12", r"\u12xz"] {
        let code = format!("function run() {{ return '{escape}'; }}");
        let invalid = reply_with(manifest("unicode_escape"), &code, TEST).replace(r"\\u", r"\u");
        assert_eq!(
            parse_reply(&invalid, &Permissions::default()).unwrap().code,
            code
        );
    }
}

#[test]
fn review_long_fences_preserve_template_literal_backticks() {
    // A comment inside template interpolation permits literal backticks on their own line.
    let code = "function run() { return `example\n${/*\n```\n*/'content'}\nend`; }";
    for close in ["````", "`````"] {
        let reply = format!(
            "```json\n{}\n```\n````js index.js\n{code}\n{close}\n```js test.js\n{TEST}\n```",
            manifest("template_fence")
        );
        assert_eq!(
            parse_reply(&reply, &Permissions::default()).unwrap().code,
            code
        );
    }
    let unclosed = format!(
        "```json\n{}\n```\n````js index.js\n{code}\n```",
        manifest("template_fence")
    );
    assert_eq!(
        key_of(&parse_reply(&unclosed, &Permissions::default()).unwrap_err()),
        "create.model_reply_truncated"
    );
    assert!(SYSTEM_PROMPT.contains("wrap that file in ````js fences"));
}

const CODE: &str = "function run(args) {\n  if (typeof args.text !== 'string') throw new Error('text must be a string');\n  return args.text.split('').reverse().join('');\n}";
const TEST: &str = r#"const cases = [
  { name: "reverses", args: { text: "abc üç" }, expect: { equals: "çü cba" } },
  { name: "bad input", args: {}, expect: { error: "must be a string" } },
];"#;

#[tokio::test]
async fn unicode_review_retries_missing_real_input_coverage() {
    let ascii = r#"const cases=[{name:'üç',args:{text:'abc'},expect:{equals:'cba'}},{args:{text:'def'},expect:{equals:'fed'}}];"#;
    let (ep, seen) = chat_endpoint(vec![
        reply_with(manifest("unicode_retry"), CODE, ascii),
        good_reply("unicode_retry"),
    ])
    .await;
    let mut req = request_for(ep, &[]);
    req.description = "Türkçe metni tersine çevir".into();
    let draft = generate(&req, &ToolRegistry::new(), &AtomicBool::new(false))
        .await
        .unwrap();
    assert!(draft.report.passed);
    assert_eq!(draft.attempts, 2);
    assert!(seen.lock().unwrap()[1]["messages"]
        .to_string()
        .contains("Unicode coverage missing"));
    discard(&draft.id);
}

#[test]
fn unicode_review_triggers_for_text_or_non_ascii_description_only() {
    let mut files = parse_reply(&good_reply("unicode_check"), &Permissions::default()).unwrap();
    let case = |value: &str| {
        package_test::judge(
            0,
            &json!({"args":{"text":value},"expect":{"equals":"ok"}}),
            &Ok("ok".into()),
        )
    };
    let report = |value: &str| TestReport {
        passed: true,
        cases: vec![case(value)],
        error: None,
        warnings: Vec::new(),
    };
    let mut ascii = report("abc");
    review_unicode("Reverse text", &files, &mut ascii);
    assert!(!ascii.passed);
    let mut unicode = report("üç");
    review_unicode("Reverse text", &files, &mut unicode);
    assert!(unicode.passed);
    files.code = "function run(a){return a.x+1}".into();
    let mut ascii = report("abc");
    review_unicode("Add numbers", &files, &mut ascii);
    assert!(ascii.passed);
    review_unicode("Sayıları topla", &files, &mut ascii);
    assert!(!ascii.passed, "a string argument can carry Turkish input");
    // S38: integer-only input cannot carry non-ASCII letters.
    let text_schema = files.manifest.input_schema.clone();
    files.manifest.input_schema =
        json!({"type":"object","properties":{"n":{"type":"integer"}},"required":["n"]});
    let mut numeric = report("7");
    review_unicode("Sayının asal olup olmadığını söyler", &files, &mut numeric);
    assert!(numeric.passed);
    files.manifest.input_schema = text_schema;
    files.code = r"function run(a){return a.text.replace(/[^\w\s]/g,'')}".into();
    let mut ascii = report("abc");
    review_unicode("Text statistics", &files, &mut ascii);
    assert!(ascii.error.unwrap().contains("ASCII-only regex"));
    let mut unicode = report("üç");
    review_unicode("Text statistics", &files, &mut unicode);
    assert!(unicode.passed);
    assert_eq!(unicode.warnings.len(), 1);
}

#[tokio::test]
async fn unicode_text_statistics_preserve_turkish_letters_in_sandbox() {
    let test = r#"const cases=[{args:{text:'bir iki iki üç üç üç'},expect:{equals:'6'}},{args:{text:'ü ç ş ğ ı ö'},expect:{equals:'6'}}];"#;
    let ascii = r#"function run(a){return a.text.replace(/[^\w\s]/g,'').trim().split(/\s+/).length.toString()}"#;
    let unicode = r#"function run(a){return a.text.replace(/[^\p{L}\p{N}\s]/gu,'').trim().split(/\s+/u).length.toString()}"#;
    let broken = package_test::run(ascii.into(), test.into(), Permissions::default()).await;
    assert!(!broken.passed);
    let fixed = package_test::run(unicode.into(), test.into(), Permissions::default()).await;
    assert!(fixed.passed, "{fixed:?}");
    assert!(fixed.cases.iter().all(|case| case.unicode_input));
}

fn manifest(name: &str) -> Value {
    json!({
        "name": name,
        "description": "Reverses a text.",
        "input_schema": {"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]},
        "output": "The text, reversed.",
        "permissions": {"network": [], "fs": {"read": [], "write": []}}
    })
}

fn reply_with(manifest: Value, code: &str, test: &str) -> String {
    json!({"manifest": manifest, "index_js": code, "test_js": test}).to_string()
}

fn good_reply(name: &str) -> String {
    reply_with(manifest(name), CODE, TEST)
}

fn key_of(e: &str) -> String {
    parse_key(e).map(|(k, _)| k.to_string()).unwrap_or_default()
}

fn perms(network: &[&str], read: &[&str], write: &[&str]) -> Permissions {
    let own = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    Permissions {
        network: own(network),
        fs: package::FsScope {
            read: own(read),
            write: own(write),
        },
        shell: false,
    }
}

// ---- upper bound ----

#[test]
fn hosts_must_be_listed_by_the_user() {
    let allowed = perms(&["example.com", "*.wikipedia.org"], &[], &[]);
    assert!(within_bounds(&perms(&["example.com"], &[], &[]), &allowed).is_ok());
    assert!(within_bounds(&perms(&["en.wikipedia.org"], &[], &[]), &allowed).is_ok());
    assert!(within_bounds(&perms(&["*.wikipedia.org"], &[], &[]), &allowed).is_ok());
    assert!(within_bounds(&perms(&["*.en.wikipedia.org"], &[], &[]), &allowed).is_ok());
    for wider in [
        "evil.com",
        "wikipedia.org",
        "*.example.com",
        "api.example.com",
        "*.com",
    ] {
        let e = within_bounds(&perms(&[wider], &[], &[]), &allowed).unwrap_err();
        assert_eq!(
            key_of(&e),
            "tools.generator.permissions_exceeded",
            "{wider}"
        );
    }
    // Nothing allowed: nothing may be asked.
    assert!(within_bounds(&perms(&["example.com"], &[], &[]), &Permissions::default()).is_err());
    assert!(within_bounds(&Permissions::default(), &Permissions::default()).is_ok());
}

#[test]
fn folders_must_be_inside_the_ones_the_user_listed() {
    let allowed = perms(&[], &["docs", "./notes/"], &["out"]);
    assert!(within_bounds(&perms(&[], &["docs"], &[]), &allowed).is_ok());
    assert!(within_bounds(&perms(&[], &["docs/sub/deep"], &["out/a"]), &allowed).is_ok());
    assert!(within_bounds(&perms(&[], &["notes"], &[]), &allowed).is_ok());
    for (read, write) in [
        (vec!["."], vec![]),
        (vec!["documents"], vec![]),
        (vec!["out"], vec![]),
        (vec![], vec!["docs"]),
        (vec![], vec!["."]),
    ] {
        assert!(
            within_bounds(&perms(&[], &read, &write), &allowed).is_err(),
            "{read:?} {write:?}"
        );
    }
    // The whole workspace granted covers any folder.
    assert!(within_bounds(&perms(&[], &["a/b"], &[]), &perms(&[], &["."], &[])).is_ok());
}

// ---- reply parsing ----

#[test]
fn a_good_reply_becomes_checked_files() {
    let f = parse_reply(&good_reply("reverse_text"), &Permissions::default()).unwrap();
    assert_eq!(f.manifest.name, "reverse_text");
    // The generator owns these fields; the version defaults.
    assert_eq!(f.manifest.entry, "index.js");
    assert_eq!(f.manifest.test.as_deref(), Some("test.js"));
    assert_eq!(f.manifest.version, "0.1.0");
    assert_eq!(f.manifest.class(), PermissionClass::Safe);
    assert_eq!(f.code, CODE);
    assert!(f.manifest_json.contains("reverse_text"));
    package::parse_manifest(&f.manifest_json).unwrap();
}

#[test]
fn fenced_and_wrapped_replies_are_accepted() {
    let body = good_reply("reverse_text");
    for text in [
        format!("```json\n{body}\n```"),
        format!("Here is the tool:\n{body}\nHope it helps."),
        format!("<think>hmm {{not json}}</think>\n{body}"),
    ] {
        assert!(
            parse_reply(&text, &Permissions::default()).is_ok(),
            "{text}"
        );
    }
}

#[test]
fn unusable_replies_say_what_is_wrong() {
    let none = Permissions::default();
    for (text, key) in [
        ("no json here".to_string(), "tools.generator.reply_invalid"),
        (
            json!({"index_js": CODE}).to_string(),
            "tools.generator.reply_invalid",
        ),
        (
            reply_with(manifest("reverse_text"), "  ", TEST),
            "tools.generator.reply_invalid",
        ),
        (
            reply_with(manifest("Bad-Name"), CODE, TEST),
            "tools.package.name_invalid",
        ),
    ] {
        assert_eq!(
            key_of(&parse_reply(&text, &none).unwrap_err()),
            key,
            "{text}"
        );
    }
    let mut shell = manifest("reverse_text");
    shell["permissions"]["shell"] = json!(true);
    let e = parse_reply(&reply_with(shell, CODE, TEST), &none).unwrap_err();
    assert_eq!(key_of(&e), "tools.package.shell_not_allowed");
}

#[test]
fn a_manifest_that_asks_for_more_than_allowed_is_refused() {
    let mut m = manifest("fetcher");
    m["permissions"]["network"] = json!(["example.com", "tracker.net"]);
    m["permissions"]["fs"]["write"] = json!(["out"]);
    let reply = reply_with(m, CODE, TEST);
    let e = parse_reply(&reply, &perms(&["example.com"], &[], &["out"])).unwrap_err();
    let (key, params) = parse_key(&e).unwrap();
    assert_eq!(key, "tools.generator.permissions_exceeded");
    assert_eq!(params["item"], "tracker.net");
    let e = parse_reply(&reply, &perms(&["example.com", "tracker.net"], &[], &[])).unwrap_err();
    assert_eq!(parse_key(&e).unwrap().1["kind"], "write");
    assert!(parse_reply(
        &reply,
        &perms(&["example.com", "tracker.net"], &[], &["out"])
    )
    .is_ok());
}

// ---- the retry loop against a chat endpoint ----

/// Answers each `POST /v1/chat/completions` with the next reply of the list (the last one
/// repeats) and keeps the request bodies.
async fn chat_endpoint(replies: Vec<String>) -> (Endpoint, Arc<Mutex<Vec<Value>>>) {
    chat_endpoint_responses(
        replies
            .into_iter()
            .map(|reply| json!({"choices": [{"message": {"role": "assistant", "content": reply}}]}))
            .collect(),
    )
    .await
}

async fn chat_endpoint_responses(replies: Vec<Value>) -> (Endpoint, Arc<Mutex<Vec<Value>>>) {
    chat_endpoint_context(replies, None).await
}
async fn chat_endpoint_context(
    replies: Vec<Value>,
    n_ctx: Option<u64>,
) -> (Endpoint, Arc<Mutex<Vec<Value>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen: Arc<Mutex<Vec<Value>>> = Arc::default();
    let log = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let mut raw = Vec::new();
            let mut buf = vec![0u8; 16384];
            let body_at = loop {
                let n = sock.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    break None;
                }
                raw.extend_from_slice(&buf[..n]);
                if let Some(i) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&raw[..i]).to_lowercase();
                    let len: usize = head
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .and_then(|v| v.trim().parse().ok())
                        .unwrap_or(0);
                    if raw.len() >= i + 4 + len {
                        break Some(i + 4);
                    }
                }
            };
            let Some(at) = body_at else { continue };
            let request: Value = serde_json::from_slice(&raw[at..]).unwrap_or(Value::Null);
            if !String::from_utf8_lossy(&raw).starts_with("POST /v1/chat/completions ") {
                let body = if String::from_utf8_lossy(&raw).starts_with("GET /props ") {
                    json!({"default_generation_settings":{"n_ctx":n_ctx}}).to_string()
                } else {
                    "{}".into()
                };
                let out = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(out.as_bytes()).await;
                continue;
            }
            let n = {
                let mut l = log.lock().unwrap();
                let mut request = request;
                request["_client"] = json!(String::from_utf8_lossy(&raw)
                    .contains("x-harness-client: harness-tool-creator"));
                assert!(!String::from_utf8_lossy(&raw).contains("old-client"));
                l.push(request);
                l.len()
            };
            let reply = &replies[(n - 1).min(replies.len() - 1)];
            let body = reply.to_string();
            let gateway_header = if reply["_gateway"] == true {
                "x-harness-request-id: test\r\n"
            } else {
                ""
            };
            let out = format!(
                "HTTP/1.1 200 OK\r\n{gateway_header}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(out.as_bytes()).await;
            let _ = sock.shutdown().await;
        }
    });
    let ep = Endpoint {
        base_url: format!("http://127.0.0.1:{port}/v1"),
        headers: Default::default(),
        model: Some("m".into()),
    };
    (ep, seen)
}

fn request_for(ep: Endpoint, network: &[&str]) -> GenerateRequest {
    GenerateRequest {
        description: "Create the requested tool.".into(),
        network: network.iter().map(|s| s.to_string()).collect(),
        fs_read: vec![],
        fs_write: vec![],
        endpoint: ep,
    }
}

fn last_user_message(seen: &Arc<Mutex<Vec<Value>>>) -> String {
    let l = seen.lock().unwrap();
    let msgs = l.last().unwrap()["messages"].as_array().unwrap().clone();
    msgs.last().unwrap()["content"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn a_valid_first_reply_needs_one_request() {
    let (ep, seen) = chat_endpoint(vec![good_reply("rev_one")]).await;
    let registry = ToolRegistry::new();
    let d = generate(&request_for(ep, &[]), &registry, &AtomicBool::new(false))
        .await
        .unwrap();
    assert_eq!((d.attempts, d.report.passed), (1, true));
    assert_eq!(d.info.name, "rev_one");
    assert_eq!(seen.lock().unwrap().len(), 1);
    // The request carries the user's words and the upper bound.
    let first = seen.lock().unwrap()[0]["messages"].to_string();
    assert!(first.contains("Create the requested tool.") && first.contains("network hosts: none"));
    // Nothing is registered before the user approves.
    assert!(registry.get("rev_one").is_none());
    discard(&d.id);
}

#[tokio::test]
async fn a_bad_reply_goes_back_to_the_model_with_the_error() {
    let mut greedy = manifest("rev_two");
    greedy["permissions"]["network"] = json!(["tracker.net"]);
    let broken_test = r#"const cases = [{ args: { text: "abc" }, expect: { equals: "abc" } }];"#;
    let (ep, seen) = chat_endpoint(vec![
        "I cannot do that".into(),
        reply_with(greedy, CODE, TEST),
        reply_with(manifest("rev_two"), CODE, broken_test),
        good_reply("rev_two"),
    ])
    .await;
    let registry = ToolRegistry::new();
    let d = generate(&request_for(ep, &[]), &registry, &AtomicBool::new(false))
        .await
        .unwrap();
    assert_eq!(seen.lock().unwrap().len(), 4);
    assert_eq!(d.attempts, 4);
    assert!(d.report.passed);
    let msgs = seen.lock().unwrap()[1]["messages"].to_string();
    assert!(msgs.contains("I cannot do that") && msgs.contains("reply_invalid"));
    let msgs = seen.lock().unwrap()[2]["messages"].to_string();
    assert!(msgs.contains("permissions_exceeded") && msgs.contains("tracker.net"));
    discard(&d.id);
}

#[tokio::test]
async fn a_fix_that_works_ends_the_loop() {
    let broken_test = r#"const cases = [{ args: { text: "abc" }, expect: { equals: "abc" } }];"#;
    let (ep, seen) = chat_endpoint(vec![
        reply_with(manifest("rev_three"), CODE, broken_test),
        good_reply("rev_three"),
    ])
    .await;
    let registry = ToolRegistry::new();
    let d = generate(&request_for(ep, &[]), &registry, &AtomicBool::new(false))
        .await
        .unwrap();
    assert_eq!((d.attempts, d.report.passed), (2, true));
    assert_eq!(seen.lock().unwrap().len(), 2);
    discard(&d.id);
}

#[tokio::test]
async fn replies_that_never_validate_are_an_error() {
    let (ep, seen) = chat_endpoint(vec!["nothing useful".into()]).await;
    let e = generate(
        &request_for(ep, &[]),
        &ToolRegistry::new(),
        &AtomicBool::new(false),
    )
    .await
    .unwrap_err();
    assert_eq!(key_of(&e), "tools.generator.failed");
    assert_eq!(seen.lock().unwrap().len(), 1 + MAX_RETRIES);
}

#[tokio::test]
async fn a_name_of_a_built_in_tool_is_refused() {
    let (ep, _) = chat_endpoint(vec![good_reply("read_file")]).await;
    let registry = ToolRegistry::with_builtin();
    let e = generate(&request_for(ep, &[]), &registry, &AtomicBool::new(false))
        .await
        .unwrap_err();
    assert!(e.contains("name_taken"), "{e}");
}

#[tokio::test]
async fn a_server_error_is_reported_and_cancel_stops_the_request() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}/v1", listener.local_addr().unwrap());
    drop(listener);
    let ep = Endpoint {
        base_url: base_url.clone(),
        headers: Default::default(),
        model: None,
    };
    let e = generate(
        &request_for(ep, &[]),
        &ToolRegistry::new(),
        &AtomicBool::new(false),
    )
    .await
    .unwrap_err();
    assert_eq!(key_of(&e), "http.connect_failed_local");
    let (_, params) = parse_key(&e).unwrap();
    assert_eq!(params["url"], format!("{base_url}/chat/completions"));
    assert!(params.contains_key("transport_detail"));
    assert!(!params.contains_key("error"));
    let bad = GenerateRequest {
        description: "  ".into(),
        ..request_for(Endpoint::default(), &[])
    };
    let e = generate(&bad, &ToolRegistry::new(), &AtomicBool::new(false))
        .await
        .unwrap_err();
    assert_eq!(key_of(&e), "tools.generator.description_invalid");
    let wide = request_for(Endpoint::default(), &["not a host"]);
    let e = generate(&wide, &ToolRegistry::new(), &AtomicBool::new(false))
        .await
        .unwrap_err();
    assert_eq!(key_of(&e), "tools.package.network_invalid");
}

// ---- approve, limit, save ----

#[tokio::test]
async fn an_approved_tool_runs_through_the_registry_and_can_be_saved() {
    let (ep, _) = chat_endpoint(vec![good_reply("rev_saved")]).await;
    let registry = ToolRegistry::with_builtin();
    let d = generate(&request_for(ep, &[]), &registry, &AtomicBool::new(false))
        .await
        .unwrap();
    let info = approve(&d.id, &registry).unwrap();
    assert_eq!(info.name, "rev_saved");
    let def = registry
        .list()
        .into_iter()
        .find(|t| t.name == "rev_saved")
        .unwrap();
    assert_eq!(def.source, ToolSource::Generated);
    assert_eq!(def.class, PermissionClass::Safe);
    let inv = crate::tools::ToolInvocation::new(
        "rev_saved",
        json!({"text": "harness"}),
        crate::tools::ToolOrigin::new("test"),
    );
    let out = registry
        .run(&inv, &crate::tools::ToolCtx::new(None), &Default::default())
        .await;
    assert_eq!((out.error, out.content.as_str()), (None, "ssenrah"));
    // A draft is approved once.
    assert_eq!(
        key_of(&approve(&d.id, &registry).unwrap_err()),
        "tools.generator.draft_gone"
    );

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("tools");
    let saved = save(&root, "rev_saved", &registry).unwrap();
    assert_eq!(saved.name, "rev_saved");
    assert!(root.join("rev_saved/index.js").exists() && root.join("rev_saved/test.js").exists());
    assert!(installed::sync(&root, &registry).is_empty());
    let def = registry
        .list()
        .into_iter()
        .find(|t| t.name == "rev_saved")
        .unwrap();
    assert_eq!(def.source, ToolSource::Installed);
    // The saved package passes its own test from the folder.
    let report = package_test::run_package(&root.join("rev_saved"))
        .await
        .unwrap();
    assert!(report.passed, "{report:?}");
    assert_eq!(
        key_of(&save(&root, "rev_saved", &registry).unwrap_err()),
        "tools.generator.not_generated"
    );
}

#[tokio::test]
async fn at_most_five_generated_tools_are_registered() {
    let registry = ToolRegistry::new();
    for i in 0..MAX_GENERATED {
        let (ep, _) = chat_endpoint(vec![good_reply(&format!("rev_cap_{i}"))]).await;
        let d = generate(&request_for(ep, &[]), &registry, &AtomicBool::new(false))
            .await
            .unwrap();
        approve(&d.id, &registry).unwrap();
    }
    assert_eq!(session_tools(&registry).0.len(), MAX_GENERATED);
    let (ep, seen) = chat_endpoint(vec![good_reply("rev_cap_extra")]).await;
    let e = generate(&request_for(ep, &[]), &registry, &AtomicBool::new(false))
        .await
        .unwrap_err();
    assert_eq!(key_of(&e), "tools.generator.limit");
    assert!(
        seen.lock().unwrap().is_empty(),
        "no model request past the limit"
    );
    // Removing one frees a place.
    assert!(remove("rev_cap_0", &registry));
    assert_eq!(session_tools(&registry).0.len(), MAX_GENERATED - 1);
}

// ---- a real model ----

fn llama_dir() -> PathBuf {
    std::env::var_os("HARNESS_E2E_LLAMA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var("APPDATA").unwrap_or_default())
                .join("com.harnessstudio.app/bin/managed")
        })
}

fn model_path() -> PathBuf {
    std::env::var_os("HARNESS_E2E_MODEL")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(
                r"D:\Models\unsloth\Qwen3-4B-Instruct-2507-GGUF\Qwen3-4B-Instruct-2507-Q4_K_M.gguf",
            )
        })
}

struct Server {
    child: Child,
    host: String,
    _dir: tempfile::TempDir,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn copy_dir(from: &Path, to: &Path) {
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        if e.file_type().unwrap().is_file() {
            std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
        }
    }
}

/// llama-server of `HARNESS_E2E_LLAMA_DIR` with `HARNESS_E2E_MODEL`, on a free port.
async fn start_server() -> Server {
    let src = llama_dir();
    let exe = if cfg!(windows) {
        "llama-server.exe"
    } else {
        "llama-server"
    };
    assert!(src.join(exe).exists(), "no llama-server in {src:?}");
    assert!(model_path().exists(), "model not found: {:?}", model_path());
    let dir = tempfile::tempdir().unwrap();
    copy_dir(&src, dir.path());
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let child = Command::new(dir.path().join(exe))
        .arg("-m")
        .arg(model_path())
        .args(["-c", "8192", "--jinja", "--host", "127.0.0.1", "--port"])
        .arg(port.to_string())
        .args(["-ngl", "99", "-np", "1"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let server = Server {
        child,
        host: format!("http://127.0.0.1:{port}"),
        _dir: dir,
    };
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        if let Ok(r) = reqwest::get(format!("{}/health", server.host)).await {
            if r.status().is_success() {
                break;
            }
        }
        assert!(
            Instant::now() < deadline,
            "llama-server did not become healthy"
        );
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    server
}

#[tokio::test]
#[ignore = "needs a llama-server build and a model (see the module doc)"]
async fn e2e_a_real_model_writes_a_working_tool() {
    let server = start_server().await;
    let ep = Endpoint {
        base_url: format!("{}/v1", server.host),
        headers: Default::default(),
        model: None,
    };
    let req = GenerateRequest {
        description: "Count the words in a text and also return how many characters it has (spaces not counted). Take the text as the argument `text`.".into(),
        network: vec![],
        fs_read: vec![],
        fs_write: vec![],
        endpoint: ep,
    };
    let registry = ToolRegistry::with_builtin();
    let d = generate(&req, &registry, &AtomicBool::new(false))
        .await
        .unwrap();
    println!(
        "attempts: {}\n{}\n--- index.js\n{}\n--- test.js\n{}\n{:#?}",
        d.attempts, d.manifest_json, d.code, d.test, d.report
    );
    // The upper bound held: no network, no folders.
    assert!(d.info.network.is_empty() && d.info.fs_read.is_empty() && d.info.fs_write.is_empty());
    assert!(d.attempts as usize <= 1 + MAX_RETRIES);
    // A small model may write a test its own code fails (its code or its expectations are
    // wrong): then the draft cannot be approved. When the test passes, the tool joins the
    // registry and runs.
    if d.report.passed {
        approve(&d.id, &registry).unwrap();
        assert!(registry.get(&d.info.name).is_some());
    } else {
        let e = approve(&d.id, &registry).unwrap_err();
        assert_eq!(key_of(&e), "tools.generator.test_failed");
        assert!(registry.get(&d.info.name).is_none());
    }
}

#[test]
fn reply_extraction_handles_prose_fences_and_string_braces() {
    let good = good_reply("wrapped_reply");
    for text in [
        format!("Here is the package:\n```json\n{good}\n```\nDone {{not JSON}}"),
        format!("Explanation {{{{invalid}}}}\n{good}\nTrailing text"),
    ] {
        assert!(
            parse_reply(&text, &Permissions::default()).is_ok(),
            "{text}"
        );
    }
    assert!(json_object(
        &format!("{good} {good}"),
        &["manifest", "index_js", "test_js"]
    )
    .is_none());
    assert!(parse_reply(
        &json!({"manifest": "[]"}).to_string(),
        &Permissions::default()
    )
    .is_err());
}

#[tokio::test]
async fn token_limit_replies_are_retried_even_when_the_json_is_complete() {
    let (ep, seen) = chat_endpoint_responses(vec![
        json!({"choices": [{"finish_reason": "length", "message": {"content": good_reply("limit_retry")}}]}),
        json!({"choices": [{"finish_reason": "stop", "message": {"content": good_reply("limit_retry")}}]}),
    ]).await;
    let d = generate(
        &request_for(ep, &[]),
        &ToolRegistry::new(),
        &AtomicBool::new(false),
    )
    .await
    .unwrap();
    assert_eq!(d.attempts, 2);
    assert!(last_user_message(&seen).contains("token limit"));
    assert_eq!(seen.lock().unwrap()[0]["max_tokens"], MAX_REPLY_TOKENS);
    discard(&d.id);
}

#[tokio::test]
async fn retry_keeps_previous_code_and_all_acronym_and_input_failures() {
    let code = "function run(a) { return a.text.toLowerCase(); }";
    let test = r#"const cases = [
        {name: "acronym", args: {text: "HTTPRequest"}, expect: {equals: "http_request"}},
        {name: "unicode", args: {text: "üç"}, expect: {equals: "üç"}},
        {name: "invalid_format", args: {text: "invalid"}, expect: {error: true}}
    ];"#;
    let fixed = "function run(a) { if (a.text === 'invalid') throw new Error('invalid input'); return a.text.replace(/([A-Z]+)([A-Z][a-z])/g, '$1_$2').toLowerCase(); }";
    let (ep, seen) = chat_endpoint(vec![
        reply_with(manifest("edge_retry"), code, test),
        reply_with(manifest("edge_retry"), fixed, test),
    ])
    .await;
    let d = generate(
        &request_for(ep, &[]),
        &ToolRegistry::new(),
        &AtomicBool::new(false),
    )
    .await
    .unwrap();
    assert!(d.report.passed);
    let messages = seen.lock().unwrap()[1]["messages"].clone();
    assert_eq!(
        messages[2]["content"],
        reply_with(manifest("edge_retry"), code, test)
    );
    let feedback = messages[3]["content"].as_str().unwrap();
    assert!(
        feedback.contains("http_request")
            && feedback.contains("httprequest")
            && feedback.contains("invalid_format")
            && feedback.contains("expected run() to fail")
    );
    discard(&d.id);
}

#[test]
fn reply_extraction_selects_only_complete_packages() {
    let good = good_reply("example_reply");
    for text in [
        format!("Örnek: {{}}\n{good}"),
        format!("{good}\nÖrnek: {{\"q\":\"x\"}}"),
    ] {
        let files = parse_reply(&text, &Permissions::default()).unwrap();
        assert_eq!(files.manifest.name, "example_reply");
        assert_eq!(files.code, CODE);
    }
    let cut = &good[..good.len() - 1];
    for text in [
        cut.to_string(),
        r#"{"manifest": {"name": "inner_only"}"#.to_string(),
        format!("{cut}, invalid"),
        format!("{good} {good}"),
    ] {
        assert!(
            json_object(&text, &["manifest", "index_js", "test_js"]).is_none(),
            "{text}"
        );
        assert!(parse_reply(&text, &Permissions::default()).is_err());
    }
}

#[tokio::test]
async fn token_limit_retries_preserve_the_last_test_failed_draft() {
    let test = r#"const cases = [{name: "wrong", args: {text: "abc"}, expect: {equals: "abc"}}];"#;
    let reply = reply_with(manifest("preserved_draft"), CODE, test);
    let (ep, seen) = chat_endpoint_responses(vec![
        json!({"choices": [{"finish_reason": "stop", "message": {"content": reply}}]}),
        json!({"choices": [{"finish_reason": "length", "message": {"content": "{\"manifest\":"}}]}),
    ])
    .await;
    let registry = ToolRegistry::new();
    let d = generate(&request_for(ep, &[]), &registry, &AtomicBool::new(false))
        .await
        .unwrap();
    assert_eq!(seen.lock().unwrap().len(), 1 + MAX_RETRIES);
    assert_eq!(d.info.name, "preserved_draft");
    assert_eq!(d.code, CODE);
    assert_eq!(d.test, test);
    assert!(!d.report.passed);
    assert_eq!(d.report.cases.len(), 1);
    assert_eq!(d.report.cases[0].actual, "cba");
    assert_eq!(
        key_of(d.report.error.as_deref().unwrap()),
        "create.model_reply_truncated"
    );
    assert_eq!(
        key_of(&approve(&d.id, &registry).unwrap_err()),
        "tools.generator.test_failed"
    );
    discard(&d.id);
}
#[tokio::test]
async fn repeated_code_warns_on_final_attempt_without_weakening_tests() {
    let code = "function run(){return 'wrong'}";
    let test = "const cases=[{args:{},expect:{equals:'correct'}},{args:{extra:true},expect:{equals:'correct'}}];";
    let (ep, seen) = chat_endpoint(vec![
        reply_with(manifest("repeated_code"), code, test),
        reply_with(manifest("repeated_code"), code, test),
        reply_with(
            manifest("repeated_code"),
            "function run(){return 'correct'}",
            test,
        ),
    ])
    .await;
    let d = generate(
        &request_for(ep, &[]),
        &ToolRegistry::new(),
        &AtomicBool::new(false),
    )
    .await
    .unwrap();
    assert!(d.report.passed);
    assert_eq!(d.attempts, 3);
    assert_eq!(d.test, test);
    let messages = seen.lock().unwrap()[2]["messages"].clone();
    let feedback = messages[5]["content"].as_str().unwrap();
    assert!(feedback.contains("You repeated the same code"));
    assert!(feedback.contains("DIFFERENT approach"));
    assert!(feedback.contains("Never weaken valid expectations"));
    discard(&d.id);
}

#[test]
fn raw_blocks_preserve_js_escapes_and_require_complete_unique_blocks() {
    let m = manifest("raw_reply").to_string();
    let code = r"function run(a) { return /\w+\s\d+\./.test(a.text); }";
    for headings in [true, false] {
        let reply = if headings {
            format!(
                "```json\n{m}\n```\nindex.js:\n```js\n{code}\n```\ntest.js:\n```js\n{TEST}\n```"
            )
        } else {
            format!("```json\n{m}\n```\n```js index.js\n{code}\n```\n```js test.js\n{TEST}\n```")
        };
        let f = parse_reply(&reply, &Permissions::default()).unwrap();
        assert_eq!(f.code, code);
        assert_eq!(f.test, TEST);
        let cut = &reply[..reply.len() - 3];
        assert_eq!(
            key_of(&parse_reply(cut, &Permissions::default()).unwrap_err()),
            "create.model_reply_truncated"
        );
        assert!(parse_reply(
            &format!("{reply}\n```js index.js\n{code}\n```"),
            &Permissions::default()
        )
        .is_err());
    }
}

#[test]
fn legacy_invalid_escapes_are_repaired_once_without_altering_valid_escapes() {
    let code = r"function run(a) { return /\w+\d+\s+\./.test(a.text); }";
    let reply = reply_with(manifest("escape_reply"), code, TEST);
    let invalid = reply
        .replace(r"\\w", r"\w")
        .replace(r"\\d", r"\d")
        .replace(r"\\s", r"\s")
        .replace(r"\\.", r"\.");
    for reply in [invalid.clone(), format!("```json\n{invalid}\n```")] {
        let f = parse_reply(&reply, &Permissions::default()).unwrap();
        assert_eq!(f.code, code);
        assert_eq!(f.test, TEST);
    }
    let valid = r#"{"x":"\n\r\t\b\f\u1234\\\"\/","y":"\w"}"#;
    let repaired = repair_json_escapes(valid);
    assert!(repaired.contains(r#""x":"\n\r\t\b\f\u1234\\\"\/""#));
    assert_eq!(
        serde_json::from_str::<Value>(&repaired).unwrap()["y"],
        r"\w"
    );
    assert_eq!(repair_json_escapes(r"outside \w"), r"outside \w");
    assert!(parse_reply(&invalid[..invalid.len() - 1], &Permissions::default()).is_err());
}

#[tokio::test]
async fn retry_cannot_hide_failures_by_deleting_cases() {
    let bad = TEST.replace("cba", "wrong");
    let reduced = r#"const cases = [{name:"bad input",args:{},expect:{error:true}}];"#;
    let (ep, seen) = chat_endpoint(vec![
        reply_with(manifest("case_guard"), CODE, &bad),
        reply_with(manifest("case_guard"), CODE, reduced),
        reply_with(manifest("case_guard"), CODE, TEST),
    ])
    .await;
    let d = generate(
        &request_for(ep, &[]),
        &ToolRegistry::new(),
        &AtomicBool::new(false),
    )
    .await
    .unwrap();
    assert!(d.report.passed);
    assert_eq!(d.attempts, 3);
    let feedback = last_user_message(&seen);
    assert!(
        feedback.contains("case count decreased")
            && feedback.contains("reverses")
            && feedback.contains("wrong")
    );
    discard(&d.id);
}

#[test]
fn retry_guidance_distinguishes_test_errors_and_allows_explained_corrections() {
    let message = retry_message("bad_case: malformed regex");
    assert!(
        message.contains("fix the test")
            && message.contains("one-line JS comment")
            && message.contains("Never weaken")
            && message.contains("```js test.js")
    );
}

#[tokio::test]
async fn rejected_reduction_does_not_reset_the_case_count_guard() {
    let bad = TEST.replace("cba", "wrong");
    let reduced = r#"const cases = [{args:{},expect:{error:true}}];"#;
    let (ep, _) = chat_endpoint(vec![
        reply_with(manifest("persistent_guard"), CODE, &bad),
        reply_with(manifest("persistent_guard"), CODE, reduced),
    ])
    .await;
    let d = generate(
        &request_for(ep, &[]),
        &ToolRegistry::new(),
        &AtomicBool::new(false),
    )
    .await
    .unwrap();
    assert!(!d.report.passed);
    assert!(d
        .report
        .error
        .as_deref()
        .unwrap()
        .contains("decreased from 2 to 1"));
    discard(&d.id);
}

#[tokio::test]
async fn truncated_raw_blocks_preserve_the_previous_failed_draft() {
    let bad = TEST.replace("cba", "wrong");
    let (ep, _) = chat_endpoint(vec![
        reply_with(manifest("cut_blocks"), CODE, &bad),
        format!(
            "```json\n{}\n```\n```js index.js\n{CODE}",
            manifest("cut_blocks")
        ),
    ])
    .await;
    let d = generate(
        &request_for(ep, &[]),
        &ToolRegistry::new(),
        &AtomicBool::new(false),
    )
    .await
    .unwrap();
    assert_eq!(d.code, CODE);
    assert_eq!(d.test, bad);
    assert_eq!(d.attempts as usize, 1 + MAX_RETRIES);
    assert_eq!(
        key_of(d.report.error.as_deref().unwrap()),
        "create.model_reply_truncated"
    );
    assert!(!d.report.passed);
    discard(&d.id);
}

#[test]
fn model_manifest_network_urls_normalize_before_bounds() {
    let mut m = manifest("api_lookup");
    m["permissions"]["network"] = json!(["https://user:secret@API.Example.com:8443/v1"]);
    let allowed = Permissions {
        network: vec!["api.example.com".into()],
        ..Default::default()
    };
    let legacy = reply_with(m.clone(), CODE, TEST);
    let fenced =
        format!("```json\n{m}\n```\n```js index.js\n{CODE}\n```\n```js test.js\n{TEST}\n```");
    for reply in [legacy, fenced] {
        let files = parse_reply(&reply, &allowed).unwrap();
        assert_eq!(files.manifest.permissions.network, ["api.example.com"]);
        assert_eq!(
            package::parse_manifest(&files.manifest_json).unwrap(),
            files.manifest
        );
        assert!(!files.manifest_json.contains("secret"));
        assert_eq!(
            key_of(&parse_reply(&reply, &Permissions::default()).unwrap_err()),
            "tools.generator.permissions_exceeded"
        );
    }
    for value in [
        "https://*.example.com/",
        "https://127.0.0.1/",
        "https://localhost/",
        "http://[::1]/",
        "ftp://api.example.com/",
    ] {
        m["permissions"]["network"] = json!([value]);
        assert!(
            parse_reply(&reply_with(m.clone(), CODE, TEST), &allowed).is_err(),
            "{value}"
        );
    }
    m["permissions"]["network"] = json!(["api.example.com", "https://api.example.com/v1"]);
    assert!(parse_reply(&reply_with(m, CODE, TEST), &allowed).is_err());
}

#[tokio::test]
async fn direct_usage_is_attributed_but_gateway_response_is_not_recorded_twice() {
    let direct_client = format!("direct-test-{}", uuid::Uuid::new_v4());
    let gateway_client = format!("gateway-test-{}", uuid::Uuid::new_v4());
    let response = json!({"choices":[{"message":{"content":"ok"}}],"usage":{"prompt_tokens":12,"completion_tokens":3}});
    let (ep, _) = chat_endpoint_responses(vec![response.clone()]).await;
    ask(
        &ep,
        &[json!({"role":"user","content":"hi"})],
        &AtomicBool::new(false),
        &direct_client,
    )
    .await
    .unwrap();
    let (ep, _) = chat_endpoint_responses(vec![json!({"_gateway":true,"choices":[{"message":{"content":"ok"}}],"usage":{"prompt_tokens":12,"completion_tokens":3}})]).await;
    ask(
        &ep,
        &[json!({"role":"user","content":"hi"})],
        &AtomicBool::new(false),
        &gateway_client,
    )
    .await
    .unwrap();
    let summary = crate::analytics::summary(None, None).unwrap();
    let row = summary
        .clients
        .iter()
        .find(|c| c.client == direct_client)
        .unwrap();
    assert_eq!(
        (row.requests, row.prompt_tokens, row.completion_tokens),
        (1, 12, 3)
    );
    assert!(!summary.clients.iter().any(|c| c.client == gateway_client));
}

#[test]
fn normalized_model_urls_do_not_widen_exact_or_wildcard_permissions() {
    for (url, allowed, accepted) in [
        (
            "https://api.example.com:8443/private?q=1",
            "api.example.com",
            true,
        ),
        ("https://api.example.com/private", "*.example.com", true),
        ("https://example.com/private", "*.example.com", false),
        ("https://evil.example.net/private", "*.example.com", false),
    ] {
        let mut m = manifest("network_bounds");
        m["permissions"]["network"] = json!([url]);
        let result = parse_reply(&reply_with(m, CODE, TEST), &perms(&[allowed], &[], &[]));
        assert_eq!(result.is_ok(), accepted, "{url} under {allowed}");
    }
}

#[test]
fn code_comparison_preserves_literal_whitespace() {
    assert_ne!(
        normalized_code("function run(){return 'a  b'}"),
        normalized_code("function run(){return 'a b'}")
    );
}

#[tokio::test]
async fn successful_test_repair_never_warns_about_repeated_code() {
    let code = "function run(a){return a.x}";
    let bad = "const cases=[{args:{x:1},expect:{equals:2}},{args:{x:2},expect:{equals:3}}];";
    let good = "const cases=[{args:{x:1},expect:{equals:1}},{args:{x:2},expect:{equals:2}}];";
    let (ep, _) = chat_endpoint(vec![
        reply_with(manifest("test_repair"), code, bad),
        reply_with(manifest("test_repair"), code, good),
    ])
    .await;
    let draft = generate(
        &request_for(ep, &[]),
        &ToolRegistry::new(),
        &AtomicBool::new(false),
    )
    .await
    .unwrap();
    assert!(draft.report.passed);
    assert_eq!(draft.history.len(), 2);
    assert_eq!(draft.history[1].total_cases, 2);
    assert!(!draft.history[1].repeated);
    discard(&draft.id);
}

fn mismatch(name: &str, expected: &str, actual: &str) -> package_test::CaseReport {
    package_test::CaseReport {
        name: name.into(),
        passed: false,
        reason: "equals".into(),
        expected: expected.into(),
        actual: actual.into(),
        unicode_input: false,
    }
}

fn failing_report(cases: Vec<package_test::CaseReport>) -> TestReport {
    TestReport {
        passed: false,
        cases,
        error: None,
        warnings: Vec::new(),
    }
}

#[test]
fn contradiction_needs_same_actual_with_changed_code() {
    let report = failing_report(vec![
        mismatch("vowels", "4", "5"),
        mismatch("other", "x", "y"),
    ]);
    let previous = vec![("vowels".to_string(), "5".to_string())];
    let found = detect_suspect(&previous, &report, true).unwrap();
    assert_eq!(found.case, "vowels");
    assert_eq!(found.expected, "4");
    assert_eq!(found.actual, "5");
    // Same code again is a plain repetition, not a contradiction.
    assert!(detect_suspect(&previous, &report, false).is_none());
    // A different actual value means the code is still moving.
    let moved = vec![("vowels".to_string(), "6".to_string())];
    assert!(detect_suspect(&moved, &report, true).is_none());
    // Crashes and malformed cases never count.
    let mut crashed = report.clone();
    crashed.cases[0].reason = "unexpected_error".into();
    assert!(detect_suspect(&previous, &crashed, true).is_none());
    assert_eq!(
        mismatch_outputs(&report),
        vec![("vowels".into(), "5".into()), ("other".into(), "y".into())]
    );
    assert!(side_by_side(&report).contains("test expects \"4\" | code returned \"5\""));
}

#[tokio::test]
async fn a_wrong_test_expectation_is_fixed_by_the_model() {
    let code = "function run(){return '3'}";
    let wrong = "const cases=[{name:'a',args:{},expect:{equals:'2'}},{name:'b',args:{x:1},expect:{equals:'3'}}];";
    let right = "const cases=[{name:'a',args:{},expect:{equals:'3'}},{name:'b',args:{x:1},expect:{equals:'3'}}];";
    let (ep, seen) = chat_endpoint(vec![
        reply_with(manifest("wrong_test"), code, wrong),
        reply_with(manifest("wrong_test"), code, right),
    ])
    .await;
    let draft = generate(
        &request_for(ep, &[]),
        &ToolRegistry::new(),
        &AtomicBool::new(false),
    )
    .await
    .unwrap();
    assert!(draft.report.passed);
    assert!(draft.suspect.is_none());
    assert_eq!(draft.test, right);
    let msg = last_user_message(&seen);
    assert!(msg.contains("test expects \"2\" | code returned \"3\""));
    assert!(msg.contains("code AND the test expectation may be wrong"));
    discard(&draft.id);
}

#[tokio::test]
async fn a_wrong_code_is_fixed_by_the_model() {
    let test = "const cases=[{name:'a',args:{},expect:{equals:'3'}},{name:'b',args:{x:1},expect:{equals:'3'}}];";
    let (ep, seen) = chat_endpoint(vec![
        reply_with(manifest("wrong_code"), "function run(){return '2'}", test),
        reply_with(manifest("wrong_code"), "function run(){return '3'}", test),
    ])
    .await;
    let draft = generate(
        &request_for(ep, &[]),
        &ToolRegistry::new(),
        &AtomicBool::new(false),
    )
    .await
    .unwrap();
    assert!(draft.report.passed);
    assert!(draft.suspect.is_none());
    assert!(last_user_message(&seen).contains("test expects \"3\" | code returned \"2\""));
    discard(&draft.id);
}

#[tokio::test]
async fn stable_code_against_a_fixed_wrong_test_is_reported_as_suspect() {
    let test = "const cases=[{name:'a',args:{},expect:{equals:'2'}},{name:'b',args:{x:1},expect:{equals:'3'}}];";
    let package = |code: &str| reply_with(manifest("stuck_test"), code, test);
    let (ep, seen) = chat_endpoint(vec![
        package("function run(){return '3'}"),
        package("function run(){ return '3' }"),
        package("function run(){ const v = '3'; return v }"),
        package("function run(){ const v = '3'; return v + '' }"),
        package("function run(){ return '' + '3' }"),
    ])
    .await;
    let draft = generate(
        &request_for(ep, &[]),
        &ToolRegistry::new(),
        &AtomicBool::new(false),
    )
    .await
    .unwrap();
    assert!(!draft.report.passed);
    let suspect = draft.suspect.clone().expect("suspect");
    assert_eq!(suspect.case, "a");
    assert_eq!(suspect.expected, "2");
    assert_eq!(suspect.actual, "3");
    assert!(seen
        .lock()
        .unwrap()
        .iter()
        .any(|r| r["messages"].to_string().contains("SUSPECT TEST")));
    discard(&draft.id);
}

#[test]
fn a_context_overflow_from_the_model_is_a_keyed_error() {
    let raw = "HTTP 400: request (4273 tokens) exceeds the available context size (4096 tokens), try increasing it";
    assert_eq!(context_exceeded(raw), Some((4273, 4096)));
    assert_eq!(context_exceeded("HTTP 500: boom"), None);
}

#[test]
fn entire_internet_is_an_explicit_generation_upper_bound() {
    let none = perms(&[], &[], &[]);
    let any = perms(&["*"], &[], &[]);
    let listed = perms(&["example.com", "*.wikipedia.org"], &[], &[]);
    for asked in [&none, &any, &listed] {
        assert!(within_bounds(asked, &any).is_ok());
    }
    assert!(within_bounds(&any, &none).is_err());
    assert!(within_bounds(&any, &listed).is_err());
    assert!(within_bounds(&listed, &none).is_err());
    assert!(within_bounds(&perms(&["other.net"], &[], &[]), &listed).is_err());
}

#[test]
fn entire_internet_manifest_is_normalized_and_checked_against_user_scope() {
    let mut m = manifest("public_lookup");
    m["permissions"]["network"] = json!(["*"]);
    let reply = reply_with(m, CODE, TEST);
    assert!(parse_reply(&reply, &perms(&[], &[], &[])).is_err());
    assert!(parse_reply(&reply, &perms(&["example.com"], &[], &[])).is_err());
    let parsed = parse_reply(&reply, &perms(&["*"], &[], &[])).unwrap();
    assert_eq!(parsed.manifest.permissions.network, ["*"]);
}
