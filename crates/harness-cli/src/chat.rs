//! `run` and `chat`: load a model if needed, then stream an OpenAI-compatible chat
//! completion from its llama-server (the model's endpoint, not the Control API).

use std::io::{BufRead, IsTerminal, Write};
use std::time::Duration;

use futures_util::StreamExt;
use serde_json::{json, Value};

use crate::cli::{ChatArgs, GenerationArgs, RunArgs};
use crate::client::seg;
use crate::commands::resolve::{definition_for, idle_server_of, servers_of, wait_running};
use crate::error::{CliError, CliResult, Exit};
use crate::output::{str_of, Out};
use crate::session::Session;

/// A server serving the model.
pub struct Target {
    pub server_id: String,
    pub alias: String,
    /// `http://host:port/v1`
    pub endpoint: String,
    pub api_key: Option<String>,
    /// This command started it.
    pub loaded: bool,
    /// Requests go through the instance's gateway (managed endpoint).
    pub managed: bool,
}

pub async fn ensure_loaded(
    session: &Session,
    model: &str,
    gen: &GenerationArgs,
    out: &Out,
) -> CliResult<Target> {
    let client = &session.client;
    let def = definition_for(client, model, out).await?;
    let def_id = str_of(&def["id"]);
    let timeout = Duration::from_secs(gen.timeout);
    let running = servers_of(client, &def_id).await?;
    let (view, loaded) = match running
        .iter()
        .find(|s| s["status"] == "Running")
        .or(running.first())
    {
        Some(s) if s["status"] == "Running" => (s.clone(), false),
        Some(s) => (
            wait_running(client, &str_of(&s["id"]), timeout, out).await?,
            false,
        ),
        None => {
            out.info(format!("Loading {}...", str_of(&def["alias"])));
            let reuse = idle_server_of(client, &def_id).await?;
            let loaded = client
                .post(
                    "/load",
                    &json!({"definition_id": def_id, "server_id": reuse}),
                )
                .await?;
            let id = str_of(&loaded["server_id"]);
            (wait_running(client, &id, timeout, out).await?, true)
        }
    };
    let endpoint = view["endpoint"]
        .as_str()
        .ok_or_else(|| CliError::general("cli.server_failed", "The server reports no endpoint"))?;
    let key_needed = view["active"]["api_key_set"] == true;
    if key_needed && gen.api_key.is_none() {
        return Err(CliError::usage(
            "cli.invalid_argument",
            "This server requires an API key: pass --api-key or set HARNESS_SERVER_API_KEY",
        ));
    }
    let mut target = Target {
        server_id: str_of(&view["id"]),
        alias: view["served_alias"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| str_of(&def["alias"])),
        endpoint: session.reachable_endpoint(endpoint),
        api_key: gen.api_key.clone().filter(|_| key_needed),
        loaded,
        managed: false,
    };
    use_managed_endpoint(session, &mut target).await;
    Ok(target)
}

/// While the instance's gateway runs, `run`/`chat` use the managed endpoint (the alias routes
/// to the same server; the request gets client attribution and queueing).
async fn use_managed_endpoint(session: &Session, target: &mut Target) {
    if let Some(gw) = managed_endpoint(session).await {
        target.endpoint = gw.base_url;
        target.api_key = gw.token;
        target.managed = true;
    }
}

/// The instance's gateway as this command can reach it.
pub struct Managed {
    /// `http://host:port/v1`
    pub base_url: String,
    /// The token to send (`None`: the gateway needs none).
    pub token: Option<String>,
}

/// The managed endpoint while the gateway runs. A gateway that needs a token is used with the
/// local gateway client token (owner on this computer) or a named token (`--host` +
/// `HARNESS_TOKEN`); without a usable token (or a loopback gateway of a remote instance)
/// there is none and callers stay on the direct endpoint.
pub async fn managed_endpoint(session: &Session) -> Option<Managed> {
    let status = session.client.get("/status").await.ok()?;
    let gw = &status["gateway"];
    let base = gw["endpoint"]["base_url"]
        .as_str()
        .filter(|_| gw["running"] == true)?;
    let remote = matches!(session.via, crate::session::Via::Remote);
    if remote && harness_core::network::is_loopback_host(gw["host"].as_str().unwrap_or("127.0.0.1"))
    {
        return None; // a loopback gateway on another computer is not reachable from here
    }
    let token = if gw["require_token"] == true {
        let t = match session.via {
            crate::session::Via::Remote => {
                Some(session.client.token().to_string()).filter(|t| t.starts_with("hs_"))
            }
            _ => std::fs::read_to_string(harness_core::gateway::client_token_path(
                &harness_core::paths::data_root(),
            ))
            .ok()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty()),
        };
        Some(t?)
    } else {
        None
    };
    Some(Managed {
        base_url: session.reachable_endpoint(base),
        token,
    })
}

