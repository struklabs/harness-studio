//! The tool engine with a real model: llama-server loads a tool-capable GGUF and
//! `tools::agent::run_agent` drives it through the engine (policy, working directory, limits,
//! log). Nothing is mocked: the model decides what to call.
//!
//! Ignored by default (needs a runtime binary and a model; ports 18950+):
//!
//! HS_LLAMA_SERVER=<llama-server.exe> HS_TEST_MODEL=<tool-capable .gguf> \
//!   cargo test -p harness-core --test tools_e2e -- --ignored --nocapture
//!
//! Developed with Qwen3-4B-Instruct-2507 (Q4_K_M). Prompts are explicit and the temperature is
//! 0, but a model is a model: the assertions are about what the *engine* saw and did (which
//! tool ran, which error code came back, what was sent to the model), not about wording.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use harness_core::analytics::{AnalyticsDb, ToolCallQuery};
use harness_core::tools::agent::{
    run_agent, AgentOutcome, AgentRequest, ApproveAll, Approver, DenyAll, StopReason,
};
use harness_core::tools::{
    LogSink, ToolDef, ToolErrorCode, ToolLimits, ToolOrigin, ToolPolicy, ToolRegistry, ToolService,
    ToolSettings,
};
use serde_json::{json, Value};

struct LlamaServer {
    child: Child,
    base: String,
}

impl Drop for LlamaServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn free_port(from: u16) -> u16 {
    (from..from + 50)
        .find(|p| std::net::TcpListener::bind(("127.0.0.1", *p)).is_ok())
        .expect("a free port")
}

async fn start_server(bin: &Path, model: &Path) -> LlamaServer {
    let port = free_port(18950);
    let child = Command::new(bin)
        .args(["-m"])
        .arg(model)
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .args(["--jinja", "-c", "8192", "-ngl", "99", "-np", "1"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start llama-server");
    let server = LlamaServer {
        child,
        base: format!("http://127.0.0.1:{port}"),
    };
    let http = reqwest::Client::new();
    let deadline = Instant::now() + Duration::from_secs(240);
    loop {
        if let Ok(r) = http.get(format!("{}/health", server.base)).send().await {
            if r.status().is_success() {
                return server;
            }
        }
        assert!(Instant::now() < deadline, "llama-server did not get ready");
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

struct Counting(AtomicUsize, bool);

impl Approver for Counting {
    fn approve<'a>(
        &'a self,
        _: &'a ToolDef,
        _: &'a Value,
    ) -> futures_util::future::BoxFuture<'a, bool> {
        self.0.fetch_add(1, Ordering::SeqCst);
        let answer = self.1;
        Box::pin(async move { answer })
    }
}

struct Rig {
    server: LlamaServer,
    service: ToolService,
    db: Arc<AnalyticsDb>,
    _db_dir: tempfile::TempDir,
}

impl Rig {
    fn request(&self, work: &Path, policy: ToolPolicy, system: &str, user: &str) -> AgentRequest {
        let mut req = AgentRequest::new(
            format!("{}/v1", self.server.base),
            "qwen",
            vec![
                json!({"role": "system", "content": system}),
                json!({"role": "user", "content": user}),
            ],
        );
        req.workdir = Some(work.to_path_buf());
        req.settings = Some(ToolSettings {
            policy,
            trusted: vec![],
            workdir: String::new(),
            ..Default::default()
        });
        req.max_tokens = Some(400);
        req.origin = ToolOrigin::new("e2e")
            .conversation("e2e-conv")
            .model("qwen");
        req
    }

    fn log(&self) -> Vec<String> {
        self.db
            .tool_calls(&ToolCallQuery::default())
            .unwrap()
            .into_iter()
            .rev()
            .map(|r| {
                format!(
                    "{} {} approved={} error={}",
                    r.tool,
                    r.class,
                    r.approved,
                    r.error_code.as_deref().unwrap_or("-")
                )
            })
            .collect()
    }
}

fn show(name: &str, o: &AgentOutcome) {
    eprintln!("\n=== {name}: stop={:?}, rounds={}", o.stop, o.rounds);
    for c in &o.calls {
        let err = c
            .result
            .as_ref()
            .and_then(|r| r.error)
            .map_or("ok", |e| e.as_str());
        let sent: String = c.sent.chars().take(110).collect();
        eprintln!(
            "  r{} {}({}) -> {err}: {sent:?}",
            c.round, c.tool, c.arguments
        );
    }
    eprintln!("  answer: {:?}", o.text);
}

const SYSTEM: &str = "You are a careful assistant with tools for a project folder. \
    Use a tool whenever you need file contents or must run something; never invent contents.";

fn temp_dir() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&harness_core::paths::test_dir_prefix("tools-e2e"))
        .tempdir()
        .unwrap()
}

