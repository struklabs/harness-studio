//! The coding area end to end with a mocked model: a local endpoint that answers chat
//! completions with scripted tool calls. Nothing else is faked: the real agent loop, tool
//! engine, workspace rules, approvals, checkpoints and undo run against real files.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use harness_core::coding::{
    answer_approval, run_task, ChangeStatus, Event, RunHandle, RunRequest, Store,
};
use harness_core::state::{Project, ProjectSettings};
use harness_core::tools::{CancelToken, LogSink, ToolRegistry, ToolService, ToolSettings};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Serves `replies` (one assistant message per request, in order) and returns the base URL and
/// the request bodies it received.
async fn mock_model(replies: Vec<Value>) -> (String, Arc<Mutex<Vec<Value>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    tokio::spawn(async move {
        for message in replies {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            let body = loop {
                let mut chunk = [0; 8192];
                let n = socket.read(&mut chunk).await.unwrap();
                assert!(n > 0, "request ended early");
                bytes.extend_from_slice(&chunk[..n]);
                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                    let length: usize = headers
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .unwrap()
                        .trim()
                        .parse()
                        .unwrap();
                    if bytes.len() >= end + 4 + length {
                        break serde_json::from_slice::<Value>(&bytes[end + 4..end + 4 + length])
                            .unwrap();
                    }
                }
            };
            log.lock().unwrap().push(body);
            let reply =
                json!({"choices": [{"message": message, "finish_reason": "stop"}]}).to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                reply.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        }
    });
    (base, seen)
}

fn tool_call(id: &str, name: &str, args: Value) -> Value {
    json!({
        "role": "assistant",
        "content": "",
        "tool_calls": [{"id": id, "type": "function", "function": {"name": name, "arguments": args.to_string()}}]
    })
}

fn project(folder: &Path) -> Project {
    Project {
        id: "proj-e2e".into(),
        name: "e2e".into(),
        description: String::new(),
        folder: Some(folder.to_string_lossy().into_owned()),
        settings: ProjectSettings::default(),
        archived: false,
        created_at: 0,
        updated_at: 0,
        chat_count: 0,
        output_count: 0,
    }
}

/// Runs one task against the mock; approvals are answered with `allow`.
async fn run(
    folder: &Path,
    data: &Path,
    replies: Vec<Value>,
    allow: bool,
) -> (
    harness_core::coding::RunSummary,
    Vec<Event>,
    Arc<Mutex<Vec<Value>>>,
) {
    let (base, seen) = mock_model(replies).await;
    let events = Arc::new(Mutex::new(Vec::new()));
    let log = events.clone();
    let sink: harness_core::coding::run::Sink = Arc::new(move |event| {
        if let Event::Approval { approval_id, .. } = &event {
            assert!(answer_approval(approval_id, allow));
        }
        log.lock().unwrap().push(event);
    });
    let service = ToolService::new(ToolRegistry::with_builtin(), LogSink::None);
    let summary = tokio::time::timeout(
        Duration::from_secs(60),
        run_task(
            &service,
            &Store::new(data.to_path_buf()),
            &project(folder),
            RunRequest {
                base_url: base,
                api_key: None,
                model: "mock".into(),
                prompt: "Rename the greeting".into(),
            },
            &ToolSettings::default(),
            RunHandle {
                run_id: "run-1".into(),
                sink,
                cancel: CancelToken::new(),
            },
        ),
    )
    .await
    .expect("run finished")
    .expect("run started");
    let events = events.lock().unwrap().clone();
    (summary, events, seen)
}

fn fixture() -> (tempfile::TempDir, tempfile::TempDir) {
    let (data, work) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    std::fs::create_dir_all(work.path().join("src")).unwrap();
    std::fs::write(
        work.path().join("src/lib.rs"),
        "pub fn greet() -> &'static str {\n    \"hello\"\n}\n",
    )
    .unwrap();
    (data, work)
}