#[derive(Debug, Default)]
pub struct Completion {
    pub content: String,
    pub reasoning: String,
    pub finish_reason: Option<String>,
    pub usage: Value,
    pub timings: Value,
    pub interrupted: bool,
}

fn server_error(status: u16, body: &str) -> CliError {
    let parsed: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let msg = parsed["error"]["message"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| body.chars().take(300).collect());
    let exit = if crate::messages::mentions_out_of_memory(&msg) {
        Exit::OutOfMemory
    } else {
        match crate::error::exit_for_status(status) {
            Exit::Ok | Exit::Usage => Exit::General,
            other => other,
        }
    };
    CliError::new(
        exit,
        "cli.chat_failed",
        format!("llama-server answered HTTP {}: {}", status, msg),
    )
    .with("http_status", status)
}

/// Streams one completion. Tokens go to `sink` as they arrive (the reasoning text to stderr
/// unless quiet); Ctrl+C stops generation and keeps what arrived.
pub async fn complete(
    target: &Target,
    messages: &[Value],
    gen: &GenerationArgs,
    mut sink: Option<&mut dyn Write>,
    out: &Out,
) -> CliResult<Completion> {
    let http = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .no_proxy()
        .build()
        .unwrap_or_default();
    let mut body = json!({
        "model": target.alias,
        "messages": messages,
        "stream": true,
        "stream_options": {"include_usage": true},
    });
    if let Some(n) = gen.max_tokens {
        body["max_tokens"] = json!(n);
    }
    if let Some(t) = gen.temperature {
        body["temperature"] = json!(t);
    }
    let url = format!("{}/chat/completions", target.endpoint);
    let mut req = http.post(&url).json(&body);
    if let Some(key) = &target.api_key {
        req = req.bearer_auth(key);
    }
    if target.managed {
        req = req.header("x-harness-client", "harness-cli");
    }
    let unreachable = |e: reqwest::Error| CliError::transport(&e, &url);
    let resp = req.send().await.map_err(unreachable)?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        let text = resp.text().await.unwrap_or_default();
        return Err(server_error(status, &text));
    }
    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    let mut done = Completion::default();
    let show_reasoning = !out.quiet && sink.is_some();
    let mut in_reasoning = false;
    'outer: loop {
        let chunk = tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                done.interrupted = true;
                break 'outer;
            }
            c = stream.next() => c,
        };
        let Some(chunk) = chunk else { break };
        buf.extend_from_slice(&chunk.map_err(unreachable)?);
        while let Some(pos) = buf.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = buf.drain(..=pos).collect();
            let line = String::from_utf8_lossy(&line);
            let Some(data) = line.trim().strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data == "[DONE]" {
                break 'outer;
            }
            let Ok(event) = serde_json::from_str::<Value>(data) else {
                continue;
            };
            if event.get("error").is_some() {
                return Err(server_error(500, data));
            }
            let choice = &event["choices"][0];
            if let Some(r) = choice["delta"]["reasoning_content"].as_str() {
                done.reasoning.push_str(r);
                if show_reasoning {
                    if !in_reasoning {
                        eprint!("\x1b[2m");
                        in_reasoning = true;
                    }
                    eprint!("{}", r);
                }
            }
            if let Some(c) = choice["delta"]["content"].as_str() {
                if in_reasoning {
                    eprintln!("\x1b[0m");
                    in_reasoning = false;
                }
                done.content.push_str(c);
                if let Some(w) = sink.as_deref_mut() {
                    let _ = w.write_all(c.as_bytes());
                    let _ = w.flush();
                }
            }
            if let Some(f) = choice["finish_reason"].as_str() {
                done.finish_reason = Some(f.to_string());
            }
            if event["usage"].is_object() {
                done.usage = event["usage"].clone();
            }
            if event["timings"].is_object() {
                done.timings = event["timings"].clone();
            }
        }
    }
    if in_reasoning {
        eprint!("\x1b[0m");
    }
    Ok(done)
}

fn base_messages(gen: &GenerationArgs) -> Vec<Value> {
    gen.system
        .iter()
        .map(|s| json!({"role": "system", "content": s}))
        .collect()
}

fn stats_line(c: &Completion) -> Option<String> {
    let tps = c.timings["predicted_per_second"].as_f64()?;
    let n = c.timings["predicted_n"].as_u64().unwrap_or(0);
    Some(format!("{} tokens, {:.1} tokens/s", n, tps))
}