#[tokio::test]
#[ignore]
async fn a_real_model_drives_the_tool_engine() {
    let (Some(bin), Some(model)) = (
        std::env::var_os("HS_LLAMA_SERVER").map(PathBuf::from),
        std::env::var_os("HS_TEST_MODEL").map(PathBuf::from),
    ) else {
        eprintln!("HS_LLAMA_SERVER / HS_TEST_MODEL not set, skipping");
        return;
    };
    assert!(bin.is_file() && model.is_file());
    let db_dir = temp_dir();
    let db = Arc::new(AnalyticsDb::open(&db_dir.path().join("a.db")).unwrap());
    let rig = Rig {
        server: start_server(&bin, &model).await,
        service: ToolService::new(ToolRegistry::with_builtin(), LogSink::Db(db.clone())),
        db,
        _db_dir: db_dir,
    };

    // 1. One tool call: the model reads a file in the working directory.
    let work = temp_dir();
    std::fs::write(
        work.path().join("note.txt"),
        "The secret code is PLUM-4417.\n",
    )
    .unwrap();
    let o = run_agent(
        &rig.service,
        rig.request(
            work.path(),
            ToolPolicy::AllowAll,
            SYSTEM,
            "Read note.txt and tell me the secret code.",
        ),
        &DenyAll,
    )
    .await
    .unwrap();
    show("single call", &o);
    assert_eq!(o.stop, StopReason::Answered);
    assert_eq!(o.calls[0].tool, "read_file");
    assert!(o.calls[0]
        .result
        .as_ref()
        .unwrap()
        .content
        .contains("PLUM-4417"));
    assert!(o.text.contains("PLUM-4417"), "{}", o.text);

    // 2. Chained calls: find the file with the glob tool, then read it.
    let work = temp_dir();
    std::fs::create_dir_all(work.path().join("docs")).unwrap();
    std::fs::create_dir_all(work.path().join("src")).unwrap();
    std::fs::write(work.path().join("docs/alpha.md"), "token: KIWI-2291\n").unwrap();
    std::fs::write(work.path().join("docs/beta.md"), "token: FIG-0003\n").unwrap();
    std::fs::write(work.path().join("src/main.rs"), "fn main() {}\n").unwrap();
    let o = run_agent(
        &rig.service,
        rig.request(
            work.path(),
            ToolPolicy::AllowAll,
            SYSTEM,
            "First list the markdown files under the docs folder with the file_glob_search tool, \
             then read alpha.md with read_file and tell me its token.",
        ),
        &DenyAll,
    )
    .await
    .unwrap();
    show("chained calls", &o);
    let tools: Vec<&str> = o.calls.iter().map(|c| c.tool.as_str()).collect();
    let glob = tools.iter().position(|t| *t == "file_glob_search");
    let read = tools.iter().rposition(|t| *t == "read_file");
    assert!(
        glob.is_some() && read.is_some() && glob < read,
        "glob should come before read: {tools:?}"
    );
    assert!(o.rounds >= 3, "{}", o.rounds);
    assert!(o.text.contains("KIWI-2291"), "{}", o.text);

    // 3. Bad arguments. llama-server constrains a call to the tool's schema (types and
    //    required names cannot go wrong), so what is left to reject is *values*: a path that
    //    leaves the working directory and a pattern that is not a valid regex. The engine
    //    answers with the error, the model reads it and says so.
    let outer = temp_dir();
    let work = outer.path().join("project");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::write(
        outer.path().join("outside.txt"),
        "outside
",
    )
    .unwrap();
    std::fs::write(
        work.join("a.txt"),
        "apple
",
    )
    .unwrap();
    let o = run_agent(
        &rig.service,
        rig.request(
            &work,
            ToolPolicy::AllowAll,
            SYSTEM,
            "Call read_file with the path ../outside.txt and tell me what the tool answered.",
        ),
        &DenyAll,
    )
    .await
    .unwrap();
    show("path outside the working directory", &o);
    let outside = o
        .calls
        .iter()
        .find(|c| c.result.as_ref().and_then(|r| r.error) == Some(ToolErrorCode::OutsideWorkspace))
        .expect("the escape should be rejected");
    assert!(outside.sent.starts_with("Error: "), "{}", outside.sent);
    assert!(o
        .messages
        .iter()
        .any(|m| m["role"] == "tool" && m["content"].as_str() == Some(outside.sent.as_str())));
    assert!(!o.text.is_empty());
    assert!(!o.text.contains(
        "outside
"
    ));

    let o = run_agent(
        &rig.service,
        rig.request(
            &work,
            ToolPolicy::AllowAll,
            SYSTEM,
            "Use grep_search in the folder . with the pattern ( (a single opening parenthesis)              and literal set to false, and tell me what the tool answered.",
        ),
        &DenyAll,
    )
    .await
    .unwrap();
    show("invalid regex", &o);
    let bad = o
        .calls
        .iter()
        .find(|c| c.tool == "grep_search" && c.result.as_ref().is_some_and(|r| r.is_error()))
        .expect("the pattern should be rejected");
    assert!(bad.sent.starts_with("Error: "), "{}", bad.sent);
    assert!(!o.text.is_empty());

    // 4. Timeout: a command that outlives the call's time limit is stopped.
    let work = temp_dir();
    let sleeper = if cfg!(windows) {
        "ping -n 40 127.0.0.1"
    } else {
        "sleep 40"
    };
    let mut req = rig.request(
        work.path(),
        ToolPolicy::AllowAll,
        SYSTEM,
        &format!("Run exactly this shell command with exec_shell_command and report what happened: {sleeper}"),
    );
    req.call_timeout = Some(Duration::from_secs(2));
    let started = Instant::now();
    let o = run_agent(&rig.service, req, &DenyAll).await.unwrap();
    show("timeout", &o);
    let timed_out = o
        .calls
        .iter()
        .find(|c| c.result.as_ref().and_then(|r| r.error) == Some(ToolErrorCode::Timeout))
        .expect("a call should have timed out");
    assert!(
        timed_out.sent.contains("did not finish within 2 seconds"),
        "{}",
        timed_out.sent
    );
    assert!(started.elapsed() < Duration::from_secs(60));
    assert!(!o.text.is_empty());

    // 5. Approval refusal: the policy asks, nobody approves; the refusal goes back to the
    //    model and nothing is written. Then the same request with an approval writes.
    let work = temp_dir();
    let denier = Counting(AtomicUsize::new(0), false);
    let o = run_agent(
        &rig.service,
        rig.request(
            work.path(),
            ToolPolicy::AskEveryTime,
            SYSTEM,
            "Create a file named hello.txt containing the text hi, using the write_file tool.",
        ),
        &denier,
    )
    .await
    .unwrap();
    show("approval refused", &o);
    let refused = o
        .calls
        .iter()
        .find(|c| c.result.as_ref().and_then(|r| r.error) == Some(ToolErrorCode::ApprovalRequired))
        .expect("the write should have been refused");
    assert_eq!(refused.tool, "write_file");
    assert!(
        refused.sent.contains("needs the user's approval"),
        "{}",
        refused.sent
    );
    assert!(denier.0.load(Ordering::SeqCst) >= 1);
    assert!(!work.path().join("hello.txt").exists());
    assert!(!o.text.is_empty());

    let work = temp_dir();
    let approver = Counting(AtomicUsize::new(0), true);
    let o = run_agent(
        &rig.service,
        rig.request(
            work.path(),
            ToolPolicy::AskEveryTime,
            SYSTEM,
            "Create a file named hello.txt containing the text hi, using the write_file tool.",
        ),
        &approver,
    )
    .await
    .unwrap();
    show("approval granted", &o);
    assert!(approver.0.load(Ordering::SeqCst) >= 1);
    assert!(work.path().join("hello.txt").is_file());
    let _ = ApproveAll; // the stock approvers are part of the API; the counting one is used above

    // 6. Round limit: the files point at each other for longer than the limit allows.
    let work = temp_dir();
    for i in 1..=6 {
        let body = if i < 6 {
            format!("next: n{}.txt\n", i + 1)
        } else {
            "the end\n".to_string()
        };
        std::fs::write(work.path().join(format!("n{i}.txt")), body).unwrap();
    }
    let mut req = rig.request(
        work.path(),
        ToolPolicy::AllowAll,
        SYSTEM,
        "Start at n1.txt. Each file names the next file to read; read them one at a time with \
         read_file until a file says the end, then tell me how many files you read.",
    );
    req.limits = ToolLimits {
        max_rounds: 3,
        ..ToolLimits::default()
    };
    let o = run_agent(&rig.service, req, &DenyAll).await.unwrap();
    show("round limit", &o);
    assert_eq!(o.stop, StopReason::RoundLimit);
    assert_eq!(o.rounds, 3);
    assert!(o.calls.len() >= 3);
    assert!(o.text.is_empty());

    // The tool log has every call of the run, the refusals included.
    let log = rig.log();
    eprintln!("\n=== tool log ({} rows)", log.len());
    for l in &log {
        eprintln!("  {l}");
    }
    assert!(log.iter().any(|l| l.contains("approval_required")));
    assert!(log.iter().any(|l| l.contains("outside_workspace")));
    assert!(log.iter().any(|l| l.contains("error=failed")));
    assert!(log.iter().any(|l| l.contains("timeout")));
    assert!(log
        .iter()
        .any(|l| l.starts_with("write_file dangerous approved=true")));
}