#[tokio::test]
async fn a_model_edit_is_shown_as_a_diff_and_undone() {
    let (data, work) = fixture();
    let lib = work.path().join("src/lib.rs");
    let original = std::fs::read_to_string(&lib).unwrap();

    let (summary, events, seen) = run(
        work.path(),
        data.path(),
        vec![
            tool_call(
                "c1",
                "edit_file",
                json!({"path": "src/lib.rs", "edits": [{"old_text": "\"hello\"", "new_text": "\"hello, world\""}]}),
            ),
            json!({"role": "assistant", "content": "Changed the greeting."}),
        ],
        false,
    )
    .await;

    // The model was offered the coding tools and its call ran through the engine.
    let first = seen.lock().unwrap()[0].clone();
    let offered: Vec<&str> = first["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["function"]["name"].as_str().unwrap())
        .collect();
    assert!(offered.contains(&"edit_file") && offered.contains(&"exec_shell_command"));
    assert_eq!(summary.stop, "answered");
    assert_eq!(summary.text, "Changed the greeting.");
    assert_eq!(
        std::fs::read_to_string(&lib).unwrap(),
        original.replace("hello", "hello, world")
    );

    // The review view: one modified file with a diff.
    assert_eq!(summary.changes.len(), 1);
    let change = &summary.changes[0];
    assert_eq!(change.path, "src/lib.rs");
    assert_eq!(change.status, ChangeStatus::Modified);
    assert!(
        change
            .diff
            .contains("-    \"hello\"\n+    \"hello, world\"\n"),
        "{}",
        change.diff
    );
    assert_eq!(summary.calls.len(), 1);
    assert_eq!(summary.calls[0].tool, "edit_file");

    // Progress events came in order: started, round, call running, call done, ..., finished.
    assert!(matches!(events.first(), Some(Event::Started { .. })));
    assert!(matches!(
        events.last(),
        Some(Event::Finished { changes: 1, .. })
    ));
    assert!(events.iter().any(|e| matches!(
        e,
        Event::Call { tool, status: harness_core::coding::run::CallStatus::Done, .. } if tool == "edit_file"
    )));

    // Undo puts the file back; the checkpoint remembers it was undone.
    let store = Store::new(data.path().to_path_buf());
    let restored = store.undo(&summary.checkpoint, None).unwrap();
    assert_eq!(restored, vec!["src/lib.rs".to_string()]);
    assert_eq!(std::fs::read_to_string(&lib).unwrap(), original);
    assert!(store.changes(&summary.checkpoint).unwrap().is_empty());
    assert!(
        store
            .get("proj-e2e", &summary.checkpoint.id)
            .unwrap()
            .undone
    );
}

#[tokio::test]
async fn edits_outside_the_workspace_and_into_git_are_refused() {
    let (data, work) = fixture();
    std::fs::create_dir_all(work.path().join(".git")).unwrap();
    let outside = work.path().parent().unwrap().join("coding-e2e-escape.txt");
    let (summary, _, seen) = run(
        work.path(),
        data.path(),
        vec![
            tool_call(
                "c1",
                "write_file",
                json!({"path": "../coding-e2e-escape.txt", "content": "x"}),
            ),
            tool_call(
                "c2",
                "write_file",
                json!({"path": ".git/config", "content": "x"}),
            ),
            json!({"role": "assistant", "content": "I could not."}),
        ],
        false,
    )
    .await;
    assert!(!outside.exists());
    assert!(summary.changes.is_empty());
    // The refusals reached the model as tool results it can read.
    let last = seen.lock().unwrap().last().unwrap().clone();
    let tools: Vec<String> = last["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["role"] == "tool")
        .map(|m| m["content"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(tools.len(), 2);
    assert!(tools.iter().all(|t| t.starts_with("Error:")), "{tools:?}");
}

#[tokio::test]
async fn a_command_runs_only_after_approval() {
    let command = "echo made> made.txt";
    for allow in [false, true] {
        let (data, work) = fixture();
        let (summary, events, _) = run(
            work.path(),
            data.path(),
            vec![
                tool_call("c1", "exec_shell_command", json!({"command": command})),
                json!({"role": "assistant", "content": "done"}),
            ],
            allow,
        )
        .await;
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::Approval { summary, .. } if summary == command)),
            "the command was put to the user"
        );
        let made = work.path().join("made.txt");
        assert_eq!(made.exists(), allow);
        if allow {
            assert_eq!(summary.changes.len(), 1);
            assert_eq!(summary.changes[0].status, ChangeStatus::Added);
            Store::new(data.path().to_path_buf())
                .undo(&summary.checkpoint, None)
                .unwrap();
            assert!(!made.exists());
        } else {
            assert!(summary.changes.is_empty());
            assert_eq!(
                summary.calls[0].status,
                harness_core::coding::run::CallStatus::Denied
            );
        }
    }
}