async fn finish(session: &Session, target: &Target, unload: bool, out: &Out) {
    if unload && target.loaded {
        if let Err(e) = session
            .client
            .post(
                &format!("/servers/{}/stop", seg(&target.server_id)),
                &json!({}),
            )
            .await
        {
            out.warn(format!(
                "could not stop {}: {}",
                target.server_id, e.message
            ));
        }
    }
}

pub async fn run(session: &Session, args: &RunArgs, out: &Out) -> CliResult<()> {
    let mut prompt = args.prompt.join(" ");
    if prompt.trim().is_empty() {
        if std::io::stdin().is_terminal() {
            let chat_args = ChatArgs {
                model: args.model.clone(),
                unload: args.unload,
                gen: args.gen.clone(),
            };
            return chat(session, &chat_args, out).await;
        }
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut prompt)
            .map_err(|e| CliError::general("cli.io", e.to_string()))?;
        if prompt.trim().is_empty() {
            return Err(CliError::usage("cli.invalid_argument", "Empty prompt"));
        }
    }
    let target = ensure_loaded(session, &args.model, &args.gen, out).await?;
    let mut messages = base_messages(&args.gen);
    messages.push(json!({"role": "user", "content": prompt}));
    let mut stdout = std::io::stdout();
    let sink: Option<&mut dyn Write> = if out.json { None } else { Some(&mut stdout) };
    let result = complete(&target, &messages, &args.gen, sink, out).await;
    let result = match result {
        Ok(c) => c,
        Err(e) => {
            finish(session, &target, args.unload, out).await;
            return Err(e);
        }
    };
    if out.json {
        let value = json!({
            "model": target.alias,
            "server_id": target.server_id,
            "endpoint": target.endpoint,
            "loaded": target.loaded,
            "content": result.content,
            "reasoning_content": if result.reasoning.is_empty() { Value::Null } else { json!(result.reasoning) },
            "finish_reason": result.finish_reason,
            "interrupted": result.interrupted,
            "usage": result.usage,
            "timings": result.timings,
        });
        out.data(&value, String::new);
    } else {
        println!();
        if let Some(s) = stats_line(&result) {
            out.info(s);
        }
    }
    finish(session, &target, args.unload, out).await;
    if result.interrupted {
        return Err(CliError::from_code(
            Exit::General,
            "cli.aborted",
            Default::default(),
        ));
    }
    Ok(())
}

pub async fn chat(session: &Session, args: &ChatArgs, out: &Out) -> CliResult<()> {
    let target = ensure_loaded(session, &args.model, &args.gen, out).await?;
    let interactive = std::io::stdin().is_terminal();
    if interactive && !out.quiet {
        eprintln!(
            "Chatting with {} on {} (/bye to leave, /clear to reset)",
            target.alias, target.server_id
        );
    }
    let mut messages = base_messages(&args.gen);
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Option<String>>(1);
    // stdin is read on a thread: a blocking read must not stall the runtime (Ctrl+C).
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut lines = stdin.lock().lines();
        loop {
            let line = lines.next().and_then(Result::ok);
            let end = line.is_none();
            if tx.blocking_send(line).is_err() || end {
                break;
            }
        }
    });
    loop {
        if interactive {
            eprint!(">>> ");
            let _ = std::io::stderr().flush();
        }
        let line = tokio::select! {
            _ = tokio::signal::ctrl_c() => None,
            l = rx.recv() => l.flatten(),
        };
        let Some(line) = line else {
            if interactive {
                eprintln!();
            }
            break;
        };
        let text = line.trim();
        match text {
            "" => continue,
            "/bye" | "/exit" | "/quit" => break,
            "/clear" => {
                messages = base_messages(&args.gen);
                out.info("Conversation cleared.");
                continue;
            }
            _ => {}
        }
        messages.push(json!({"role": "user", "content": text}));
        let mut stdout = std::io::stdout();
        let sink: Option<&mut dyn Write> = if out.json { None } else { Some(&mut stdout) };
        let reply = match complete(&target, &messages, &args.gen, sink, out).await {
            Ok(r) => r,
            Err(e) => {
                finish(session, &target, args.unload, out).await;
                return Err(e);
            }
        };
        messages.push(json!({"role": "assistant", "content": reply.content}));
        if out.json {
            out.json_line(&json!({
                "role": "assistant",
                "content": reply.content,
                "reasoning_content": if reply.reasoning.is_empty() { Value::Null } else { json!(reply.reasoning) },
                "finish_reason": reply.finish_reason,
                "interrupted": reply.interrupted,
                "usage": reply.usage,
                "timings": reply.timings,
            }));
        } else {
            println!();
            if interactive {
                if let Some(s) = stats_line(&reply) {
                    out.info(s);
                }
            }
        }
    }
    finish(session, &target, args.unload, out).await;
    Ok(())
}