// ---------------------------------------------------------------------------------------------
// The streaming loop (T1c)
// ---------------------------------------------------------------------------------------------

use futures_util::future::BoxFuture;
use harness_core::tools::agent_stream::{
    run_agent_stream, AgentEvent, HttpEndpoint, RoundHook, StreamOutcome, StreamRequest,
    ToolSelection,
};
use harness_core::tools::CancelToken;

impl Rig {
    fn stream_request(
        &self,
        work: &Path,
        policy: ToolPolicy,
        system: &str,
        user: &str,
    ) -> StreamRequest {
        let mut req = StreamRequest::new(
            "qwen",
            vec![
                json!({"role": "system", "content": system}),
                json!({"role": "user", "content": user}),
            ],
        );
        req.workdir = Some(work.to_path_buf());
        req.settings = Some(ToolSettings {
            policy,
            trusted: vec![],
            workdir: String::new(),
            ..Default::default()
        });
        req.params.insert("temperature".into(), json!(0));
        req.params.insert("max_tokens".into(), json!(400));
        req.origin = ToolOrigin::new("e2e-stream")
            .conversation("e2e-stream-conv")
            .model("qwen");
        req
    }

    fn endpoint(&self) -> HttpEndpoint {
        HttpEndpoint::new(format!("{}/v1", self.server.base), None)
    }
}

