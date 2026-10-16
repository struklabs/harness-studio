use super::*;
fn proposal(name: &str) -> Proposal {
    Proposal {
        name: name.into(),
        description: format!("Unique purpose {name}"),
        category: "other".into(),
        permissions: Default::default(),
    }
}
fn item(name: &str) -> QueueItem {
    QueueItem {
        id: name.into(),
        proposal: proposal(name),
        status: ItemStatus::Queued,
        usage: Default::default(),
        history: vec![],
        draft: None,
        error: None,
    }
}
#[test]
fn categories_validate_and_default() {
    let mut v = json!({"name":"sample", "version":"1.0.0", "description":"description", "input_schema":{"type":"object"}, "output":"text", "runtime":"js", "entry":"index.js"});
    assert_eq!(
        package::parse_manifest(&v.to_string()).unwrap().category,
        "other"
    );
    for cat in package::CATEGORIES {
        v["category"] = json!(cat);
        assert!(package::parse_manifest(&v.to_string()).is_ok());
    }
    v["category"] = json!("unknown");
    assert!(package::parse_manifest(&v.to_string()).is_err());
}
#[test]
fn strict_ideation_validates_and_deduplicates() {
    let old = InventoryTool {
        name: "slugify".into(),
        description: "Convert input text into a URL friendly slug".into(),
        category: "text".into(),
        source: ToolSource::Builtin,
        server: None,
    };
    let mut p = proposal("url_slug");
    p.description = old.description.clone();
    let reply =
        json!({"tools":[proposal("slugify"), p, proposal("csv_parse"), proposal("csv_parse")]})
            .to_string();
    assert_eq!(
        parse_proposals(&reply, &[old], 8).unwrap().accepted.len(),
        1
    );
    let wrapped = format!(
        "Here are the tools:\n```json\n{}\n```\nDone",
        json!({"tools":[proposal("csv_parse")]})
    );
    assert_eq!(
        parse_proposals(&wrapped, &[], 8).unwrap().accepted[0].name,
        "csv_parse"
    );
    assert!(parse_proposals("```json {} ```", &[], 8).is_err());
    assert!(parse_proposals(
        &json!({"tools":[{"name":"bad","description":"x","permissions":{"shell":true}}]})
            .to_string(),
        &[],
        8
    )
    .is_err());
    assert!(parse_proposals(
        &json!({"tools":[{"name":"bad","description":"x","category":"wrong"}]}).to_string(),
        &[],
        8
    )
    .is_err());
    assert!(parse_proposals(&json!({"tools":[]}).to_string(), &[], 8).is_err());
}
#[test]
fn ideation_example_is_not_a_suggestion_and_placeholder_names_are_rejected() {
    let prompt = ideation_prompt(3, &[]).unwrap();
    assert!(prompt.contains("\"name\":\"<tool_name>\""));
    assert!(prompt.contains("\"description\":\"<one precise sentence>\""));
    assert!(prompt.contains("do not copy the example"));
    assert!(prompt.contains("do not impose impossible constraints"));
    assert!(!prompt.contains("snake_case"));
    assert!(!prompt.contains("camelCase"));
    for name in [
        "<tool_name>",
        "tool_name",
        "toolname",
        "name",
        "placeholder",
        "example_tool",
        "your_tool_name",
    ] {
        let reply = json!({"tools":[proposal("csv_parse"), proposal(name)]}).to_string();
        assert!(parse_proposals(&reply, &[], 8).is_err(), "{name}");
    }
    // Real transformations remain valid; only schema placeholders are forbidden.
    assert!(parse_proposals(
        &json!({"tools":[proposal("snake_case")]}).to_string(),
        &[],
        8
    )
    .is_ok());
}
#[test]
fn order_and_recovery_preserve_finished_items() {
    let mut s = RunState {
        queue: vec![item("aa"), item("bb"), item("cc")],
        status: RunStatus::Running,
        ..Default::default()
    };
    s.queue[0].status = ItemStatus::Installed;
    s.reorder(&["cc".into(), "bb".into()]).unwrap();
    assert_eq!(
        s.queue.iter().map(|q| q.id.as_str()).collect::<Vec<_>>(),
        ["aa", "cc", "bb"]
    );
    assert!(s.reorder(&["bb".into(), "bb".into()]).is_err());
    s.queue[1].status = ItemStatus::Building;
    s.recover();
    assert_eq!(s.status, RunStatus::Paused);
    assert_eq!(s.queue[0].status, ItemStatus::Installed);
    assert_eq!(s.queue[1].status, ItemStatus::Queued);
}
#[test]
fn no_external_permission_is_auto_installed() {
    let mut p = package::Permissions::default();
    assert!(!needs_approval(&p));
    p.network.push("example.com".into());
    assert!(needs_approval(&p));
    p.network = vec!["*".into()];
    assert!(needs_approval(&p));
    p.network.clear();
    p.fs.read.push(".".into());
    assert!(needs_approval(&p));
    p.fs.read.clear();
    p.fs.write.push("out".into());
    assert!(needs_approval(&p));
}
#[test]
fn persisted_queue_round_trip_and_actions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("run.json");
    let worker = AutoGenerator::open(path.clone(), dir.path().join("tools")).unwrap();
    worker
        .change(|s| {
            s.queue = vec![item("aa"), item("bb")];
            s.status = RunStatus::Running;
            AutoGenerator::log(s, "queue", "two tools");
            Ok(())
        })
        .unwrap();
    worker.interrupt(false).unwrap();
    worker.reorder(&["bb".into(), "aa".into()]).unwrap();
    let restored = AutoGenerator::open(path, dir.path().join("tools")).unwrap();
    assert_eq!(worker.snapshot(), restored.snapshot());
    assert!(restored.retry("bb").is_err());
    restored.remove("bb").unwrap();
    assert_eq!(restored.snapshot().queue.len(), 1);
    assert!(restored.approve("aa", ToolService::shared()).is_err());
}

