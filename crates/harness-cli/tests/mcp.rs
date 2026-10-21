//! `harness mcp` over stdio: the real binary as an MCP server, driven by a minimal MCP client
//! (newline-delimited JSON-RPC on the child's stdin/stdout), against temporary data dirs.
//!
//! The `#[ignore]` end-to-end test needs a real llama-server and model (ports 18900+):
//!
//! HS_LLAMA_SERVER=<llama-server> HS_TEST_MODEL=<gguf> cargo test -p harness-cli --test mcp
//!   -- --ignored --nocapture --test-threads=1

mod common;

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

use common::{cmd, data_dir, free_port, llama_server, test_model, Serve};
use serde_json::{json, Value};

/// A running `harness mcp` and a tiny MCP client for it.
struct McpClient {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
    next_id: u64,
}

impl McpClient {
    fn start(data_dir: &Path, args: &[&str]) -> Self {
        let mut child = cmd(data_dir)
            .arg("mcp")
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let out = BufReader::new(child.stdout.take().unwrap());
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            for l in out.lines().map_while(Result::ok) {
                let _ = tx.send(l);
            }
        });
        McpClient {
            child,
            stdin: Some(stdin),
            lines: rx,
            next_id: 1,
        }
    }

    fn send(&mut self, msg: &Value) {
        let stdin = self.stdin.as_mut().unwrap();
        writeln!(stdin, "{}", msg).unwrap();
        stdin.flush().unwrap();
    }

    /// Sends a request and waits for the response with the same id (every stdout line must be
    /// a JSON-RPC message).
    fn request(&mut self, method: &str, params: Value, timeout: Duration) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            let line = self
                .lines
                .recv_timeout(left)
                .unwrap_or_else(|_| panic!("no reply to {method} within {timeout:?}"));
            let msg: Value = serde_json::from_str(&line)
                .unwrap_or_else(|e| panic!("stdout line is not JSON-RPC ({e}): {line}"));
            assert_eq!(msg["jsonrpc"], "2.0", "{line}");
            if msg["id"] == id {
                return msg;
            }
        }
    }

    fn call(&mut self, tool: &str, args: Value, timeout: Duration) -> Value {
        let r = self.request(
            "tools/call",
            json!({"name": tool, "arguments": args}),
            timeout,
        );
        assert!(r.get("error").is_none(), "{tool}: {r}");
        r["result"].clone()
    }

    fn call_ok(&mut self, tool: &str, args: Value, timeout: Duration) -> Value {
        let r = self.call(tool, args, timeout);
        assert_eq!(r["isError"], false, "{tool}: {r}");
        // The same JSON is given as text for text-only clients.
        let text: Value = serde_json::from_str(r["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(text, r["structuredContent"]);
        r["structuredContent"].clone()
    }

    fn initialize(&mut self) -> Value {
        let init = self.request(
            "initialize",
            json!({"protocolVersion": "2025-06-18", "capabilities": {},
                   "clientInfo": {"name": "harness-test", "version": "1"}}),
            Duration::from_secs(30),
        );
        self.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        init
    }

    /// Closes stdin (the MCP shutdown signal) and waits for the process to exit.
    fn close(mut self) -> i32 {
        drop(self.stdin.take());
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status.code().unwrap_or(-1);
            }
            assert!(
                std::time::Instant::now() < deadline,
                "harness mcp did not exit"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

impl Drop for McpClient {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn tool_names(list: &Value) -> Vec<String> {
    list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn stdio_handshake_scopes_and_read_tools() {
    let dir = data_dir(None, free_port(18900));
    let mut mcp = McpClient::start(dir.path(), &["--headless"]);

    let init = mcp.initialize();
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18", "{init}");
    assert_eq!(init["result"]["serverInfo"]["name"], "harness");
    assert!(init["result"]["capabilities"]["tools"].is_object());

    // Default scopes: read + servers:write (no downloads/deletions).
    let list = mcp.request("tools/list", json!({}), Duration::from_secs(10));
    let names = tool_names(&list);
    for t in [
        "list_models",
        "get_model_info",
        "hardware_status",
        "recommend_models",
        "load_model",
        "unload_model",
        "list_servers",
        "start_server",
        "stop_server",
        "get_endpoint",
        "get_metrics",
        "get_logs",
        "run_benchmark",
        "compare_performance",
    ] {
        assert!(names.contains(&t.to_string()), "{t} missing: {names:?}");
    }
    assert!(!names.contains(&"pull_model".to_string()));
    assert!(!names.contains(&"delete_model".to_string()));
    let load = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "load_model")
        .unwrap();
    assert_eq!(load["annotations"]["readOnlyHint"], false);
    assert_eq!(load["annotations"]["destructiveHint"], false);
    assert_eq!(load["inputSchema"]["required"], json!(["model"]));

    // A hidden tool is refused as a tool error naming the scope.
    let pull = mcp.call(
        "pull_model",
        json!({"repo": "a/b"}),
        Duration::from_secs(10),
    );
    assert_eq!(pull["isError"], true);
    assert_eq!(
        pull["structuredContent"]["error"]["code"],
        "api.insufficient_scope"
    );
    assert_eq!(
        pull["structuredContent"]["error"]["params"]["scope"],
        "downloads:write"
    );

    // Read tools against the temporary instance.
    let models = mcp.call_ok("list_models", json!({}), Duration::from_secs(30));
    // (Models linked from other apps on this machine may show up; the shape is what counts.)
    assert_eq!(
        models["count"].as_u64().unwrap() as usize,
        models["models"].as_array().unwrap().len()
    );
    let running = mcp.call_ok("list_servers", json!({}), Duration::from_secs(30));
    assert_eq!(running["count"], 0);
    let all = mcp.call_ok(
        "list_servers",
        json!({"all": true}),
        Duration::from_secs(30),
    );
    assert!(all["servers"]
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s["status"] == "Stopped"));
    let hw = mcp.call_ok("hardware_status", json!({}), Duration::from_secs(60));
    assert!(
        hw["hardware"]["total_ram_bytes"].as_u64().unwrap() > 0,
        "{hw}"
    );
    let perf = mcp.call_ok("compare_performance", json!({}), Duration::from_secs(30));
    assert!(perf["benchmarks"].as_array().unwrap().is_empty());

    // Tool errors are results the model can read; bad arguments too.
    let missing = mcp.call(
        "get_model_info",
        json!({"model": "nope"}),
        Duration::from_secs(30),
    );
    assert_eq!(missing["isError"], true);
    assert_eq!(
        missing["structuredContent"]["error"]["code"],
        "api.not_found"
    );
    let bad = mcp.call("get_logs", json!({"server_id": 5}), Duration::from_secs(10));
    assert_eq!(
        bad["structuredContent"]["error"]["code"],
        "invalid_arguments"
    );
    let extra = mcp.call("list_servers", json!({"bogus": 1}), Duration::from_secs(10));
    assert_eq!(
        extra["structuredContent"]["error"]["code"],
        "invalid_arguments"
    );
    // Protocol errors stay JSON-RPC errors.
    let unknown = mcp.request(
        "tools/call",
        json!({"name": "no_such_tool", "arguments": {}}),
        Duration::from_secs(10),
    );
    assert_eq!(unknown["error"]["code"], -32602);
    let method = mcp.request("resources/list", json!({}), Duration::from_secs(10));
    assert_eq!(method["error"]["code"], -32601);

    // A modern (2026-07-28) request on the same stream: stateless, `_meta` per request.
    let meta = json!({"io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {},
        "io.modelcontextprotocol/clientInfo": {"name": "t", "version": "1"}});
    let discover = mcp.request(
        "server/discover",
        json!({"_meta": meta}),
        Duration::from_secs(10),
    );
    assert_eq!(discover["result"]["resultType"], "complete", "{discover}");
    assert!(discover["result"]["supportedVersions"]
        .as_array()
        .unwrap()
        .contains(&json!("2026-07-28")));
    assert!(discover["result"]["ttlMs"].as_u64().is_some());
    let modern = mcp.request(
        "tools/list",
        json!({"_meta": meta}),
        Duration::from_secs(10),
    );
    assert_eq!(modern["result"]["resultType"], "complete");
    assert_eq!(modern["result"]["cacheScope"], "private");
    assert_eq!(
        modern["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
        "harness"
    );
    let future = mcp.request(
        "tools/list",
        json!({"_meta": {"io.modelcontextprotocol/protocolVersion": "2099-01-01",
            "io.modelcontextprotocol/clientCapabilities": {}}}),
        Duration::from_secs(10),
    );
    assert_eq!(future["error"]["code"], -32022);
    assert!(future["error"]["data"]["supported"]
        .as_array()
        .unwrap()
        .contains(&json!("2026-07-28")));

    assert_eq!(mcp.close(), 0);
    // The temporary instance released the data directory.
    assert!(!dir.path().join("instance.json").exists());
}

#[test]
fn stdio_scope_option_and_missing_instance() {
    let dir = data_dir(None, free_port(18900));
    // Everything incl. models:write.
    let mut mcp = McpClient::start(
        dir.path(),
        &["--headless", "--scope", "read,servers:write,models:write"],
    );
    mcp.initialize();
    let names = tool_names(&mcp.request("tools/list", json!({}), Duration::from_secs(10)));
    assert!(names.contains(&"pull_model".to_string()));
    assert!(names.contains(&"delete_model".to_string()));
    assert_eq!(mcp.close(), 0);

    // Read only.
    let mut mcp = McpClient::start(dir.path(), &["--headless", "--scope", "read"]);
    mcp.initialize();
    let names = tool_names(&mcp.request("tools/list", json!({}), Duration::from_secs(10)));
    assert!(names.contains(&"get_endpoint".to_string()));
    assert!(!names.contains(&"load_model".to_string()));
    assert_eq!(mcp.close(), 0);

    // Without a running instance and without --headless the server still speaks MCP; tools
    // explain how to start Harness.
    let mut mcp = McpClient::start(dir.path(), &[]);
    mcp.initialize();
    let r = mcp.call("list_models", json!({}), Duration::from_secs(10));
    assert_eq!(r["isError"], true);
    assert_eq!(r["structuredContent"]["error"]["code"], "cli.no_instance");
    assert!(r["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("harness serve"));
    assert_eq!(mcp.close(), 0);

    // Bad --scope: usage error.
    let r = common::run(dir.path(), &["mcp", "--scope", "root"]);
    assert_eq!(r.code, 2, "{}", r.stderr);
}

/// Posts a chat completion to an OpenAI-compatible base URL (what an agent does with
/// `get_endpoint`).
pub fn chat(base_url: &str, model: &str) -> Value {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let resp = reqwest::Client::new()
            .post(format!("{}/chat/completions", base_url))
            .json(&json!({"model": model, "max_tokens": 16, "temperature": 0,
                "messages": [{"role": "user", "content": "Say hello in three words."}]}))
            .send()
            .await
            .unwrap();
        assert!(resp.status().is_success(), "{}", resp.status());
        resp.json::<Value>().await.unwrap()
    })
}

#[test]
#[ignore]
fn e2e_mcp_stdio_loads_a_real_model() {
    let (Some(bin), Some(model)) = (llama_server(), test_model()) else {
        eprintln!("HS_LLAMA_SERVER / HS_TEST_MODEL not set, skipping");
        return;
    };
    let dir = data_dir(Some(&bin), free_port(18900));
    // The model's folder is a library directory (get_model_info / recommend_models read the
    // library).
    let settings_path = dir.path().join("config/settings.json");
    let mut settings: Value =
        serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
    settings["extra_models_dirs"] = json!([model.parent().unwrap()]);
    std::fs::write(&settings_path, settings.to_string()).unwrap();
    // Through a running owner (`harness serve`), as with the GUI.
    let mut serve = Serve::start(dir.path(), free_port(18960));
    let mut mcp = McpClient::start(dir.path(), &[]);
    mcp.initialize();
    let model = model.to_str().unwrap();

    let t0 = std::time::Instant::now();
    let loaded = mcp.call_ok(
        "load_model",
        json!({"model": model}),
        Duration::from_secs(300),
    );
    eprintln!("load_model ({:.1}s): {loaded}", t0.elapsed().as_secs_f32());
    assert_eq!(loaded["ready"], true);
    assert_eq!(loaded["created_definition"], true);
    let server_id = loaded["server_id"].as_str().unwrap().to_string();

    let ep = mcp.call_ok(
        "get_endpoint",
        json!({"model": model}),
        Duration::from_secs(30),
    );
    eprintln!("get_endpoint: {ep}");
    assert_eq!(ep["kind"], "direct");
    assert!(ep["managed"].is_null());
    let base = ep["base_url"].as_str().unwrap().to_string();
    let served = ep["model"].as_str().unwrap().to_string();
    assert_eq!(ep["server_id"], server_id.as_str());
    let port: u16 = base
        .rsplit(':')
        .next()
        .and_then(|p| p.split('/').next())
        .and_then(|p| p.parse().ok())
        .unwrap();
    // A new definition takes a port from Harness's automatic range, not `settings.server.port`.
    assert!(
        harness_core::ports::auto_range().contains(&port),
        "server port {port}"
    );

    let reply = chat(&base, &served);
    let text = reply["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or_default();
    eprintln!("chat: {text}");
    assert!(reply["usage"]["completion_tokens"].as_u64().unwrap() > 0);

    // Alias works too; the same server is found.
    let by_alias = mcp.call_ok(
        "get_endpoint",
        json!({"model": loaded["definition"]["alias"]}),
        Duration::from_secs(30),
    );
    assert_eq!(by_alias["server_id"], server_id.as_str());
    std::thread::sleep(Duration::from_millis(1500));
    let metrics = mcp.call_ok(
        "get_metrics",
        json!({"server_id": server_id}),
        Duration::from_secs(30),
    );
    assert_eq!(metrics["snapshot"]["server_online"], true, "{metrics}");
    let logs = mcp.call_ok(
        "get_logs",
        json!({"server_id": server_id, "lines": 20}),
        Duration::from_secs(30),
    );
    assert!(!logs["lines"].as_array().unwrap().is_empty());
    let info = mcp.call_ok(
        "get_model_info",
        json!({"model": model}),
        Duration::from_secs(30),
    );
    assert_eq!(info["model"]["loaded_on"], json!([server_id]));
    assert!(info["memory"]["vram_needed_gib"].as_f64().unwrap() > 0.0);
    let rec = mcp.call_ok(
        "recommend_models",
        json!({"context_size": 8192}),
        Duration::from_secs(60),
    );
    eprintln!("recommend_models: {}", rec["recommendations"][0]);
    assert!(!rec["recommendations"].as_array().unwrap().is_empty());

    let unloaded = mcp.call_ok(
        "unload_model",
        json!({"model": model}),
        Duration::from_secs(60),
    );
    assert_eq!(unloaded["stopped"], json!([server_id]));
    assert!(!common::port_open(port), "llama-server still listening");
    let none = mcp.call(
        "get_endpoint",
        json!({"model": model}),
        Duration::from_secs(30),
    );
    assert_eq!(none["structuredContent"]["error"]["code"], "mcp.not_loaded");

    // Short benchmark through llama-bench, then the comparison sees it.
    let bench = mcp.call_ok(
        "run_benchmark",
        json!({"model": model, "n_prompt": [64], "n_gen": [16], "repetitions": 1}),
        Duration::from_secs(600),
    );
    eprintln!("run_benchmark: {bench}");
    assert_eq!(bench["status"], "completed");
    let cmp = mcp.call_ok("compare_performance", json!({}), Duration::from_secs(30));
    assert!(
        cmp["benchmarks"][0]["best_gen_tps"].as_f64().unwrap() > 0.0,
        "{cmp}"
    );

    assert_eq!(mcp.close(), 0);
    let r = common::run(dir.path(), &["shutdown", "--json"]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    let (code, _) = serve.wait_exit(Duration::from_secs(30));
    assert_eq!(code, 0);
}

#[test]
#[ignore]
fn e2e_mcp_stdio_headless_instance_stops_its_servers() {
    let (Some(bin), Some(model)) = (llama_server(), test_model()) else {
        eprintln!("HS_LLAMA_SERVER / HS_TEST_MODEL not set, skipping");
        return;
    };
    let dir = data_dir(Some(&bin), free_port(18900));
    let mut mcp = McpClient::start(dir.path(), &["--headless"]);
    mcp.initialize();
    let loaded = mcp.call_ok(
        "load_model",
        json!({"model": model.to_str().unwrap()}),
        Duration::from_secs(300),
    );
    let base = loaded["endpoint"].as_str().unwrap().to_string();
    let reply = chat(&base, loaded["model"].as_str().unwrap());
    assert!(reply["usage"]["completion_tokens"].as_u64().unwrap() > 0);
    let port: u16 = base
        .rsplit(':')
        .next()
        .and_then(|p| p.split('/').next())
        .and_then(|p| p.parse().ok())
        .unwrap();
    assert!(common::port_open(port));
    // Closing stdin ends the temporary instance and its llama-server.
    assert_eq!(mcp.close(), 0);
    assert!(
        !common::port_open(port),
        "llama-server outlived harness mcp"
    );
    assert!(!dir.path().join("instance.json").exists());
}

/// Harness's own tools over stdio: off by default, offered with `settings.tools.mcp_expose`,
/// the policy decides each call (MCP cannot ask), calls land in `tool_calls` under the client's
/// name from `initialize`.
#[test]
fn stdio_offers_harness_tools_only_when_opted_in() {
    let dir = data_dir(None, free_port(18900));
    let work = tempfile::tempdir().unwrap();
    std::fs::write(work.path().join("a.txt"), "hello stdio\n").unwrap();
    let set = |expose: bool, policy: &str| {
        let path = dir.path().join("config/settings.json");
        let mut s: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        s["tools"] = json!({"policy": policy, "trusted": ["read_file"],
            "workdir": work.path(), "mcp_expose": expose});
        std::fs::write(&path, serde_json::to_string_pretty(&s).unwrap()).unwrap();
    };

    // Default (setting off): the management tools only.
    set(false, "allow_trusted");
    let mut mcp = McpClient::start(dir.path(), &["--headless"]);
    mcp.initialize();
    let names = tool_names(&mcp.request("tools/list", json!({}), Duration::from_secs(10)));
    assert!(names.contains(&"list_models".to_string()));
    assert!(!names.contains(&"read_file".to_string()), "{names:?}");
    assert_eq!(mcp.close(), 0);

    // Opted in: the default stdio scopes include `tools`.
    set(true, "allow_trusted");
    let mut mcp = McpClient::start(dir.path(), &["--headless"]);
    mcp.initialize();
    let list = mcp.request("tools/list", json!({}), Duration::from_secs(30));
    let names = tool_names(&list);
    for n in [
        "read_file",
        "write_file",
        "exec_shell_command",
        "calculator",
    ] {
        assert!(names.contains(&n.to_string()), "{n}: {names:?}");
    }
    let tool = |n: &str| {
        list["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == n)
            .unwrap()
            .clone()
    };
    assert_eq!(tool("read_file")["annotations"]["readOnlyHint"], true);
    assert_eq!(
        tool("exec_shell_command")["annotations"]["destructiveHint"],
        true
    );
    let t = Duration::from_secs(30);
    let read = mcp.call("read_file", json!({"path": "a.txt"}), t);
    assert_eq!(read["isError"], false, "{read}");
    assert!(read["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("hello stdio"));
    let calc = mcp.call("calculator", json!({"expression": "2+3"}), t);
    assert_eq!(calc["isError"], false, "{calc}");
    // Not trusted: a tool error that says how to trust it; nothing was written.
    let write = mcp.call("write_file", json!({"path": "w.txt", "content": "x"}), t);
    assert_eq!(write["isError"], true, "{write}");
    assert!(write["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("Settings > Tools"));
    assert!(!work.path().join("w.txt").exists());
    assert_eq!(mcp.close(), 0);

    // `--scope` without `tools` hides them again.
    let mut mcp = McpClient::start(dir.path(), &["--headless", "--scope", "read"]);
    mcp.initialize();
    let names = tool_names(&mcp.request("tools/list", json!({}), Duration::from_secs(10)));
    assert!(!names.contains(&"read_file".to_string()));
    assert_eq!(mcp.close(), 0);

    // The log has the calls under the client name `harness-test` (from `initialize`).
    let db = harness_core::analytics::AnalyticsDb::open(&dir.path().join("analytics.db")).unwrap();
    let rows = db
        .tool_calls(&harness_core::analytics::ToolCallQuery::default())
        .unwrap();
    let logged = |tool: &str, err: Option<&str>| {
        rows.iter().any(|r| {
            r.tool == tool && r.client == "mcp:harness-test" && r.error_code.as_deref() == err
        })
    };
    assert!(logged("read_file", None), "{rows:?}");
    assert!(logged("write_file", Some("approval_required")), "{rows:?}");
}

/// Runs an agent CLI (`claude`, `codex`; npm `.cmd` shims on Windows) with extra env vars.
/// `None` when it is not installed.
fn agent_cli(name: &str, args: &[&str], env: &[(&str, &Path)], cwd: &Path) -> Option<String> {
    let mut c = if cfg!(windows) {
        let mut c = std::process::Command::new("cmd");
        c.arg("/C").arg(name);
        c
    } else {
        std::process::Command::new(name)
    };
    c.args(args).current_dir(cwd).stdin(Stdio::null());
    for (k, v) in env {
        c.env(k, v);
    }
    let out = c.output().ok()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if !out.status.success() && (text.contains("not recognized") || text.contains("not found")) {
        return None;
    }
    Some(text)
}

/// "Harness MCP'yi ekle" end to end with the real Claude Code CLI: the integration writes a
/// temporary `CLAUDE_CONFIG_DIR/.claude.json` (never the user's), and `claude mcp list` starts
/// / connects to the configured server (stdio and HTTP). Needs `claude` on PATH.
#[test]
#[ignore]
fn e2e_claude_code_connects_to_harness_mcp() {
    use harness_core::integrations::harness_mcp::{apply_in, McpRoots, McpTransport};
    let dir = data_dir(None, free_port(18900));
    let cfg = tempfile::tempdir().unwrap();
    let roots = McpRoots {
        home: Some(cfg.path().to_path_buf()),
        claude_config_dir: Some(cfg.path().to_path_buf()),
        codex_home: None,
        ledger: None,
    };
    let env = [("CLAUDE_CONFIG_DIR", cfg.path())];

    // stdio: Claude Code starts `harness mcp` itself.
    let stdio = McpTransport::Stdio {
        command: common::bin().to_string(),
        args: vec!["mcp".into(), "--headless".into()],
        env: [
            (
                "HARNESS_DATA_DIR".to_string(),
                dir.path().to_string_lossy().to_string(),
            ),
            ("HARNESS_MEMORY_SECRETS".to_string(), "1".to_string()),
        ]
        .into(),
    };
    apply_in(&roots, "claude-code", &stdio).unwrap();
    let Some(list) = agent_cli("claude", &["mcp", "list"], &env, cfg.path()) else {
        eprintln!("claude not installed, skipping");
        return;
    };
    eprintln!("claude mcp list (stdio):\n{list}");
    let line = list.lines().find(|l| l.starts_with("harness:")).unwrap();
    assert!(line.contains("Connected"), "{list}");
    let get = agent_cli("claude", &["mcp", "get", "harness"], &env, cfg.path()).unwrap();
    assert!(
        get.contains("User config") && get.contains("stdio"),
        "{get}"
    );

    // HTTP: the Control API's /mcp of a running instance, with a read-only named token.
    let mut serve = Serve::start(dir.path(), free_port(18960));
    let r = common::run(
        dir.path(),
        &["api-token", "create", "claude", "--scope", "read", "--json"],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    let token = r.json()["token"].as_str().unwrap().to_string();
    let url = format!("{}/mcp", serve.ready["base_url"].as_str().unwrap());
    apply_in(
        &roots,
        "claude-code",
        &McpTransport::Http {
            url: url.clone(),
            token: Some(token),
        },
    )
    .unwrap();
    let list = agent_cli("claude", &["mcp", "list"], &env, cfg.path()).unwrap();
    eprintln!("claude mcp list (http):\n{list}");
    let line = list.lines().find(|l| l.starts_with("harness:")).unwrap();
    assert!(line.contains(&url) && line.contains("Connected"), "{list}");

    let r = common::run(dir.path(), &["shutdown", "--json"]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    serve.wait_exit(Duration::from_secs(30));
}

/// Codex reads what the integration writes into a temporary `CODEX_HOME/config.toml`
/// (`codex mcp list` does not connect; `codex mcp get --json` shows the parsed entry).
#[test]
#[ignore]
fn e2e_codex_reads_harness_mcp_entry() {
    use harness_core::integrations::harness_mcp::{apply_in, McpRoots, McpTransport};
    let home = tempfile::tempdir().unwrap();
    // Codex refuses helper binaries under the temp dir but still reads the config.
    let roots = McpRoots {
        home: Some(home.path().to_path_buf()),
        claude_config_dir: None,
        codex_home: Some(home.path().to_path_buf()),
        ledger: None,
    };
    std::fs::write(
        home.path().join("config.toml"),
        "# user settings\n[mcp_servers.other]\ncommand = \"npx\"\n",
    )
    .unwrap();
    apply_in(
        &roots,
        "codex",
        &McpTransport::Stdio {
            command: common::bin().to_string(),
            args: vec!["mcp".into()],
            env: Default::default(),
        },
    )
    .unwrap();
    let env = [("CODEX_HOME", home.path())];
    let Some(out) = agent_cli(
        "codex",
        &["mcp", "get", "harness", "--json"],
        &env,
        home.path(),
    ) else {
        eprintln!("codex not installed, skipping");
        return;
    };
    eprintln!("codex mcp get harness --json:\n{out}");
    let json_start = out.find('{').expect("JSON from codex");
    let v: Value = serde_json::Deserializer::from_str(&out[json_start..])
        .into_iter::<Value>()
        .next()
        .unwrap()
        .unwrap();
    assert_eq!(v["transport"]["command"], common::bin());
    assert_eq!(v["transport"]["args"], json!(["mcp"]));
    let list = agent_cli("codex", &["mcp", "list"], &env, home.path()).unwrap();
    assert!(list.contains("harness") && list.contains("other"), "{list}");
}

/// With the gateway on, `get_endpoint` hands out the managed endpoint and the alias even before
/// the model is loaded; the first request through it loads the model (ports 19200+).
#[test]
#[ignore]
fn e2e_mcp_get_endpoint_managed_via_gateway() {
    let (Some(bin), Some(model)) = (llama_server(), test_model()) else {
        eprintln!("HS_LLAMA_SERVER / HS_TEST_MODEL not set, skipping");
        return;
    };
    let dir = data_dir(Some(&bin), free_port(19200));
    let gw_port = free_port(19260);
    let mut serve = Serve::start_with(
        dir.path(),
        free_port(19240),
        &["--gateway", "--gateway-port", &gw_port.to_string()],
    );
    let gw_port = serve.gateway_port();
    // A definition for the model (the dry run creates it; nothing is loaded).
    let r = common::run(
        dir.path(),
        &[
            "--json",
            "launch",
            "claude",
            "--model",
            model.to_str().unwrap(),
            "--dry-run",
        ],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    let alias = r.json()["model"].as_str().unwrap().to_string();

    let mut mcp = McpClient::start(dir.path(), &[]);
    mcp.initialize();
    let ep = mcp.call_ok(
        "get_endpoint",
        json!({"model": alias}),
        Duration::from_secs(30),
    );
    eprintln!("get_endpoint: {ep}");
    assert_eq!(ep["kind"], "managed");
    assert_eq!(ep["model"], alias.as_str());
    assert_eq!(ep["loaded"], false);
    let base = ep["base_url"].as_str().unwrap().to_string();
    assert_eq!(base, format!("http://127.0.0.1:{gw_port}/v1"));
    assert!(ep["messages_url"]
        .as_str()
        .unwrap()
        .ends_with("/v1/messages"));

    // JIT: the first request through the managed endpoint loads the model.
    let reply = tokio::runtime::Runtime::new().unwrap().block_on(async {
        let resp = reqwest::Client::new()
            .post(format!("{base}/chat/completions"))
            .json(&json!({"model": alias, "max_tokens": 8,
                "messages": [{"role": "user", "content": "Say hello."}]}))
            .send()
            .await
            .unwrap();
        let status = resp.status();
        let body = resp.text().await.unwrap();
        assert!(status.is_success(), "{status}: {body}");
        serde_json::from_str::<Value>(&body).unwrap()
    });
    eprintln!("chat: {reply}");
    assert!(reply["usage"]["completion_tokens"].as_u64().unwrap() > 0);
    let ep = mcp.call_ok(
        "get_endpoint",
        json!({"model": alias}),
        Duration::from_secs(30),
    );
    assert_eq!(ep["kind"], "managed");
    assert_eq!(ep["loaded"], true, "{ep}");
    assert!(ep["direct"].is_object(), "{ep}");

    assert_eq!(mcp.close(), 0);
    assert_eq!(common::run(dir.path(), &["shutdown", "--json"]).code, 0);
    let (code, _) = serve.wait_exit(Duration::from_secs(60));
    assert_eq!(code, 0);
}

/// A conversation of `turns` question/answer pairs, each question stating a distinct fact.
fn seed_conversation(db_path: &Path, title: &str, alias: &str, facts: &[&str]) -> String {
    use harness_core::state::{AppendMessage, NewConversation, StateDb, VariantInput};
    let db = StateDb::open(db_path).unwrap();
    let conv = db
        .create_conversation(&NewConversation {
            title: Some(title.into()),
            model_alias: Some(alias.into()),
            ..Default::default()
        })
        .unwrap();
    let mut parent: Option<String> = None;
    for (i, fact) in facts.iter().enumerate() {
        // About 90 words per question, so a handful of turns is a few thousand tokens.
        let filler = "I am writing this down so that nothing gets lost between our meetings, \
            because last quarter several details were forgotten and we had to redo the planning \
            twice, which cost the team a lot of time and goodwill with the stakeholders. ";
        let question = format!(
            "Note number {}. {fact} {filler}Please confirm you have it.",
            i + 1
        );
        let user = db
            .append_message(AppendMessage {
                conversation_id: conv.id.clone(),
                parent_id: parent.clone(),
                role: "user".into(),
                variant: VariantInput {
                    content: question,
                    ..Default::default()
                },
                id: None,
                variant_id: None,
                any_parent_variant: false,
            })
            .unwrap();
        let answer = format!(
            "Confirmed, I have noted: {fact} I will keep this in mind for the rest of the \
             planning and I will flag anything that conflicts with it, so that the meeting notes \
             stay consistent and nobody has to redo work because of a forgotten detail."
        );
        let reply = db
            .append_message(AppendMessage {
                conversation_id: conv.id.clone(),
                parent_id: Some(user.id.clone()),
                role: "assistant".into(),
                variant: VariantInput {
                    content: answer,
                    model_alias: Some(alias.into()),
                    ..Default::default()
                },
                id: None,
                variant_id: None,
                any_parent_variant: false,
            })
            .unwrap();
        parent = Some(reply.id.clone());
    }
    conv.id
}

/// Conversations through the CLI and the MCP tools with a real model: the summary request goes
/// through the gateway of `harness serve --gateway`, to a real llama-server (ports 18990+).
#[test]
#[ignore]
fn e2e_conversations_compaction_cli_and_mcp() {
    let (Some(bin), Some(model)) = (llama_server(), test_model()) else {
        eprintln!("HS_LLAMA_SERVER / HS_TEST_MODEL not set, skipping");
        return;
    };
    let dir = data_dir(Some(&bin), free_port(18990));
    let settings_path = dir.path().join("config/settings.json");
    let mut settings: Value =
        serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
    settings["server"]["default_ctx"] = json!(4096);
    std::fs::write(&settings_path, settings.to_string()).unwrap();
    let gw_port = free_port(19040);
    let mut serve = Serve::start_with(
        dir.path(),
        free_port(19020),
        &["--gateway", "--gateway-port", &gw_port.to_string()],
    );

    // A request through the gateway loads the model (its definition's alias) at 4096 context.
    let r = common::run(
        dir.path(),
        &[
            "run",
            model.to_str().unwrap(),
            "Reply with OK.",
            "--max-tokens",
            "4",
            "--json",
        ],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    let alias = r.json()["model"].as_str().unwrap().to_string();

    let facts = [
        "The project code name is Bluefin Heron.",
        "The total budget is 48,200 euros.",
        "The team lead is Marisol Quinteros.",
        "The launch is on 17 March at 09:40.",
        "The main risk is the supplier in Gdansk.",
        "The test environment is called Kestrel.",
        "The review board meets every second Tuesday.",
    ];
    let db_path = dir.path().join("state.db");
    let a = seed_conversation(&db_path, "Planning notes A", &alias, &facts);
    let b = seed_conversation(&db_path, "Planning notes B", &alias, &facts);

    // --- CLI ---
    let ls = common::run(dir.path(), &["conversations", "ls", "Planning", "--json"]);
    assert_eq!(ls.code, 0, "{}", ls.stderr);
    assert_eq!(ls.json().as_array().unwrap().len(), 2, "{}", ls.stdout);
    let show = common::run(dir.path(), &["conversations", "show", &a]);
    assert_eq!(show.code, 0, "{}", show.stderr);
    assert!(show.stdout.contains("Bluefin Heron"), "{}", show.stdout);
    let md = common::run(
        dir.path(),
        &["conversations", "export", &a, "--format", "markdown"],
    );
    assert!(md.stdout.contains("# Planning notes A"), "{}", md.stdout);
    let docs = common::run(dir.path(), &["conversations", "docs", &a, "--json"]);
    assert_eq!(docs.json(), json!([]));

    let t0 = std::time::Instant::now();
    let c = common::run(
        dir.path(),
        &[
            "conversations",
            "compact",
            &a,
            "--keep-turns",
            "1",
            "--json",
        ],
    );
    assert_eq!(c.code, 0, "{}\n{}", c.stdout, c.stderr);
    let done = c.json();
    eprintln!("cli compact ({:.1}s): {done}", t0.elapsed().as_secs_f32());
    let m = &done["marker"];
    let (before, after) = (
        m["tokens_before"].as_u64().unwrap(),
        m["tokens_after"].as_u64().unwrap(),
    );
    assert!(after < before, "{before} -> {after}");
    assert_eq!(m["kept_turns"], 1);
    assert_eq!(m["turns"], 6);
    let marker_id = done["marker_id"].as_str().unwrap().to_string();
    let view = common::run(dir.path(), &["conversations", "show", &a, "--json"]);
    let view = view.json();
    let summary = view["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == marker_id.as_str())
        .expect("marker message");
    let text = summary["variant"]["content"]
        .as_str()
        .unwrap()
        .to_lowercase();
    eprintln!("summary: {text}");
    assert!(text.contains("bluefin"), "the summary keeps the facts");
    assert!(text.contains("48,200") || text.contains("48200"), "{text}");
    // Undo and redo.
    let u = common::run(
        dir.path(),
        &["conversations", "undo", &a, &marker_id, "--json"],
    );
    assert_eq!(u.code, 0, "{}", u.stderr);
    assert_eq!(u.json()["undone"], true);
    let view = common::run(dir.path(), &["conversations", "show", &a, "--json"]).json();
    let marker = view["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == marker_id.as_str())
        .unwrap();
    assert_eq!(marker["variant"]["meta"]["compaction"]["undone"], true);
    let r = common::run(
        dir.path(),
        &["conversations", "undo", &a, &marker_id, "--redo", "--json"],
    );
    assert_eq!(r.json()["undone"], false);
    // A second compaction rolls the first summary into a new one.
    let again = common::run(
        dir.path(),
        &[
            "conversations",
            "compact",
            &a,
            "--keep-turns",
            "1",
            "--json",
        ],
    );
    assert_eq!(
        again.code, 0,
        "{}
{}",
        again.stdout, again.stderr
    );
    assert_eq!(again.json()["marker"]["previous_id"], marker_id.as_str());

    // --- MCP (stdio, connected to the running owner) ---
    let mut mcp = McpClient::start(dir.path(), &["--scope", "read,conversations"]);
    mcp.initialize();
    let list = mcp.request("tools/list", json!({}), Duration::from_secs(10));
    let names = tool_names(&list);
    for t in [
        "list_conversations",
        "get_conversation",
        "export_conversation",
        "compact_conversation",
    ] {
        assert!(names.contains(&t.to_string()), "{t}: {names:?}");
    }
    let l = mcp.call_ok(
        "list_conversations",
        json!({"query": "Planning"}),
        Duration::from_secs(20),
    );
    assert_eq!(l["count"], 2, "{l}");
    let g = mcp.call_ok(
        "get_conversation",
        json!({"id": b, "last": 4}),
        Duration::from_secs(20),
    );
    assert_eq!(g["message_count"], 14);
    assert_eq!(g["messages"].as_array().unwrap().len(), 4);
    assert_eq!(g["messages_omitted"], 10);
    let e = mcp.call_ok(
        "export_conversation",
        json!({"id": b}),
        Duration::from_secs(20),
    );
    assert!(e["markdown"].as_str().unwrap().contains("Kestrel"));
    let t0 = std::time::Instant::now();
    let c = mcp.call_ok(
        "compact_conversation",
        json!({"id": b, "keep_turns": 1, "instruction": "Keep every name, number and date."}),
        Duration::from_secs(300),
    );
    eprintln!("mcp compact ({:.1}s): {c}", t0.elapsed().as_secs_f32());
    assert!(c["tokens_after"].as_u64().unwrap() < c["tokens_before"].as_u64().unwrap());
    let g = mcp.call_ok(
        "get_conversation",
        json!({"id": b, "last": 3}),
        Duration::from_secs(20),
    );
    let marker = g["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["compaction"].is_object())
        .expect("compaction marker in the newest messages");
    assert!(marker["content"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("marisol"));
    // Without the scope the tools are gone.
    mcp.close();
    let mut limited = McpClient::start(dir.path(), &["--scope", "read"]);
    limited.initialize();
    let names = tool_names(&limited.request("tools/list", json!({}), Duration::from_secs(10)));
    assert!(
        !names.contains(&"list_conversations".to_string()),
        "{names:?}"
    );
    limited.close();

    assert_eq!(common::run(dir.path(), &["shutdown", "--json"]).code, 0);
    let (code, _) = serve.wait_exit(Duration::from_secs(60));
    assert_eq!(code, 0);
}

/// Real OpenCode, installed separately; XDG roots isolate config, auth, cache and state.
/// HS_OPENCODE_BIN must name the executable. Optional HS_OPENCODE_REPORT_DIR keeps evidence.
#[test]
#[ignore]
fn e2e_opencode_connects_to_harness_mcp() {
    use harness_core::integrations::harness_mcp::{apply_in, McpRoots, McpTransport};
    let executable = std::env::var_os("HS_OPENCODE_BIN").expect("set HS_OPENCODE_BIN");
    let profile = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let roots = McpRoots {
        home: Some(profile.path().to_path_buf()),
        ..Default::default()
    };
    let transport = McpTransport::Stdio {
        command: common::bin().to_string(),
        args: vec![
            "mcp".into(),
            "--headless".into(),
            "--scope".into(),
            "read".into(),
        ],
        env: [
            (
                "HARNESS_DATA_DIR".into(),
                data.path().to_string_lossy().into_owned(),
            ),
            ("HARNESS_MEMORY_SECRETS".into(), "1".into()),
        ]
        .into(),
    };
    apply_in(&roots, "opencode", &transport).unwrap();
    let run = |args: &[&str]| {
        std::process::Command::new(&executable)
            .args(args)
            .current_dir(profile.path())
            .env("XDG_CONFIG_HOME", profile.path().join(".config"))
            .env("XDG_DATA_HOME", profile.path().join("data"))
            .env("XDG_CACHE_HOME", profile.path().join("cache"))
            .env("XDG_STATE_HOME", profile.path().join("state"))
            .env("OPENCODE_TEST_HOME", profile.path())
            .env("TEMP", profile.path().join("tmp"))
            .env("TMP", profile.path().join("tmp"))
            .env("OPENCODE_DISABLE_PROJECT_CONFIG", "1")
            .env("OPENCODE_DISABLE_AUTOUPDATE", "1")
            .env_remove("OPENCODE_CONFIG")
            .env_remove("OPENCODE_CONFIG_CONTENT")
            .env_remove("OPENCODE_CONFIG_DIR")
            .env_remove("HARNESS_HOST")
            .env_remove("HARNESS_TOKEN")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };
    std::fs::create_dir_all(profile.path().join("tmp")).unwrap();
    let paths = run(&["debug", "paths"]);
    let path_text = String::from_utf8_lossy(&paths.stdout);
    eprintln!("OpenCode paths:\n{path_text}");
    assert!(paths.status.success());
    for label in ["home", "data", "cache", "config", "state"] {
        let line = path_text
            .lines()
            .find(|line| line.starts_with(label))
            .unwrap();
        assert!(line.contains(profile.path().to_str().unwrap()), "{line}");
    }
    let list = run(&["mcp", "list"]);
    let listing = format!(
        "{}{}",
        String::from_utf8_lossy(&list.stdout),
        String::from_utf8_lossy(&list.stderr)
    );
    eprintln!("OpenCode mcp list:\n{listing}");
    assert!(list.status.success(), "{listing}");
    assert!(
        listing.contains("harness") && listing.contains("connected"),
        "{listing}"
    );
    // Optional model session uses only the gateway's chat endpoint and a read-only MCP scope.
    // The local headless instance has no models or llama-server processes.
    if std::env::var_os("HS_OPENCODE_MODEL_TURN").is_some() {
        let config = profile.path().join(".config/opencode/opencode.json");
        let mut value: Value =
            serde_json::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
        value["provider"] = json!({"harness": {
            "npm": "@ai-sdk/openai-compatible", "name": "Harness",
            "options": {"baseURL": "http://127.0.0.1:11435/v1", "apiKey": "harness-local"},
            "models": {"qwen2.5-coder-14b-instruct-q4_k_m": {"name": "Qwen 2.5 Coder 14B", "limit": {"context": 32768, "output": 1024}}}
        }});
        value["tools"] = json!({"*": false, "harness_list_models": true});
        value["permission"] = json!({"*": "deny", "harness_list_models": "allow"});
        value["snapshot"] = json!(false);
        std::fs::write(config, serde_json::to_string_pretty(&value).unwrap()).unwrap();
        let output = run(&["run", "--format", "json", "--model", "harness/qwen2.5-coder-14b-instruct-q4_k_m",
            "Call harness_list_models exactly once. Report the returned model count. Do not use any other tool."]);
        let transcript = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        eprintln!("OpenCode model session:\n{transcript}");
        if let Some(report) = std::env::var_os("HS_OPENCODE_REPORT_DIR") {
            std::fs::create_dir_all(&report).unwrap();
            std::fs::write(
                Path::new(&report).join("opencode-model-turn.jsonl"),
                &transcript,
            )
            .unwrap();
        }
        // Connection validation is independent of the small model's tool-calling ability.
        eprintln!(
            "Model exit: {}; tool event: {}",
            output.status,
            transcript.contains("\"tool\":\"harness_list_models\"")
                || transcript.contains("\"tool\": \"harness_list_models\"")
        );
    }
}