/// Counts the hook calls and rewrites the conversation like a compaction would.
struct CountingHook {
    rounds: Vec<u32>,
    sizes: Vec<usize>,
}

impl RoundHook for CountingHook {
    fn before_next_round<'a>(
        &'a mut self,
        round: u32,
        messages: &'a mut Vec<Value>,
    ) -> BoxFuture<'a, ()> {
        self.rounds.push(round);
        self.sizes.push(messages.len());
        // The hook may rewrite the conversation: mark the system prompt as seen.
        if let Some(first) = messages.first_mut() {
            if let Some(text) = first["content"].as_str() {
                if !text.contains("[hooked]") {
                    first["content"] = json!(format!("{text} [hooked]"));
                }
            }
        }
        Box::pin(async {})
    }
}

fn show_stream(name: &str, o: &StreamOutcome, events: &[AgentEvent]) {
    eprintln!(
        "\n=== {name}: stop={:?}, rounds={}, finish={:?}, tokens={:?}/{:?}",
        o.stop, o.rounds, o.finish_reason, o.usage.prompt_tokens, o.usage.completion_tokens
    );
    let texts = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::Text(_)))
        .count();
    eprintln!("  text deltas: {texts}");
    for c in &o.calls {
        let err = c
            .result
            .as_ref()
            .and_then(|r| r.error)
            .map_or("ok", |e| e.as_str());
        let sent: String = c.sent.chars().take(100).collect();
        eprintln!(
            "  r{} {}({}) -> {err}: {sent:?}",
            c.round, c.tool, c.arguments
        );
    }
    for c in &o.client_calls {
        eprintln!("  client call: {}({})", c.name, c.arguments);
    }
    eprintln!("  answer: {:?}", o.text);
}