#[test]
fn clear_logs_persists_without_changing_run_and_accepts_new_activity() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("run.json");
    let root = dir.path().join("tools");
    let worker = AutoGenerator::open(path.clone(), root.clone()).unwrap();
    worker
        .change(|s| {
            s.status = RunStatus::Running;
            s.queue = vec![item("pending"), item("finished")];
            s.queue[1].status = ItemStatus::Installed;
            s.inventory = vec![known("existing", "An existing tool")];
            s.usage = generator::TokenUsage {
                prompt_tokens: 123,
                completion_tokens: 45,
                estimated: true,
            };
            AutoGenerator::log(s, "build", "old activity");
            Ok(())
        })
        .unwrap();
    worker.active.store(true, Ordering::SeqCst);
    let mut expected = worker.snapshot();
    expected.logs.clear();
    assert_eq!(worker.clear_logs().unwrap(), expected);
    assert_eq!(worker.snapshot(), expected);
    let persisted: RunState = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(persisted, expected);
    assert!(worker.active.load(Ordering::SeqCst));
    assert!(!worker.cancel.load(Ordering::SeqCst));
    let mut recovered = expected.clone();
    recovered.recover();
    assert_eq!(
        AutoGenerator::open(path.clone(), root).unwrap().snapshot(),
        recovered
    );
    assert_eq!(worker.clear_logs().unwrap(), expected);
    worker
        .change(|s| {
            AutoGenerator::log(s, "build", "new activity");
            Ok(())
        })
        .unwrap();
    let persisted: RunState = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(persisted.logs.len(), 1);
    assert_eq!(persisted.logs[0].message, "new activity");
}

async fn model_server(replies: Vec<String>) -> (Endpoint, tokio::task::JoinHandle<()>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let job = tokio::spawn(async move {
        let mut replies = replies.into_iter();
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            loop {
                let mut chunk = [0u8; 4096];
                let n = stream.read(&mut chunk).await.unwrap();
                if n == 0 {
                    break;
                }
                bytes.extend_from_slice(&chunk[..n]);
                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                    let len: usize = headers
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length: "))
                        .unwrap_or("0")
                        .parse()
                        .unwrap();
                    if bytes.len() >= end + 4 + len {
                        break;
                    }
                }
            }
            if !String::from_utf8_lossy(&bytes).starts_with("POST /v1/chat/completions ") {
                stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").await.unwrap();
                continue;
            }
            let Some(reply) = replies.next() else {
                return;
            };
            let last = replies.len() == 0;
            let body = json!({"usage":{"prompt_tokens":100,"completion_tokens":50},"choices":[{"message":{"content":reply}}]}).to_string();
            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
            stream.write_all(response.as_bytes()).await.unwrap();
            if last {
                return;
            }
        }
    });
    (
        Endpoint {
            base_url: format!("http://127.0.0.1:{port}/v1"),
            ..Default::default()
        },
        job,
    )
}
fn package_reply(p: &Proposal, code: &str, test: &str) -> String {
    json!({"manifest":{"name":p.name,"category":p.category,"description":p.description,"input_schema":{"type":"object"},"output":"result","permissions":p.permissions},"index_js":code,"test_js":test}).to_string()
}
#[tokio::test]
async fn worker_retries_installs_safe_and_holds_scoped_drafts_across_restart() {
    let safe = proposal("auto_test_safe");
    let mut scoped = proposal("auto_test_scoped");
    scoped.description = "Read a specific workspace file".into();
    scoped.category = "file".into();
    scoped.permissions.fs.read = vec![".".into()];
    let mut failed = proposal("auto_test_failed");
    failed.description = "Count vowel occurrences in input".into();
    let test =
        "const cases=[{args:{},expect:{equals:'ok'}},{args:{unused:true},expect:{equals:'ok'}}];";
    let (ep, job) = model_server(vec![
        "bad JSON".into(),
        json!({"tools":[safe.clone(),scoped.clone(),failed]}).to_string(),
        package_reply(
            &proposal("wrong_identity"),
            "function run(){return 'ok'}",
            test,
        ),
        package_reply(&safe, "function run(){return 'ok'}", test),
        package_reply(
            &scoped,
            "function run(){return harness.fs.read('file')} ",
            "const cases=[{args:{},expect:{error:true}},{args:{extra:true},expect:{error:true}}];",
        ),
        "bad package".into(),
        "bad package".into(),
        "bad package".into(),
        "bad package".into(),
        "bad package".into(),
    ])
    .await;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("tools");
    let path = dir.path().join("run.json");
    let worker = AutoGenerator::open(path.clone(), root.clone()).unwrap();
    worker
        .change(|s| {
            s.status = RunStatus::Running;
            s.count = 3;
            Ok(())
        })
        .unwrap();
    let service = ToolService::new(
        super::super::ToolRegistry::with_builtin(),
        super::super::LogSink::None,
    );
    let sink: Arc<dyn Fn(RunState) + Send + Sync> = Arc::new(|_| {});
    worker.run(ep, &sink, &service).await.unwrap();
    job.await.unwrap();
    let s = worker.snapshot();
    assert_eq!(s.status, RunStatus::Completed);
    assert_eq!(
        s.queue.iter().map(|q| q.status).collect::<Vec<_>>(),
        [
            ItemStatus::Installed,
            ItemStatus::AwaitingApproval,
            ItemStatus::Failed
        ]
    );
    assert_eq!(s.queue[0].draft.as_ref().unwrap().attempts, 2);
    assert_eq!(
        (
            s.ideation_usage.prompt_tokens,
            s.ideation_usage.completion_tokens
        ),
        (200, 100)
    );
    assert_eq!(
        (s.usage.prompt_tokens, s.usage.completion_tokens),
        (1000, 500)
    );
    for q in &s.queue {
        assert_eq!(
            q.usage.prompt_tokens,
            q.history.iter().map(|a| a.usage.prompt_tokens).sum::<u64>()
        );
    }
    assert!(root.join("auto_test_safe/manifest.json").exists());
    assert!(!root.join("auto_test_scoped").exists());
    let restored = AutoGenerator::open(path, root.clone()).unwrap();
    assert_eq!(s, restored.snapshot());
    assert!(restored.approve(&s.queue[2].id, &service).is_err());
    restored.approve(&s.queue[1].id, &service).unwrap();
    assert!(root.join("auto_test_scoped/manifest.json").exists());
    assert_eq!(
        service
            .registry()
            .get("auto_test_scoped")
            .unwrap()
            .def()
            .class,
        super::super::PermissionClass::Restricted
    );
}

