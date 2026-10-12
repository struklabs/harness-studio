//! End-to-end runs against a real llama-server and model (`#[ignore]`; nothing is mocked).
//!
//! ```text
//! cargo test -p harness-core compaction::e2e -- --ignored --nocapture --test-threads=1
//! ```
//!
//! The llama.cpp build is copied from `HARNESS_E2E_LLAMA_DIR` (default: the managed runtime,
//! `%APPDATA%\com.harnessstudio.app\bin\managed`) into a temporary directory and started on
//! a free port with `HARNESS_E2E_MODEL` (default: Qwen3-4B-Instruct-2507 Q4_K_M) and a small
//! context (4096), so a short conversation outgrows it. The conversation itself is produced
//! by the model.

use super::*;
use crate::state::{AppendMessage, NewConversation, StateDb, VariantInput};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

const N_CTX: u64 = 4096;
const SYSTEM: &str = "You are a helpful assistant.";

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

/// A llama-server copied to a temp dir; killed on drop.
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

async fn start_server() -> Server {
    start_server_with(N_CTX).await
}

async fn start_server_with(n_ctx: u64) -> Server {
    let src = llama_dir();
    assert!(
        src.join("llama-server.exe").exists() || src.join("llama-server").exists(),
        "no llama-server in {src:?}"
    );
    assert!(model_path().exists(), "model not found: {:?}", model_path());
    let dir = tempfile::tempdir().unwrap();
    copy_dir(&src, dir.path());
    let exe = if cfg!(windows) {
        "llama-server.exe"
    } else {
        "llama-server"
    };
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let child = Command::new(dir.path().join(exe))
        .args(["-m"])
        .arg(model_path())
        .args([
            "-c",
            &n_ctx.to_string(),
            "--jinja",
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
        ])
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

fn endpoint(s: &Server) -> Endpoint {
    Endpoint {
        base_url: format!("{}/v1", s.host),
        headers: Default::default(),
        model: None,
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Usage {
    prompt: u64,
    completion: u64,
}

async fn complete(s: &Server, messages: &[Value], max_tokens: u64) -> (String, Usage) {
    let body = json!({
        "messages": messages,
        "max_tokens": max_tokens,
        "temperature": 0.0,
        "seed": 7,
        "stream": false,
    });
    let v: Value = reqwest::Client::new()
        .post(format!("{}/v1/chat/completions", s.host))
        .json(&body)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let content = v["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or_else(|| panic!("no answer: {v}"));
    (
        content.to_string(),
        Usage {
            prompt: v["usage"]["prompt_tokens"].as_u64().unwrap(),
            completion: v["usage"]["completion_tokens"].as_u64().unwrap(),
        },
    )
}

/// Appends a user message, sends the conversation as the Playground would (honouring the
/// active compaction) and stores the answer with its token stats.
async fn turn(db: &StateDb, conv: &str, s: &Server, text: &str, max_tokens: u64) -> Usage {
    let leaf = db
        .get_conversation(conv)
        .unwrap()
        .messages
        .last()
        .map(|m| m.id.clone());
    db.append_message(AppendMessage {
        conversation_id: conv.into(),
        parent_id: leaf,
        role: "user".into(),
        variant: VariantInput {
            content: text.into(),
            ..Default::default()
        },
        id: None,
        variant_id: None,
        any_parent_variant: false,
    })
    .unwrap();
    let path = db.get_conversation(conv).unwrap().messages;
    let (answer, u) = complete(s, &request_messages(&path, SYSTEM), max_tokens).await;
    db.append_message(AppendMessage {
        conversation_id: conv.into(),
        parent_id: Some(path.last().unwrap().id.clone()),
        role: "assistant".into(),
        variant: VariantInput {
            content: answer,
            stats: Some(
                json!({ "source": "usage", "promptN": u.prompt, "predictedN": u.completion }),
            ),
            ..Default::default()
        },
        id: None,
        variant_id: None,
        any_parent_variant: false,
    })
    .unwrap();
    u
}

/// Facts planted early in the conversation and the question that checks each one.
const FACTS: &[(&str, &str, &[&str])] = &[
    (
        "My project's code name is KESTREL-47. Please remember it.",
        "What is my project's code name?",
        &["kestrel-47"],
    ),
    (
        "The project budget is 18,750 euros.",
        "What is the project budget?",
        &["18,750", "18750", "18.750"],
    ),
    (
        "Our kickoff meeting is on November 14 at 09:30.",
        "When is the kickoff meeting (date and time)?",
        &["09:30", "9:30"],
    ),
    (
        "The team lead is Zeynep Arıkan.",
        "Who is the team lead?",
        &["arıkan", "arikan"],
    ),
    (
        "The staging server's IP address is 10.24.7.199.",
        "What is the staging server's IP address?",
        &["10.24.7.199"],
    ),
    (
        "My cat is called Pırasa.",
        "What is my cat's name?",
        &["pırasa", "pirasa"],
    ),
    (
        "I want all reports delivered as PDF, never as Word files.",
        "In which file format do I want reports?",
        &["pdf"],
    ),
    (
        "The deadline for the beta release is March 3.",
        "What is the beta release deadline?",
        &["march 3", "3 march", "march 3rd"],
    ),
];

/// Questions that make the model write long answers (they fill the context).
const FILLERS: &[&str] = &[
    "Explain in detail how the TCP three-way handshake works and why it is needed.",
    "Describe the main differences between processes and threads, with examples.",
    "How does a hash map handle collisions? Explain chaining and open addressing.",
    "Explain how HTTPS protects data in transit, step by step.",
    "What are the trade-offs between SQL and NoSQL databases? Give concrete examples.",
    "Explain how garbage collection works in modern runtimes.",
];

/// The facts, then long filler turns.
async fn build_conversation(db: &StateDb, s: &Server, title: &str) -> String {
    let conv = db
        .create_conversation(&NewConversation {
            title: Some(title.into()),
            system_prompt: Some(SYSTEM.into()),
            ..Default::default()
        })
        .unwrap()
        .id;
    for (fact, _, _) in FACTS {
        turn(
            db,
            &conv,
            s,
            &format!("{fact} Just acknowledge briefly."),
            60,
        )
        .await;
    }
    conv
}

fn retained(answer: &str, expected: &[&str]) -> bool {
    let a = answer.to_lowercase();
    expected.iter().any(|e| a.contains(e))
}

/// Asks every fact question against the conversation as it would be sent now (one request
/// per question, not stored). Returns (facts recalled, prompt tokens of the first question).
async fn quiz(db: &StateDb, conv: &str, s: &Server) -> (usize, u64, Vec<String>) {
    let path = db.get_conversation(conv).unwrap().messages;
    let base = request_messages(&path, SYSTEM);
    let mut ok = 0;
    let mut prompt = 0;
    let mut missed = Vec::new();
    for (i, (_, q, expected)) in FACTS.iter().enumerate() {
        let mut msgs = base.clone();
        msgs.push(
            json!({ "role": "user", "content": format!("{q} Answer in one short sentence.") }),
        );
        let (a, u) = complete(s, &msgs, 60).await;
        if i == 0 {
            prompt = u.prompt;
        }
        if retained(&a, expected) {
            ok += 1;
        } else {
            missed.push(format!("{q} -> {}", a.trim()));
        }
    }
    (ok, prompt, missed)
}

fn request(conv: &str, s: &Server, trigger: TriggerKind, strategy: Strategy) -> CompactRequest {
    CompactRequest {
        conversation_id: conv.into(),
        leaf_id: None,
        endpoint: endpoint(s),
        tokenize: Some(TokenizeEndpoint {
            host_url: s.host.clone(),
            headers: Default::default(),
        }),
        n_ctx: N_CTX,
        summary_ctx: None,
        keep_turns: 2,
        clear_tool_results: true,
        summary_tokens: None,
        instruction: None,
        strategy,
        trigger,
        mid_turn: false,
        documents: Vec::new(),
        system_prompt: SYSTEM.into(),
    }
}

/// Auto-compaction fires at the threshold, the model still knows a summarized fact, the
/// prompt shrinks, and undo restores the full history.
#[tokio::test]
#[ignore = "needs a llama-server build and a GGUF model (see module docs)"]
async fn auto_compaction_with_a_real_model() {
    let server = start_server().await;
    let dir = tempfile::tempdir().unwrap();
    let db = StateDb::open(&dir.path().join("state.db")).unwrap();
    let conv = build_conversation(&db, &server, "e2e").await;
    let threshold = Threshold::Percent { percent: 80.0 };
    let limit = threshold.limit(N_CTX);

    let mut last = Usage::default();
    let mut outcome = None;
    for (i, filler) in FILLERS.iter().cycle().take(20).enumerate() {
        // Before sending: the conversation's size so far (last request + answer) and the
        // new message, as the Playground computes it.
        let used = last.prompt + last.completion;
        let incoming = estimate_tokens(filler) + MESSAGE_OVERHEAD;
        if should_compact(used, incoming, N_CTX, threshold) {
            let cancel = AtomicBool::new(false);
            let started = Instant::now();
            let out = compact(
                &db,
                &request(&conv, &server, TriggerKind::Auto, Strategy::Structured),
                &cancel,
            )
            .await
            .unwrap();
            println!(
                "turn {i}: used {used} + {incoming} >= {limit} -> compacted {} messages ({} turns, {} kept) \
                 in {:.1}s: {} -> {} tokens, summary {} tokens, {} request(s), exact counts: {}",
                out.marker.covered,
                out.marker.turns,
                out.marker.kept_turns,
                started.elapsed().as_secs_f64(),
                out.marker.tokens_before,
                out.marker.tokens_after,
                out.marker.summary_tokens,
                out.requests,
                out.marker.exact_tokens,
            );
            println!(
                "--- summary ---\n{}\n---------------",
                out.view.messages.last().unwrap().variant.content
            );
            assert!(
                out.marker.tokens_before + incoming >= limit * 9 / 10,
                "{:?}",
                out.marker
            );
            assert!(
                out.marker.tokens_after < out.marker.tokens_before / 2,
                "{:?}",
                out.marker
            );
            outcome = Some((used, out));
            break;
        }
        last = turn(&db, &conv, &server, filler, 400).await;
        println!(
            "turn {i}: prompt {} + answer {} tokens",
            last.prompt, last.completion
        );
        assert!(
            last.prompt + last.completion < N_CTX,
            "context overflowed before compaction"
        );
    }
    let (used_before, out) = outcome.expect("auto-compaction never triggered");

    // The next turn runs on summary + kept turns; a summarized fact is still known.
    let path = db.get_conversation(&conv).unwrap().messages;
    let mut msgs = request_messages(&path, SYSTEM);
    msgs.push(json!({ "role": "user", "content": "What is my project's code name and who is the team lead? Answer briefly." }));
    let (answer, u) = complete(&server, &msgs, 80).await;
    println!(
        "after compaction: prompt {} tokens (was {used_before}); answer: {}",
        u.prompt,
        answer.trim()
    );
    assert!(
        retained(&answer, &["kestrel-47"]) && retained(&answer, &["arıkan", "arikan"]),
        "{answer}"
    );
    assert!(
        u.prompt < used_before / 2,
        "prompt did not shrink: {} vs {used_before}",
        u.prompt
    );

    // Undo: the request is the whole history again (nothing was deleted).
    let view = set_undone(&db, &out.marker_id, true).unwrap();
    let full = request_messages(&view.messages, SYSTEM);
    let chat_messages = view.messages.iter().filter(|m| is_chat(m)).count();
    assert_eq!(full.len(), 1 + chat_messages);
    assert!(auto_paused(&view.messages));
    let (_, u_full) = complete(&server, &full, 1).await;
    println!(
        "after undo: {} messages, prompt {} tokens",
        full.len(),
        u_full.prompt
    );
    assert!(u_full.prompt > u.prompt * 2);
    // Re-apply.
    let view = set_undone(&db, &out.marker_id, false).unwrap();
    assert!(active_compaction(&view.messages).is_some());
}

/// The same real conversation compacted with each strategy: facts recalled vs. tokens.
#[tokio::test]
#[ignore = "needs a llama-server build and a GGUF model (see module docs)"]
async fn compare_summary_strategies() {
    let server = start_server().await;
    let dir = tempfile::tempdir().unwrap();
    let db = StateDb::open(&dir.path().join("state.db")).unwrap();
    let conv = build_conversation(&db, &server, "strategies").await;
    // Fill to ~75 % of the context with long answers.
    loop {
        let path = db.get_conversation(&conv).unwrap().messages;
        let last = path
            .iter()
            .rev()
            .find_map(|m| m.variant.stats.clone())
            .unwrap();
        let used = last["promptN"].as_u64().unwrap() + last["predictedN"].as_u64().unwrap();
        if used >= N_CTX * 3 / 4 {
            println!("conversation: {} messages, {used} tokens", path.len());
            break;
        }
        let i = path.len() / 2 % FILLERS.len();
        turn(&db, &conv, &server, FILLERS[i], 400).await;
    }

    let (full_ok, full_prompt, _) = quiz(&db, &conv, &server).await;
    println!(
        "{:<12} {:>9} {:>14} {:>14} {:>8}",
        "strategy", "recalled", "prompt tokens", "summary tok.", "seconds"
    );
    println!(
        "{:<12} {:>6}/{} {:>14} {:>14} {:>8}",
        "none (full)",
        full_ok,
        FACTS.len(),
        full_prompt,
        "-",
        "-"
    );

    // Baseline without a summary: the summarized turns are simply dropped.
    {
        let copy = db.duplicate_conversation(&conv, None).unwrap().id;
        let view = db.get_conversation(&copy).unwrap();
        let path = &view.messages;
        let tokens = vec![100; path.len()];
        let p = plan(
            path,
            &tokens,
            &PlanOptions {
                n_ctx: N_CTX,
                keep_turns: 2,
                min_tokens: 0,
            },
        )
        .unwrap();
        let kept: Vec<_> = p.kept.iter().map(|&i| path[i].clone()).collect();
        let mut base = request_messages(&kept, SYSTEM);
        let mut ok = 0;
        let mut prompt = 0;
        for (i, (_, q, expected)) in FACTS.iter().enumerate() {
            base.push(
                json!({ "role": "user", "content": format!("{q} Answer in one short sentence.") }),
            );
            let (a, u) = complete(&server, &base, 60).await;
            base.pop();
            if i == 0 {
                prompt = u.prompt;
            }
            ok += retained(&a, expected) as usize;
        }
        println!(
            "{:<12} {:>6}/{} {:>14} {:>14} {:>8}",
            "truncate",
            ok,
            FACTS.len(),
            prompt,
            0,
            "-"
        );
    }

    for strategy in [Strategy::Brief, Strategy::Structured, Strategy::Incremental] {
        let runs = std::env::var("HARNESS_E2E_RUNS")
            .ok()
            .and_then(|r| r.parse().ok())
            .unwrap_or(1);
        for run in 0..runs {
            let copy = db.duplicate_conversation(&conv, None).unwrap().id;
            let cancel = AtomicBool::new(false);
            let started = Instant::now();
            let out = compact(
                &db,
                &request(&copy, &server, TriggerKind::Manual, strategy),
                &cancel,
            )
            .await
            .unwrap();
            let secs = started.elapsed().as_secs_f64();
            let (ok, prompt, missed) = quiz(&db, &copy, &server).await;
            println!(
                "{:<12} {:>6}/{} {:>14} {:>14} {:>8.1}   (run {run}, {} request(s), {} -> {} tokens)",
                strategy.as_str(),
                ok,
                FACTS.len(),
                prompt,
                out.marker.summary_tokens,
                secs,
                out.requests,
                out.marker.tokens_before,
                out.marker.tokens_after,
            );
            for m in missed {
                println!("    missed: {m}");
            }
        }
    }
}

// ---- compaction in the middle of a long tool loop ----

/// Reads a real file the way the Playground's `read_file` tool would (first characters only).
fn read_file_tool(path: &str) -> String {
    let full = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path);
    match std::fs::read_to_string(&full) {
        Ok(text) => text.chars().take(7000).collect(),
        Err(e) => format!("Error: {e}"),
    }
}

/// One chat request with the tool offered; returns the answer, its tool calls and the usage.
async fn complete_with_tools(
    s: &Server,
    messages: &[Value],
) -> (String, Vec<(String, String, String)>, Usage) {
    let body = json!({
        "messages": messages,
        "max_tokens": 400,
        "temperature": 0.0,
        "seed": 7,
        "stream": false,
        "tools": [{
            "type": "function",
            "function": {
                "name": "read_file",
                "description": "Reads a text file of the project and returns its first characters.",
                "parameters": {
                    "type": "object",
                    "properties": { "path": { "type": "string", "description": "Path relative to the project root" } },
                    "required": ["path"]
                }
            }
        }],
    });
    let v: Value = reqwest::Client::new()
        .post(format!("{}/v1/chat/completions", s.host))
        .json(&body)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let msg = &v["choices"][0]["message"];
    let calls = msg["tool_calls"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|c| {
                    (
                        c["id"].as_str().unwrap_or("call").to_string(),
                        c["function"]["name"]
                            .as_str()
                            .unwrap_or_default()
                            .to_string(),
                        c["function"]["arguments"]
                            .as_str()
                            .unwrap_or("{}")
                            .to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    (
        msg["content"].as_str().unwrap_or_default().to_string(),
        calls,
        Usage {
            prompt: v["usage"]["prompt_tokens"].as_u64().unwrap_or(0),
            completion: v["usage"]["completion_tokens"].as_u64().unwrap_or(0),
        },
    )
}

/// A real tool loop (the model decides to read three real files one after the other) crosses
/// the threshold in the middle of one answer: the loop compacts between two rounds (as the
/// Playground does), the question stays verbatim, the request shrinks, a fact from a summarized
/// round is still known at the end, and undo restores the full history.
#[tokio::test]
#[ignore = "needs a llama-server build and a GGUF model (see module docs)"]
async fn compaction_in_the_middle_of_a_tool_loop() {
    const CTX: u64 = 12288;
    let server = start_server_with(CTX).await;
    let dir = tempfile::tempdir().unwrap();
    let db = StateDb::open(&dir.path().join("state.db")).unwrap();
    let conv = db
        .create_conversation(&NewConversation {
            title: Some("mid-turn".into()),
            system_prompt: Some(SYSTEM.into()),
            ..Default::default()
        })
        .unwrap()
        .id;
    let question = "Use the read_file tool to read these three files, one call at a time and each in its own step: \
        docs/en/ROADMAP.md, docs/en/ARCHITECTURE.md and docs/en/FEATURES.md. When you have read all three, \
        reply with the title (the first line) of each file.";
    let user = db
        .append_message(AppendMessage {
            conversation_id: conv.clone(),
            parent_id: None,
            role: "user".into(),
            variant: VariantInput {
                content: question.into(),
                ..Default::default()
            },
            id: None,
            variant_id: None,
            any_parent_variant: false,
        })
        .unwrap();
    let threshold = Threshold::Percent { percent: 40.0 };
    let limit = threshold.limit(CTX);
    let mut compactions: Vec<CompactOutcome> = Vec::new();
    let mut final_answer = None;
    let mut sizes = Vec::new();
    for round in 0..9 {
        let path = db.get_conversation(&conv).unwrap().messages;
        let msgs = request_messages(&path, SYSTEM);
        let (content, calls, usage) = complete_with_tools(&server, &msgs).await;
        sizes.push(usage.prompt);
        println!(
            "round {round}: request {} tokens, {} message(s), answer {} tokens, {} tool call(s)",
            usage.prompt,
            msgs.len(),
            usage.completion,
            calls.len()
        );
        if calls.is_empty() {
            final_answer = Some(content);
            break;
        }
        let mut stored = Vec::new();
        let mut result_tokens = 0;
        for (id, name, arguments) in calls {
            let path = serde_json::from_str::<Value>(&arguments)
                .ok()
                .and_then(|v| v["path"].as_str().map(str::to_string))
                .unwrap_or_default();
            let result = read_file_tool(&path);
            println!("  tool call {name}({path}) -> {} chars", result.len());
            result_tokens += estimate_tokens(&result);
            stored.push(json!({ "id": id, "name": name, "arguments": arguments, "status": "done", "result": result }));
        }
        db.append_message(AppendMessage {
            conversation_id: conv.clone(),
            parent_id: Some(path.last().unwrap().id.clone()),
            role: "assistant".into(),
            variant: VariantInput {
                content,
                tool_calls: Some(Value::Array(stored)),
                stats: Some(json!({ "source": "usage", "promptN": usage.prompt, "predictedN": usage.completion })),
                ..Default::default()
            },
            id: None,
            variant_id: None,
            any_parent_variant: false,
        })
        .unwrap();
        // The Playground's check between two tool rounds: the request just sent, its answer and
        // the tool results that join the next request.
        if usage.prompt + usage.completion + result_tokens >= limit {
            let cancel = AtomicBool::new(false);
            let mut req = request(&conv, &server, TriggerKind::Auto, Strategy::Structured);
            req.n_ctx = CTX;
            req.mid_turn = true;
            match compact(&db, &req, &cancel).await {
                Ok(out) => {
                    println!(
                        "  mid-turn compaction: {} messages summarized, {} -> {} tokens, summary {} tokens",
                        out.marker.covered,
                        out.marker.tokens_before,
                        out.marker.tokens_after,
                        out.marker.summary_tokens
                    );
                    println!(
                        "--- summary ---\n{}\n---------------",
                        out.view.messages.last().unwrap().variant.content
                    );
                    compactions.push(out);
                }
                Err(e) => println!("  mid-turn compaction skipped: {e}"),
            }
        }
    }
    let answer = final_answer.expect("the model never finished its answer");
    println!("final answer: {}", answer.trim());
    assert!(
        !compactions.is_empty(),
        "no mid-turn compaction happened (request sizes {sizes:?})"
    );
    for out in &compactions {
        assert!(out.marker.mid_turn);
        assert_eq!(out.marker.pinned_id.as_deref(), Some(user.id.as_str()));
        assert!(
            out.marker.tokens_after < out.marker.tokens_before,
            "{:?}",
            out.marker
        );
    }

    // The request after a compaction starts with the summary and the question, verbatim.
    let view = db.get_conversation(&conv).unwrap();
    let msgs = request_messages(&view.messages, SYSTEM);
    assert_eq!(msgs[0]["role"], "system");
    assert!(msgs[0]["content"]
        .as_str()
        .unwrap()
        .contains("conversation_summary"));
    assert_eq!(msgs[1]["role"], "user");
    assert_eq!(msgs[1]["content"], question);
    assert!(
        view.messages.iter().any(|m| m.id == user.id)
            && active_compaction(&view.messages).unwrap().pinned_index == Some(0)
    );

    // What was read in a summarized round is still known: the titles of the files.
    let a = answer.to_lowercase();
    let known = ["yol haritası", "sistem mimarisi", "özellikler"]
        .iter()
        .filter(|t| a.contains(**t))
        .count();
    assert!(
        known >= 2,
        "the answer lost the file titles ({known}/3): {answer}"
    );

    // Undo: the full history (every round) is sent again, nothing was deleted.
    let marker = compactions.last().unwrap().marker_id.clone();
    let mut v = view;
    for out in compactions.iter().rev() {
        v = set_undone(&db, &out.marker_id, true).unwrap();
        let _ = &marker;
    }
    let full = request_messages(&v.messages, SYSTEM);
    assert!(full.len() > msgs.len(), "{} vs {}", full.len(), msgs.len());
    assert!(auto_paused(&v.messages));
}