#[tokio::test]
#[ignore]
async fn a_real_model_drives_the_streaming_loop() {
    let (Some(bin), Some(model)) = (
        std::env::var_os("HS_LLAMA_SERVER").map(PathBuf::from),
        std::env::var_os("HS_TEST_MODEL").map(PathBuf::from),
    ) else {
        eprintln!("HS_LLAMA_SERVER / HS_TEST_MODEL not set, skipping");
        return;
    };
    assert!(bin.is_file() && model.is_file());
    let db_dir = temp_dir();
    let db = Arc::new(AnalyticsDb::open(&db_dir.path().join("a.db")).unwrap());
    let rig = Rig {
        server: start_server(&bin, &model).await,
        service: ToolService::new(ToolRegistry::with_builtin(), LogSink::Db(db.clone())),
        db,
        _db_dir: db_dir,
    };
    let endpoint = rig.endpoint();

    // 1. A tool round streams: the answer's tokens reach the sink one by one, the call runs
    //    between the rounds, and the hook sees every round after the first.
    let work = temp_dir();
    std::fs::write(
        work.path().join("note.txt"),
        "The secret code is PLUM-4417.\n",
    )
    .unwrap();
    let mut events = Vec::new();
    let mut hook = CountingHook {
        rounds: vec![],
        sizes: vec![],
    };
    let mut sink = |e: AgentEvent| events.push(e);
    let o = run_agent_stream(
        &rig.service,
        &endpoint,
        rig.stream_request(
            work.path(),
            ToolPolicy::AllowAll,
            SYSTEM,
            "Read note.txt and tell me the secret code.",
        ),
        &DenyAll,
        &mut sink,
        Some(&mut hook),
    )
    .await
    .unwrap();
    show_stream("streamed tool round", &o, &events);
    assert_eq!(o.stop, StopReason::Answered);
    assert_eq!(o.calls[0].tool, "read_file");
    assert!(o.text.contains("PLUM-4417"), "{}", o.text);
    assert_eq!(hook.rounds, [2], "the hook runs before each next round");
    assert!(o.messages.iter().any(|m| m["content"]
        .as_str()
        .is_some_and(|c| c.contains("[hooked]"))));
    let order: Vec<&str> = events
        .iter()
        .map(|e| match e {
            AgentEvent::Round(_) => "round",
            AgentEvent::Text(_) => "text",
            AgentEvent::Reasoning(_) => "reasoning",
            AgentEvent::ToolStart { .. } => "start",
            AgentEvent::ToolDone(_) => "done",
            AgentEvent::ClientCalls(_) => "client",
        })
        .collect();
    let start = order.iter().position(|e| *e == "start").unwrap();
    let done = order.iter().position(|e| *e == "done").unwrap();
    let first_text = order.iter().position(|e| *e == "text").unwrap();
    assert!(start < done && done < first_text, "{order:?}");
    assert!(
        order.iter().filter(|e| **e == "text").count() >= 3,
        "tokens arrive in pieces: {order:?}"
    );
    assert!(o.usage.completion_tokens.unwrap_or(0) > 0);
    assert!(o.usage.prompt_tokens.unwrap_or(0) > 0);

    // 2. Chained calls over several rounds.
    let work = temp_dir();
    std::fs::create_dir_all(work.path().join("docs")).unwrap();
    std::fs::write(work.path().join("docs/alpha.md"), "token: KIWI-2291\n").unwrap();
    std::fs::write(work.path().join("docs/beta.md"), "token: FIG-0003\n").unwrap();
    let mut events = Vec::new();
    let mut sink = |e: AgentEvent| events.push(e);
    let o = run_agent_stream(
        &rig.service,
        &endpoint,
        rig.stream_request(
            work.path(),
            ToolPolicy::AllowAll,
            SYSTEM,
            "First list the markdown files under the docs folder with the file_glob_search tool, \
             then read alpha.md with read_file and tell me its token.",
        ),
        &DenyAll,
        &mut sink,
        None,
    )
    .await
    .unwrap();
    show_stream("chained calls", &o, &events);
    let tools: Vec<&str> = o.calls.iter().map(|c| c.tool.as_str()).collect();
    assert!(
        tools.iter().position(|t| *t == "file_glob_search")
            < tools.iter().rposition(|t| *t == "read_file"),
        "{tools:?}"
    );
    assert!(o.rounds >= 3);
    assert!(o.text.contains("KIWI-2291"), "{}", o.text);

    // 3. A tool the client owns: the loop does not run it, it stops and hands the call back.
    let mut req = rig.stream_request(
        work.path(),
        ToolPolicy::AllowAll,
        "You are a helpful assistant. Use the provided tools when the user asks for them.",
        "What is the weather in Ankara right now? Use the get_weather tool.",
    );
    req.client_tools = vec![json!({"type": "function", "function": {
        "name": "get_weather", "description": "Current weather of a city",
        "parameters": {"type": "object", "properties": {"city": {"type": "string"}},
                       "required": ["city"]}}})];
    req.tools = ToolSelection::Only(vec!["calculator".into()]);
    let mut events = Vec::new();
    let mut sink = |e: AgentEvent| events.push(e);
    let o = run_agent_stream(&rig.service, &endpoint, req, &DenyAll, &mut sink, None)
        .await
        .unwrap();
    show_stream("client-owned tool", &o, &events);
    assert_eq!(o.stop, StopReason::ClientToolCalls);
    assert_eq!(o.client_calls[0].name, "get_weather");
    assert!(o.client_calls[0].arguments.contains("Ankara"));
    assert!(o.calls.is_empty());
    assert_eq!(
        o.messages.last().unwrap()["tool_calls"][0]["function"]["name"],
        "get_weather"
    );
    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::ClientCalls(c) if c.len() == 1)));

    // 4. Server tool and client tool side by side: the calculator runs here, then the model
    //    asks for the client's tool.
    let mut req = rig.stream_request(
        work.path(),
        ToolPolicy::AllowAll,
        "You are a helpful assistant. Use tools. First compute with the calculator tool, then \
         call get_weather for the city.",
        "Compute 17 * 23 with the calculator tool, then get the weather in Izmir with the \
         get_weather tool.",
    );
    req.client_tools = vec![json!({"type": "function", "function": {
        "name": "get_weather", "description": "Current weather of a city",
        "parameters": {"type": "object", "properties": {"city": {"type": "string"}},
                       "required": ["city"]}}})];
    req.tools = ToolSelection::Only(vec!["calculator".into()]);
    let mut events = Vec::new();
    let mut sink = |e: AgentEvent| events.push(e);
    let o = run_agent_stream(&rig.service, &endpoint, req, &DenyAll, &mut sink, None)
        .await
        .unwrap();
    show_stream("server + client tools", &o, &events);
    assert!(
        o.stop == StopReason::ClientToolCalls || o.stop == StopReason::Answered,
        "{:?}",
        o.stop
    );
    if o.stop == StopReason::ClientToolCalls {
        assert_eq!(o.client_calls[0].name, "get_weather");
    }

    // 5. Cancellation: a running command is stopped, the loop returns at once.
    let work = temp_dir();
    let sleeper = if cfg!(windows) {
        "ping -n 40 127.0.0.1"
    } else {
        "sleep 40"
    };
    let req = rig.stream_request(
        work.path(),
        ToolPolicy::AllowAll,
        SYSTEM,
        &format!("Run exactly this shell command with exec_shell_command: {sleeper}"),
    );
    let cancel: CancelToken = req.cancel.clone();
    let c2 = cancel.clone();
    let mut events = Vec::new();
    let mut sink = |e: AgentEvent| {
        if matches!(e, AgentEvent::ToolStart { .. }) {
            let c = c2.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(800)).await;
                c.cancel();
            });
        }
        events.push(e);
    };
    let started = Instant::now();
    let o = run_agent_stream(&rig.service, &endpoint, req, &DenyAll, &mut sink, None)
        .await
        .unwrap();
    show_stream("cancelled during a tool call", &o, &events);
    assert_eq!(o.stop, StopReason::Cancelled);
    assert!(started.elapsed() < Duration::from_secs(30));
    assert!(o
        .calls
        .iter()
        .any(|c| c.result.as_ref().and_then(|r| r.error) == Some(ToolErrorCode::Cancelled)));

    // Cancelled while the model streams.
    let work = temp_dir();
    let mut req = rig.stream_request(
        work.path(),
        ToolPolicy::AllowAll,
        "You are a storyteller.",
        "Write a very long story about a lighthouse keeper, at least 600 words.",
    );
    req.params.insert("max_tokens".into(), json!(1500));
    let cancel = req.cancel.clone();
    let mut tokens = 0usize;
    let mut sink = |e: AgentEvent| {
        if matches!(e, AgentEvent::Text(_)) {
            tokens += 1;
            if tokens == 5 {
                cancel.cancel();
            }
        }
    };
    let started = Instant::now();
    let o = run_agent_stream(&rig.service, &endpoint, req, &DenyAll, &mut sink, None)
        .await
        .unwrap();
    assert_eq!(o.stop, StopReason::Cancelled);
    assert!(started.elapsed() < Duration::from_secs(20));
    eprintln!("\n=== cancelled while streaming: stopped after {tokens} tokens");

    // 6. Timeout and approval refusal go back to the model as results.
    let work = temp_dir();
    let mut req = rig.stream_request(
        work.path(),
        ToolPolicy::AllowAll,
        SYSTEM,
        &format!("Run exactly this shell command with exec_shell_command and report what happened: {sleeper}"),
    );
    req.call_timeout = Some(Duration::from_secs(2));
    let mut events = Vec::new();
    let mut sink = |e: AgentEvent| events.push(e);
    let o = run_agent_stream(&rig.service, &endpoint, req, &DenyAll, &mut sink, None)
        .await
        .unwrap();
    show_stream("call timeout", &o, &events);
    assert!(o
        .calls
        .iter()
        .any(|c| c.result.as_ref().and_then(|r| r.error) == Some(ToolErrorCode::Timeout)));
    assert!(!o.text.is_empty());

    let work = temp_dir();
    let req = rig.stream_request(
        work.path(),
        ToolPolicy::AskEveryTime,
        SYSTEM,
        "Create a file named hello.txt containing the text hi, using the write_file tool.",
    );
    let mut events = Vec::new();
    let mut sink = |e: AgentEvent| events.push(e);
    let o = run_agent_stream(&rig.service, &endpoint, req, &DenyAll, &mut sink, None)
        .await
        .unwrap();
    show_stream("approval refused", &o, &events);
    assert!(o
        .calls
        .iter()
        .any(|c| c.result.as_ref().and_then(|r| r.error) == Some(ToolErrorCode::ApprovalRequired)));
    assert!(!work.path().join("hello.txt").exists());

    // 7. Round limit.
    let work = temp_dir();
    for i in 1..=6 {
        let body = if i < 6 {
            format!("next: n{}.txt\n", i + 1)
        } else {
            "the end\n".to_string()
        };
        std::fs::write(work.path().join(format!("n{i}.txt")), body).unwrap();
    }
    let mut req = rig.stream_request(
        work.path(),
        ToolPolicy::AllowAll,
        SYSTEM,
        "Start at n1.txt. Each file names the next file to read; read them one at a time with \
         read_file until a file says the end, then tell me how many files you read.",
    );
    req.limits = ToolLimits {
        max_rounds: 3,
        ..ToolLimits::default()
    };
    let mut events = Vec::new();
    let mut sink = |e: AgentEvent| events.push(e);
    let o = run_agent_stream(&rig.service, &endpoint, req, &DenyAll, &mut sink, None)
        .await
        .unwrap();
    show_stream("round limit", &o, &events);
    assert_eq!(o.stop, StopReason::RoundLimit);
    assert_eq!(o.rounds, 3);

    // The tool log has every call of the runs.
    let log = rig.log();
    eprintln!("\n=== tool log ({} rows)", log.len());
    for l in &log {
        eprintln!("  {l}");
    }
    assert!(log.iter().any(|l| l.contains("approval_required")));
    assert!(log.iter().any(|l| l.contains("timeout")));
    assert!(log.iter().any(|l| l.contains("cancelled")));
}