#[test]
fn legacy_run_defaults_all_token_and_history_fields() {
    let old = json!({"version":1,"id":"old","status":"completed","stage":"install","count":1,
        "planned":true,"inventory":[],"queue":[{"id":"old","proposal":proposal("old_tool"),
        "status":"failed","draft":null,"error":"old failure"}],"logs":[],"error":null});
    let state: RunState = serde_json::from_value(old).unwrap();
    assert_eq!(state.usage, generator::TokenUsage::default());
    assert_eq!(state.queue[0].usage, generator::TokenUsage::default());
    assert!(state.queue[0].history.is_empty() && state.historical_attempts.is_empty());
}
#[tokio::test]
async fn cancellation_ends_a_pending_model_request_promptly() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let ep = Endpoint {
        base_url: format!("http://{}/v1", listener.local_addr().unwrap()),
        ..Default::default()
    };
    let dir = tempfile::tempdir().unwrap();
    let worker =
        AutoGenerator::open(dir.path().join("run.json"), dir.path().join("tools")).unwrap();
    worker.launch(ep, 1, false, Arc::new(|_| {})).unwrap();
    let (_stream, _) = tokio::time::timeout(std::time::Duration::from_secs(2), listener.accept())
        .await
        .unwrap()
        .unwrap();
    worker.interrupt(false).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while worker.active.load(Ordering::SeqCst) {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(worker.snapshot().status, RunStatus::Paused);
}

#[tokio::test]
async fn stopping_during_a_sandbox_test_releases_the_worker_promptly() {
    use std::time::Duration;
    let p = proposal("auto_cancel_test");
    let (ep, job) = model_server(vec![package_reply(
        &p,
        "function run(){while(true){}}",
        "const cases=[{args:{},expect:{error:true}},{args:{extra:true},expect:{error:true}}];",
    )])
    .await;
    let dir = tempfile::tempdir().unwrap();
    let worker =
        AutoGenerator::open(dir.path().join("run.json"), dir.path().join("tools")).unwrap();
    worker
        .change(|s| {
            s.planned = true;
            s.status = RunStatus::Paused;
            s.queue = vec![item(&p.name)];
            Ok(())
        })
        .unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    worker
        .launch(
            ep,
            1,
            true,
            Arc::new(move |s| {
                let _ = tx.send(s);
            }),
        )
        .unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while let Some(s) = rx.recv().await {
            if s.queue[0].status == ItemStatus::Testing {
                break;
            }
        }
    })
    .await
    .unwrap();
    // Let the blocking engine enter the endless run(), then request Durdur.
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(worker.active.load(Ordering::SeqCst));
    worker.interrupt(true).unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while worker.active.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    job.await.unwrap();
    let s = worker.snapshot();
    assert_eq!(s.status, RunStatus::Stopped);
    assert_eq!(s.queue[0].status, ItemStatus::Queued);
    assert!(s.queue[0].draft.is_none());
    assert!(!dir.path().join("tools/auto_cancel_test").exists());
}

fn draft(name: &str) -> generator::Draft {
    let m = package::parse_manifest(
        &json!({
            "name":name,"category":"text","version":"0.1.0","description":"Test tool",
            "input_schema":{"type":"object"},"output":"text","runtime":"js",
            "entry":"index.js","test":"test.js"
        })
        .to_string(),
    )
    .unwrap();
    generator::Draft {
        id: uuid::Uuid::new_v4().to_string(),
        info: installed::InstalledTool::from_manifest(&m, None, true),
        manifest_json: serde_json::to_string(&m).unwrap(),
        code: "function run(){return 'ok'}".into(),
        test: "const cases=[{args:{},expect:{equals:'ok'}},{args:{unused:true},expect:{equals:'ok'}}];".into(),
        report: super::super::package_test::TestReport {
            passed: true,
            cases: vec![],
            error: None,
            warnings: Vec::new(),
        },
        attempts: 1,
        usage: Default::default(),
        history: vec![],
        suspect: None,
    }
}

#[test]
fn malformed_build_manifest_fails_only_the_item_and_keeps_the_message() {
    let mut q = item("auto_bad_manifest");
    q.status = ItemStatus::Testing;
    let mut d = draft(&q.id);
    d.manifest_json = "broken manifest".into();
    let expected = package::parse_manifest(&d.manifest_json).unwrap_err();
    AutoGenerator::record_draft(&mut q, Ok(d));
    assert_eq!(q.status, ItemStatus::Failed);
    assert_eq!(q.error, Some(expected));
    assert!(q.draft.is_some());
}

#[tokio::test]
async fn malformed_recovered_manifest_does_not_abort_remaining_items() {
    let dir = tempfile::tempdir().unwrap();
    let worker =
        AutoGenerator::open(dir.path().join("run.json"), dir.path().join("tools")).unwrap();
    let mut bad = item("auto_bad_recovered");
    let mut d = draft(&bad.id);
    d.manifest_json = "broken manifest".into();
    let expected = package::parse_manifest(&d.manifest_json).unwrap_err();
    bad.draft = Some(d);
    let mut good = item("auto_good_recovered");
    good.draft = Some(draft(&good.id));
    worker
        .change(|s| {
            s.planned = true;
            s.status = RunStatus::Running;
            s.queue = vec![bad, good];
            Ok(())
        })
        .unwrap();
    let service = ToolService::new(
        super::super::ToolRegistry::with_builtin(),
        super::super::LogSink::None,
    );
    let sink: Arc<dyn Fn(RunState) + Send + Sync> = Arc::new(|_| {});
    worker
        .run(Endpoint::default(), &sink, &service)
        .await
        .unwrap();
    let s = worker.snapshot();
    assert_eq!(s.status, RunStatus::Completed);
    assert_eq!(s.queue[0].status, ItemStatus::Failed);
    assert_eq!(s.queue[0].error, Some(expected.clone()));
    assert_eq!(s.queue[1].status, ItemStatus::Installed);
    worker
        .change(|s| {
            s.queue[0].status = ItemStatus::AwaitingApproval;
            Ok(())
        })
        .unwrap();
    let s = worker.approve("auto_bad_recovered", &service).unwrap();
    assert_eq!(s.queue[0].status, ItemStatus::Failed);
    assert_eq!(s.queue[0].error, Some(expected));
}

#[tokio::test]
#[ignore = "requires a running local model at HARNESS_TEST_MODEL_URL"]
async fn e2e_approval_path() {
    use crate::compaction::Endpoint;
    use crate::tools::{
        package, types::PermissionClass, types::ToolInvocation, LogSink, RunOptions, ToolOrigin,
        ToolRegistry, ToolService,
    };
    use serde_json::json;
    use std::sync::Arc;

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("tools");
    let run_path = dir.path().join("run.json");
    let worker = AutoGenerator::open(run_path.clone(), root.clone()).unwrap();

    let service = ToolService::new(ToolRegistry::with_builtin(), LogSink::None);

    let url = std::env::var("HARNESS_TEST_MODEL_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:11441/v1".into());
    let model = std::env::var("HARNESS_TEST_MODEL")
        .unwrap_or_else(|_| "Qwen3-4B-Instruct-2507-Q4_K_M".into());
    let ep = Endpoint {
        base_url: url,
        model: Some(model),
        ..Default::default()
    };

    let proposal = Proposal {
        name: "wikipedia_summary".into(),
        description: "Return the summary of a Wikipedia article title using the REST API".into(),
        category: "web".into(),
        permissions: package::Permissions {
            network: vec!["*.wikipedia.org".into()],
            ..Default::default()
        },
    };

    worker
        .change(|s| {
            s.planned = true;
            s.queue = vec![QueueItem {
                id: "test1".into(),
                proposal: proposal.clone(),
                status: ItemStatus::Queued,
                usage: Default::default(),
                history: vec![],
                draft: None,
                error: None,
            }];
            s.status = RunStatus::Paused;
            Ok(())
        })
        .unwrap();

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let sink: Arc<dyn Fn(crate::tools::automatic::RunState) + Send + Sync> = Arc::new(move |s| {
        let _ = tx.send(s);
    });

    worker.launch(ep, 1, true, sink).unwrap();

    let mut final_state = None;
    while let Some(s) = rx.recv().await {
        if matches!(
            s.status,
            RunStatus::Completed | RunStatus::Failed | RunStatus::Stopped
        ) {
            final_state = Some(s);
            break;
        }
    }
    let state = final_state.unwrap();

    let item = &state.queue[0];
    if item.status == ItemStatus::Failed {
        println!("Test failed to generate a passing tool after built-in retries.");
        if let Some(d) = &item.draft {
            println!("Attempts: {}", d.attempts);
            println!("Draft test failed: {}", d.report.feedback());
            println!("Draft code:\n{}", d.code);
            println!("Draft test code:\n{}", d.test);
        }
        if let Some(err) = &item.error {
            println!("Error: {}", err);
        }

        let attempts_made = item.draft.as_ref().map(|d| d.attempts).unwrap_or(0);
        panic!("Tool generation failed after {} attempts", attempts_made);
    }

    assert_eq!(
        item.status,
        ItemStatus::AwaitingApproval,
        "Tool should be waiting for approval due to network permissions"
    );

    let draft = item.draft.as_ref().expect("Draft must exist");
    let manifest = package::parse_manifest(&draft.manifest_json).unwrap();
    assert_eq!(
        manifest.permissions.network,
        vec!["*.wikipedia.org".to_string()],
        "Permissions should match requested"
    );

    // Approve the tool
    worker.approve("test1", &service).unwrap();

    let final_snapshot = worker.snapshot();
    let item_after = &final_snapshot.queue[0];
    assert_eq!(
        item_after.status,
        ItemStatus::Installed,
        "Tool should be installed after approval"
    );

    let tool_def = service
        .registry()
        .get("wikipedia_summary")
        .expect("Tool should be registered");
    assert_eq!(
        tool_def.def().class,
        PermissionClass::Restricted,
        "Network tool should be restricted"
    );

    let arg_name = manifest
        .input_schema
        .get("required")
        .and_then(|r| r.as_array())
        .and_then(|a| a.first())
        .and_then(|s| s.as_str())
        .unwrap_or("title");

    // Invoke for real
    let inv = ToolInvocation::new(
        "wikipedia_summary",
        json!({arg_name: "Ankara"}),
        ToolOrigin::new("test"),
    );
    let opts = RunOptions {
        approved: true,
        ..Default::default()
    };
    let res = service.run(inv, opts.clone()).await;

    assert!(res.error.is_none(), "Execution failed: {:?}", res.error);
    let output = res.content;
    println!("Tool output for Ankara: {}", output);
    assert!(
        output.contains("Ankara"),
        "Output should contain the article summary"
    );

    // Assert sandbox rejection
    let index_js_path = root.join("wikipedia_summary").join("index.js");
    let malicious_code = r#"
function run() {
    try {
        let res = harness.http.get("https://example.com/");
        return "fetched";
    } catch (e) {
        return "blocked: " + e.message;
    }
}
    "#;
    std::fs::write(&index_js_path, malicious_code).unwrap();
    crate::tools::installed::sync(&root, service.registry());

    let inv2 = ToolInvocation::new(
        "wikipedia_summary",
        json!({arg_name: "Ankara"}),
        ToolOrigin::new("test"),
    );
    let res2 = service.run(inv2, opts).await;
    let output2 = res2.content;
    println!("Malicious tool output: {}", output2);
    assert!(
        output2.contains("blocked:") || output2.contains("denied") || res2.error.is_some(),
        "Should be blocked by sandbox"
    );
}

#[test]
fn failed_test_messages_are_localizable_and_keep_the_draft() {
    let mut q = item("failed_message");
    let mut d = draft(&q.id);
    d.report.passed = false;
    d.report.error = Some(
        err("create.js_invalid_string")
            .with("detail", "unexpected end of string")
            .into(),
    );
    AutoGenerator::record_draft(&mut q, Ok(d));
    assert_eq!(q.status, ItemStatus::Failed);
    let error = q.error.as_ref().unwrap();
    let (key, params) = crate::error_key::parse(error).unwrap();
    assert_eq!(key, "create.tests_failed");
    assert!(params["detail"]
        .as_str()
        .unwrap()
        .contains("create.test_could_not_run"));
    assert!(q.draft.is_some());
}

#[test]
fn ideation_extraction_selects_only_complete_proposal_objects() {
    let good = json!({"tools": [proposal("csv_decode")]}).to_string();
    for text in [
        format!("Örnek: {{}}\n{good}"),
        format!("{good}\nÖrnek: {{\"q\":\"x\"}}"),
    ] {
        let proposals = parse_proposals(&text, &[], 8).unwrap().accepted;
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].name, "csv_decode");
    }
    let cut = &good[..good.len() - 1];
    for text in [
        cut.to_string(),
        r#"{"tools": [{"name": "inner_only"}"#.to_string(),
        format!("{cut}, invalid"),
        format!("{good} {good}"),
    ] {
        assert!(
            generator::json_object(&text, &["tools"]).is_none(),
            "{text}"
        );
        assert!(parse_proposals(&text, &[], 8).is_err());
    }
}
#[tokio::test]
async fn retry_then_resume_preserves_run_for_every_inactive_status() {
    for status in [
        RunStatus::Paused,
        RunStatus::Stopped,
        RunStatus::Failed,
        RunStatus::Completed,
    ] {
        for bulk in [false, true] {
            let name = format!("retry_inactive_{status:?}_{bulk}").to_lowercase();
            let dir = tempfile::tempdir().unwrap();
            let worker =
                AutoGenerator::open(dir.path().join("run.json"), dir.path().join("tools")).unwrap();
            worker
                .change(|s| {
                    s.id = "same-run".into();
                    s.count = 1;
                    s.planned = true;
                    s.status = status;
                    s.error = Some("stale run error".into());
                    let mut failed = item(&name);
                    failed.status = ItemStatus::Failed;
                    failed.error = Some("stale item error".into());
                    failed.draft = Some(draft(&name));
                    let mut installed = item("already_installed");
                    installed.status = ItemStatus::Installed;
                    let mut approval = item("awaiting_permission");
                    approval.status = ItemStatus::AwaitingApproval;
                    approval.draft = Some(draft("awaiting_permission"));
                    s.queue = vec![failed, installed, approval];
                    Ok(())
                })
                .unwrap();
            let before = worker.snapshot();
            let queued = if bulk {
                worker.retry_failed()
            } else {
                worker.retry(&name)
            }
            .unwrap();
            assert_eq!(queued.status, RunStatus::Paused);
            assert!(queued.error.is_none());
            assert_eq!(queued.queue[0].status, ItemStatus::Queued);
            assert!(queued.queue[0].error.is_none() && queued.queue[0].draft.is_none());
            assert_eq!(queued.queue[1..], before.queue[1..]);
            let (endpoint, job) = model_server(vec![package_reply(
                &queued.queue[0].proposal,
                "function run(){return 'ok'}",
                "const cases=[{args:{},expect:{equals:'ok'}},{args:{unused:true},expect:{equals:'ok'}}];",
            )])
            .await;
            let resumed = worker
                .launch(endpoint, queued.count, true, Arc::new(|_| {}))
                .unwrap();
            assert_eq!(resumed.id, before.id);
            assert_eq!(resumed.status, RunStatus::Running);
            tokio::time::timeout(std::time::Duration::from_secs(10), async {
                while worker.active.load(Ordering::SeqCst) {
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            job.await.unwrap();
            let done = worker.snapshot();
            assert_eq!(done.queue[0].status, ItemStatus::Installed);
            assert_eq!(done.queue[1..], before.queue[1..]);
            assert_eq!(done.id, before.id);
        }
    }
}
#[tokio::test]
async fn completed_run_bulk_retry_processes_all_three_items() {
    let dir = tempfile::tempdir().unwrap();
    let worker =
        AutoGenerator::open(dir.path().join("run.json"), dir.path().join("tools")).unwrap();
    worker
        .change(|s| {
            s.planned = true;
            s.status = RunStatus::Completed;
            s.stage = "install".into();
            s.error = Some("old run error".into());
            s.queue = ["retry_first", "retry_second", "retry_third"]
                .into_iter()
                .map(|name| {
                    let mut q = item(name);
                    q.status = ItemStatus::Failed;
                    q.error = Some("Test/name/category validation failed: old failure".into());
                    q.draft = Some(draft(name));
                    q
                })
                .collect();
            Ok(())
        })
        .unwrap();
    worker.retry_failed().unwrap();
    let queued = worker.snapshot();
    assert!(queued.error.is_none(), "retry must clear stale run errors");
    assert!(queued
        .queue
        .iter()
        .all(|q| q.status == ItemStatus::Queued && q.error.is_none() && q.draft.is_none()));
    let (ep, job) = model_server(
        queued
            .queue
            .iter()
            .map(|q| {
                package_reply(
                    &q.proposal,
                    "function run(){return 'ok'}",
                    "const cases=[{args:{},expect:{equals:'ok'}},{args:{unused:true},expect:{equals:'ok'}}];",
                )
            })
            .collect(),
    )
    .await;
    worker.launch(ep, 3, true, Arc::new(|_| {})).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while worker.active.load(Ordering::SeqCst) {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    job.await.unwrap();
    assert_eq!(worker.snapshot().status, RunStatus::Completed);
    assert!(worker
        .snapshot()
        .queue
        .iter()
        .all(|q| q.status == ItemStatus::Installed));
}
#[tokio::test]
async fn individual_retries_join_running_worker_and_resume_completed_queue() {
    let dir = tempfile::tempdir().unwrap();
    let worker =
        AutoGenerator::open(dir.path().join("run.json"), dir.path().join("tools")).unwrap();
    worker
        .change(|s| {
            s.planned = true;
            s.status = RunStatus::Completed;
            s.queue = ["single_first", "single_second", "single_third"]
                .into_iter()
                .map(|name| {
                    let mut q = item(name);
                    q.status = ItemStatus::Failed;
                    q.error = Some("old".into());
                    q
                })
                .collect();
            Ok(())
        })
        .unwrap();
    worker.retry("single_first").unwrap();
    // A completed persisted run with queued work must also resume directly.
    worker
        .change(|s| {
            s.status = RunStatus::Completed;
            Ok(())
        })
        .unwrap();
    let (ep, job) = model_server(
        worker
            .snapshot()
            .queue
            .iter()
            .map(|q| {
                package_reply(
                    &q.proposal,
                    "function run(){return 'ok'}",
                    "const cases=[{args:{},expect:{equals:'ok'}},{args:{unused:true},expect:{equals:'ok'}}];",
                )
            })
            .collect(),
    )
    .await;
    let w = worker.clone();
    let added = Arc::new(AtomicBool::new(false));
    let flag = added.clone();
    worker
        .launch(
            ep,
            3,
            true,
            Arc::new(move |s| {
                if s.queue[0].status == ItemStatus::Building && !flag.swap(true, Ordering::SeqCst) {
                    assert_eq!(w.retry("single_second").unwrap().status, RunStatus::Running);
                    assert_eq!(w.retry("single_third").unwrap().status, RunStatus::Running);
                }
            }),
        )
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while worker.active.load(Ordering::SeqCst) {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    job.await.unwrap();
    assert!(added.load(Ordering::SeqCst));
    assert_eq!(worker.snapshot().status, RunStatus::Completed);
    assert!(worker
        .snapshot()
        .queue
        .iter()
        .all(|q| q.status == ItemStatus::Installed && q.error.is_none()));
}

#[tokio::test]
#[ignore = "requires read-only access to HARNESS_TEST_MODEL_URL and an isolated run copy"]
async fn e2e_bulk_retry_existing_run() {
    let source = std::env::var("HARNESS_RETRY_RUN_COPY").expect("set isolated source copy");
    let dir = std::env::var("HARNESS_RETRY_DATA_DIR").expect("set isolated output directory");
    let dir = PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("run.json");
    std::fs::copy(source, &path).unwrap();
    let worker = AutoGenerator::open(path, dir.join("tools")).unwrap();
    let before = worker.snapshot();
    let failed: Vec<_> = before
        .queue
        .iter()
        .filter(|q| q.status == ItemStatus::Failed)
        .map(|q| q.id.clone())
        .collect();
    assert!(failed.len() >= 5);
    worker.retry_failed().unwrap();
    assert!(worker
        .snapshot()
        .queue
        .iter()
        .filter(|q| failed.contains(&q.id))
        .all(|q| q.status == ItemStatus::Queued && q.error.is_none()));
    worker
        .change(|s| {
            s.status = RunStatus::Running;
            Ok(())
        })
        .unwrap();
    let service = ToolService::new(
        super::super::ToolRegistry::with_builtin(),
        super::super::LogSink::None,
    );
    let sink: Arc<dyn Fn(RunState) + Send + Sync> = Arc::new(|s| {
        if let Some(log) = s.logs.last() {
            println!("{}: {}", log.stage, log.message);
        }
    });
    worker
        .run(
            Endpoint {
                base_url: std::env::var("HARNESS_TEST_MODEL_URL").unwrap(),
                model: Some(std::env::var("HARNESS_TEST_MODEL").unwrap()),
                ..Default::default()
            },
            &sink,
            &service,
        )
        .await
        .unwrap();
    let after = worker.snapshot();
    assert_eq!(after.status, RunStatus::Completed);
    for q in &after.queue {
        if failed.contains(&q.id) {
            assert!(after.logs[before.logs.len()..]
                .iter()
                .any(|l| l.message == format!("{} · build:1", q.proposal.name)));
            println!(
                "RESULT {} {:?} attempts={} error={:?}",
                q.proposal.name,
                q.status,
                q.draft.as_ref().map_or(0, |d| d.attempts),
                q.error
            );
        } else {
            assert_eq!(q, before.queue.iter().find(|old| old.id == q.id).unwrap());
        }
    }
}

#[test]
fn ideation_network_urls_are_hosts_and_invalid_scopes_stay_rejected() {
    let reply = |network: Vec<&str>| {
        let mut p = proposal("api_lookup");
        p.permissions.network = network.into_iter().map(str::to_string).collect();
        json!({"tools": [p]}).to_string()
    };
    let result = parse_proposals(
        &reply(vec![
            "https://user:password@API.Example.com:8443/v1/*?q=x#fragment",
        ]),
        &[],
        4,
    )
    .unwrap();
    assert_eq!(result.accepted[0].permissions.network, ["api.example.com"]);
    assert!(parse_proposals(&reply(vec!["*.example.com"]), &[], 4).is_ok());
    let any = parse_proposals(&reply(vec!["*"]), &[], 4).unwrap();
    assert_eq!(any.accepted[0].permissions.network, ["*"]);
    assert!(needs_approval(&any.accepted[0].permissions));
    assert!(parse_proposals(&reply(vec!["*", "example.com"]), &[], 4).is_err());
    for value in [
        "https://*.example.com/v1",
        "https://%2A.example.com/v1",
        "https://127.0.0.1/v1",
        "http://127.1/",
        "http://2130706433/",
        "http://[::1]:80/",
        "https://localhost/v1",
        "https://x.localhost/",
        "127.0.0.1",
        "localhost",
        "ftp://api.example.com/v1",
        "https://",
        "api.example.com/v1",
        "https://example.com:bad/",
    ] {
        assert!(
            parse_proposals(&reply(vec![value]), &[], 4).is_err(),
            "{value}"
        );
    }
    assert!(parse_proposals(
        &reply(vec!["api.example.com", "https://api.example.com/v1"]),
        &[],
        4
    )
    .is_err());
}

fn known(name: &str, description: &str) -> InventoryTool {
    InventoryTool {
        name: name.into(),
        description: description.into(),
        category: "text".into(),
        source: ToolSource::Installed,
        server: None,
    }
}

#[test]
fn equivalence_sees_through_different_names_and_wordings() {
    // Same job, word order and inflection differ.
    assert!(equivalent(
        "word_counter",
        "Counts the words in a text",
        "count_words",
        "Return how many words a string has"
    ));
    assert!(equivalent("csv_to_json", "", "convert_csv_json", ""));
    assert!(equivalent("json_formatter", "", "format_json", ""));
    // Different wording of the same purpose, unrelated names.
    assert!(equivalent(
        "reverse_text",
        "Reverse the characters of a string and return the result",
        "flip_string",
        "Reverses the characters of a string and returns the result"
    ));
    // Genuinely different tools stay apart.
    assert!(!equivalent(
        "csv_parser",
        "Parse CSV into rows",
        "json_parser",
        "Parse JSON into an object"
    ));
    assert!(!equivalent(
        "word_counter",
        "Counts the words in a text",
        "char_counter",
        "Counts the characters in a text"
    ));
    assert!(!equivalent(
        "url_title",
        "Fetch a page title",
        "url_encode",
        "Percent-encode a string"
    ));
}

#[test]
fn ideation_keeps_encoder_and_decoder_as_distinct_tools() {
    // Real TC-34 N=4 reply: the decoder's adjective "encoded" made its description
    // overlap with the encoder, even though the operations are opposite.
    let names = [
        "markdown_to_html",
        "html_to_text",
        "base64_encode",
        "base64_decode",
    ];
    let descriptions = [
        "Convert Markdown text to HTML.",
        "Extract plain text from HTML content.",
        "Encode a string to Base64.",
        "Decode a Base64 encoded string.",
    ];
    let proposals: Vec<_> = names
        .iter()
        .zip(descriptions)
        .map(|(name, description)| {
            let mut p = proposal(name);
            p.description = description.into();
            p
        })
        .collect();
    let reply = json!({"tools": proposals}).to_string();
    let accepted = parse_proposals(&reply, &[], 4).unwrap().accepted;
    assert_eq!(
        accepted.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        names
    );
    for (installed, expected) in [(2, "base64_decode"), (3, "base64_encode")] {
        let existing = [known(names[installed], descriptions[installed])];
        let accepted = parse_proposals(&reply, &existing, 4).unwrap().accepted;
        assert_eq!(accepted.len(), 3);
        assert!(accepted.iter().any(|p| p.name == expected));
        assert!(!accepted.iter().any(|p| p.name == names[installed]));
    }
}

#[test]
fn proposals_equivalent_to_installed_or_earlier_ones_are_dropped() {
    let existing = [known("word_counter", "Counts the words in a text")];
    let a = Proposal {
        name: "count_words".into(),
        description: "Return how many words a string has".into(),
        category: "text".into(),
        permissions: Default::default(),
    };
    let b = Proposal {
        name: "markdown_to_html".into(),
        description: "Render Markdown into HTML".into(),
        category: "text".into(),
        permissions: Default::default(),
    };
    let c = Proposal {
        name: "html_from_markdown".into(),
        description: "Render a Markdown document to HTML markup".into(),
        category: "text".into(),
        permissions: Default::default(),
    };
    let got = parse_proposals(&json!({"tools":[a, b, c]}).to_string(), &existing, 8)
        .unwrap()
        .accepted;
    assert_eq!(
        got.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        ["markdown_to_html"]
    );
}

/// A tool source that comes online only after the first inventory was taken.
struct Late(crate::tools::types::ToolDef);
impl crate::tools::registry::Tool for Late {
    fn def(&self) -> &crate::tools::types::ToolDef {
        &self.0
    }
    fn call<'a>(
        &'a self,
        _args: serde_json::Value,
        _ctx: &'a crate::tools::registry::ToolCtx,
    ) -> futures_util::future::BoxFuture<
        'a,
        Result<crate::tools::types::ToolOutput, crate::tools::types::ToolError>,
    > {
        Box::pin(async { Ok(crate::tools::types::ToolOutput::text("ok")) })
    }
}

#[tokio::test]
async fn late_connecting_tools_enter_the_inventory_and_block_duplicates() {
    use crate::tools::{types::PermissionClass, ToolRegistry, ToolSettings};
    let dir = tempfile::tempdir().unwrap();
    let service = ToolService::new(ToolRegistry::with_builtin(), crate::tools::LogSink::None);
    let settings = ToolSettings::default();
    let before = refresh_inventory(&service, dir.path(), &settings).await;
    assert!(!before.iter().any(|t| t.name == "summarize_document"));
    // The MCP server connects after the run started.
    service
        .registry()
        .register(std::sync::Arc::new(Late(crate::tools::types::ToolDef {
            name: "summarize_document".into(),
            description: "Summarize a long document into a short abstract".into(),
            input_schema: json!({"type":"object"}),
            class: PermissionClass::Safe,
            source: ToolSource::Mcp,
            uses_workspace: false,
        })));
    let after = refresh_inventory(&service, dir.path(), &settings).await;
    assert!(after.iter().any(|t| t.name == "summarize_document"));
    // A proposal judged against the first inventory slips through; against the fresh one it does not.
    let reply = json!({"tools":[{"name":"document_summary","description":"Summarise a long document into a brief abstract","category":"text"}]}).to_string();
    assert!(parse_proposals(&reply, &before, 4).is_ok());
    assert!(parse_proposals(&reply, &after, 4).is_err());
}

fn permission_draft(
    name: &str,
    category: &str,
    permissions: serde_json::Value,
    code: &str,
    test: &str,
) -> generator::Draft {
    let m = package::parse_manifest(
        &json!({
            "name":name,"category":category,"version":"0.1.0","description":"Needs approval",
            "input_schema":{"type":"object","properties":{"target":{"type":"string"}}},
            "output":"text","runtime":"js","entry":"index.js","test":"test.js",
            "permissions":permissions
        })
        .to_string(),
    )
    .unwrap();
    let mut d = draft(name);
    d.info = installed::InstalledTool::from_manifest(&m, None, true);
    d.manifest_json = serde_json::to_string(&m).unwrap();
    d.code = code.into();
    d.test = test.into();
    d
}

/// Real approval path for tools that reach outside: nothing is installed before the human
/// approves; afterwards the package is registered and a real call reaches a local HTTP server
/// and a temporary folder, while the policy keeps asking and the sandbox keeps its scope.
#[tokio::test]
async fn approved_external_tools_install_and_really_run() {
    use crate::tools::js_engine::TRUSTED_TEST_HOSTS;
    use crate::tools::{
        LogSink, RunOptions, ToolInvocation, ToolOrigin, ToolPolicy, ToolRegistry, ToolSettings,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    TRUSTED_TEST_HOSTS
        .lock()
        .unwrap()
        .push(format!("127.0.0.1:{port}"));
    tokio::spawn(async move {
        while let Ok((mut sock, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = vec![0u8; 2048];
                let _ = sock.read(&mut buf).await;
                let body = "pong from the fake server";
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(reply.as_bytes()).await;
                let _ = sock.shutdown().await;
            });
        }
    });

    let dir = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(work.path().join("notes")).unwrap();
    std::fs::write(work.path().join("notes/a.txt"), "inside the granted folder").unwrap();
    std::fs::write(work.path().join("secret.txt"), "outside").unwrap();
    let root = dir.path().join("tools");
    let worker = AutoGenerator::open(dir.path().join("run.json"), root.clone()).unwrap();
    let service = ToolService::new(ToolRegistry::with_builtin(), LogSink::None);

    let net = permission_draft(
        "ping_server",
        "web",
        json!({"network":["127.0.0.1"]}),
        "function run(a){ const r = harness.http.get(a.target); return r.status + ':' + r.body; }",
        "const cases=[{args:{target:'http://127.0.0.1/'},http:[{url:'http://127.0.0.1/',status:200,body:'x'}],expect:{equals:'200:x'}},{args:{target:'http://127.0.0.1/'},http:[{url:'http://127.0.0.1/',status:200,body:'y'}],expect:{contains:'y'}}];",
    );
    let fs = permission_draft(
        "read_note",
        "file",
        json!({"fs":{"read":["notes"]}}),
        "function run(a){ return harness.fs.read(a.target); }",
        "const cases=[{args:{target:'notes/none.txt'},expect:{error:true}},{args:{target:'../x'},expect:{error:true}}];",
    );
    worker
        .change(|s| {
            s.planned = true;
            s.status = RunStatus::Paused;
            s.queue = [net, fs]
                .into_iter()
                .map(|d| {
                    let mut q = item(&d.info.name);
                    q.proposal.category = d.info.category.clone();
                    q.draft = Some(d);
                    q.status = ItemStatus::AwaitingApproval;
                    q
                })
                .collect();
            Ok(())
        })
        .unwrap();
    // Nothing is installed before the human decides.
    assert!(service.registry().get("ping_server").is_none());
    assert!(service.registry().get("read_note").is_none());

    worker.approve("ping_server", &service).unwrap();
    worker.approve("read_note", &service).unwrap();
    let state = worker.snapshot();
    for q in &state.queue {
        assert_eq!(q.status, ItemStatus::Installed, "{:?}", q.error);
    }
    assert!(root.join("ping_server/manifest.json").is_file());

    let opts = |approved| RunOptions {
        workdir: Some(work.path().to_path_buf()),
        approved,
        settings: Some(ToolSettings {
            policy: ToolPolicy::AskEveryTime,
            ..Default::default()
        }),
        ..Default::default()
    };
    let call = |tool: &str, target: String| {
        ToolInvocation::new(tool, json!({"target": target}), ToolOrigin::new("test"))
    };
    let url = format!("http://127.0.0.1:{port}/ping");
    // Installed does not mean trusted: the policy still asks before an unapproved call.
    let asked = service
        .run(call("ping_server", url.clone()), opts(false))
        .await;
    assert!(asked.error.is_some(), "{}", asked.content);
    let ok = service
        .run(call("ping_server", url.clone()), opts(true))
        .await;
    assert_eq!(ok.error, None, "{}", ok.content);
    assert_eq!(ok.content, "200:pong from the fake server");
    // A host that is not in the manifest stays blocked.
    let other = format!("http://localhost:{port}/ping");
    let blocked = service.run(call("ping_server", other), opts(true)).await;
    assert!(blocked.error.is_some(), "{}", blocked.content);

    let read = service
        .run(call("read_note", "notes/a.txt".into()), opts(true))
        .await;
    assert_eq!(read.error, None, "{}", read.content);
    assert_eq!(read.content, "inside the granted folder");
    for outside in ["secret.txt", "../secret.txt"] {
        let r = service
            .run(call("read_note", outside.into()), opts(true))
            .await;
        assert!(r.error.is_some(), "{outside}: {}", r.content);
    }
}

#[test]
fn retry_keeps_accumulated_usage_across_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("run.json");
    let worker = AutoGenerator::open(path.clone(), dir.path().join("tools")).unwrap();
    let usage = generator::TokenUsage {
        prompt_tokens: 123,
        completion_tokens: 45,
        estimated: true,
    };
    worker
        .change(|s| {
            s.status = RunStatus::Completed;
            s.usage = usage.clone();
            let mut q = item("retry_usage");
            q.status = ItemStatus::Failed;
            q.usage = usage.clone();
            q.history.push(generator::Attempt {
                number: 1,
                usage: usage.clone(),
                ..Default::default()
            });
            s.queue = vec![q];
            Ok(())
        })
        .unwrap();
    let before = worker.snapshot();
    worker.retry_failed().unwrap();
    let restored = AutoGenerator::open(path, dir.path().join("tools"))
        .unwrap()
        .snapshot();
    assert_eq!(restored.usage, before.usage);
    assert_eq!(restored.queue[0].usage, before.queue[0].usage);
    assert_eq!(restored.queue[0].history, before.queue[0].history);
    assert_eq!(restored.queue[0].status, ItemStatus::Queued);
}

#[test]
fn duplicates_are_reported_with_the_tool_they_matched() {
    let existing = [known("word_counter", "Counts the words in a text")];
    let dup = Proposal {
        name: "count_words".into(),
        description: "Return how many words a string has".into(),
        category: "text".into(),
        permissions: Default::default(),
    };
    let reply = json!({"tools":[dup, proposal("csv_parse")]}).to_string();
    let parsed = parse_proposals(&reply, &existing, 8).unwrap();
    assert_eq!(parsed.accepted.len(), 1);
    assert_eq!(
        parsed.skipped,
        [SkippedProposal {
            name: "count_words".into(),
            matched: "word_counter".into()
        }]
    );
}

#[test]
fn top_up_merges_new_tools_without_duplicating_accepted_ones() {
    let mut first = proposal("csv_parse");
    first.description = "Parse CSV text into rows".into();
    let mut accepted = vec![first.clone()];
    let mut again = proposal("parse_csv");
    again.description = first.description.clone();
    let mut fresh = proposal("slugify");
    fresh.description = "Turn a title into a URL slug".into();
    let reply = json!({"tools":[again, fresh]}).to_string();
    let skipped = merge_proposals(&mut accepted, &reply, &[], 2).unwrap();
    assert_eq!(
        accepted.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        ["csv_parse", "slugify"]
    );
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0].matched, "csv_parse");
    // Never exceeds the requested count.
    let mut full = vec![first];
    let reply = json!({"tools":[proposal("a_tool"), proposal("b_tool")]}).to_string();
    merge_proposals(&mut full, &reply, &[], 2).unwrap();
    assert_eq!(full.len(), 2);
    assert!(top_up_message(1, &full, &skipped).contains("exactly 1 MORE"));
}
