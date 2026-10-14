//! Harness's own MCP client (`harness_core::tools::mcp_client`) against real MCP servers:
//!
//! * the repo's own `harness mcp` (stdio) and its Control API endpoint `/mcp` (Streamable HTTP),
//!   each in the modern (2026-07-28) and the legacy (`initialize`) protocol;
//! * `@modelcontextprotocol/server-everything` over stdio and Streamable HTTP (`#[ignore]`:
//!   needs Node and the network for `npx`);
//! * a small Node protocol fixture (`#[ignore]`: needs Node) for what no real server does on
//!   demand: paginated `tools/list`, a tool list that changes with `list_changed`, and a record
//!   of the `notifications/cancelled` it receives.
//!
//! ```text
//! cargo test -p harness-cli --test mcp_client
//! cargo test -p harness-cli --test mcp_client -- --ignored --nocapture --test-threads=1
//! ```

mod common;

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{bin, cmd, free_port, Serve};
use harness_core::mcp::McpServerDef;
use harness_core::tools::mcp_client::{
    ConnectOptions, Era, McpConnection, McpError, McpHub, McpServerState, McpTarget,
};
use harness_core::tools::{
    CancelToken, LogSink, PermissionClass, RunOptions, ToolErrorCode, ToolInvocation, ToolOrigin,
    ToolPolicy, ToolRegistry, ToolService, ToolSettings,
};
use serde_json::{json, Value};

fn stdio_def(command: &str, args: &[&str], env: &[(&str, &str)]) -> McpServerDef {
    McpServerDef {
        command: Some(command.to_string()),
        args: args.iter().map(|s| s.to_string()).collect(),
        env: env
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        ..Default::default()
    }
}

/// `harness mcp` on the given data dir.
fn harness_mcp_def(data_dir: &std::path::Path) -> McpServerDef {
    stdio_def(
        bin(),
        &["mcp", "--scope", "read,servers:write,models:write"],
        &[
            ("HARNESS_DATA_DIR", data_dir.to_str().unwrap()),
            ("HARNESS_MEMORY_SECRETS", "1"),
        ],
    )
}

async fn connect(def: &McpServerDef, legacy_only: bool) -> Arc<McpConnection> {
    let target = McpTarget::from_def(def).unwrap();
    McpConnection::connect(
        "t",
        target,
        ConnectOptions {
            legacy_only,
            timeout: Duration::from_secs(90),
            ..Default::default()
        },
    )
    .await
    .unwrap_or_else(|e| panic!("connect failed: {e}"))
}

fn pid_alive(pid: u32) -> bool {
    if cfg!(windows) {
        let out = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).contains(&format!("\"{pid}\""))
    } else if cfg!(target_os = "linux") {
        // A killed child stays a zombie until its parent reaps it, and `kill -0` counts a
        // zombie as running: read its state instead.
        match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(stat) => stat
                .rsplit_once(')')
                .is_some_and(|(_, rest)| !rest.trim_start().starts_with('Z')),
            Err(_) => false,
        }
    } else {
        // No /proc on macOS: `ps` reports a zombie's state as `Z` (and nothing once reaped).
        std::process::Command::new("ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output()
            .map(|o| {
                let stat = String::from_utf8_lossy(&o.stdout);
                let stat = stat.trim();
                !stat.is_empty() && !stat.starts_with('Z')
            })
            .unwrap_or(false)
    }
}

async fn wait_dead(pid: u32) {
    let t = Instant::now();
    while pid_alive(pid) {
        assert!(
            t.elapsed() < Duration::from_secs(10),
            "process {pid} is still running"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn names(c: &McpConnection) -> Vec<String> {
    c.tools().into_iter().map(|t| t.name).collect()
}

async fn call(c: &McpConnection, tool: &str, args: Value) -> Result<String, String> {
    c.call_tool(tool, args, Duration::from_secs(60), &CancelToken::new())
        .await
        .map(|o| o.content)
        .map_err(|e| format!("{:?}: {}", e.code, e.message))
}

// ---------------------------------------------------------------------------------------
// harness-mcp: the repo's own server, both eras, both transports
// ---------------------------------------------------------------------------------------

/// stdio, modern: `server/discover` probe → 2026-07-28; list, a call that succeeds against a
/// running instance and one that fails without it; closing leaves no process.
#[tokio::test]
async fn stdio_modern_handshake_list_call_and_cleanup() {
    let dir = common::data_dir(None, free_port(18700));
    let mut serve = Serve::start(dir.path(), free_port(18710));

    let c = connect(&harness_mcp_def(dir.path()), false).await;
    assert_eq!(c.era(), Era::Modern("2026-07-28".into()));
    let info = c.server_info().unwrap();
    assert_eq!(info["name"], "harness");
    let tools = names(&c);
    assert!(tools.contains(&"list_models".to_string()), "{tools:?}");
    assert!(tools.len() >= 15, "{tools:?}");
    let pid = c.pid().unwrap();
    assert!(pid_alive(pid));

    // Call success: a real call into the running instance's Control API.
    let ok = call(&c, "list_models", json!({})).await.unwrap();
    let v: Value = serde_json::from_str(&ok).unwrap();
    assert!(
        v.get("library").is_some() || v.get("models").is_some(),
        "{ok}"
    );

    // Call error: the tool ran and reported a failure (an unknown tool is a protocol error).
    let err = call(&c, "get_model_info", json!({"model": "no-such-model.gguf"}))
        .await
        .unwrap_err();
    assert!(err.starts_with("Failed"), "{err}");
    let err = call(&c, "no_such_tool", json!({})).await.unwrap_err();
    assert!(
        err.contains("no_such_tool") || err.contains("Unknown"),
        "{err}"
    );

    // Closing ends the process.
    c.close(Duration::from_secs(3)).await;
    drop(c);
    wait_dead(pid).await;

    common::run(dir.path(), &["shutdown", "--json"]);
    serve.wait_exit(Duration::from_secs(30));
}

/// stdio, legacy: the same server through `initialize` (the era most servers speak).
#[tokio::test]
async fn stdio_legacy_handshake_negotiates_the_offered_version() {
    let dir = common::data_dir(None, free_port(18730));
    let c = connect(&harness_mcp_def(dir.path()), true).await;
    assert_eq!(c.era(), Era::Legacy("2025-11-25".into()));
    assert!(names(&c).contains(&"hardware_status".to_string()));
    // No instance runs: the tool answers with its own error, the connection stays usable.
    let err = call(&c, "hardware_status", json!({})).await.unwrap_err();
    assert!(err.starts_with("Failed"), "{err}");
    assert!(!c.is_broken());
}

/// Streamable HTTP against the Control API's `/mcp`: modern and legacy, with the bearer
/// token in the server's headers.
#[tokio::test]
async fn http_modern_and_legacy_against_the_control_api() {
    let dir = common::data_dir(None, free_port(18740));
    let mut serve = Serve::start(dir.path(), free_port(18750));
    let r = common::run(
        dir.path(),
        &[
            "api-token",
            "create",
            "mcp-client",
            "--scope",
            "read",
            "--json",
        ],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    let token = r.json()["token"].as_str().unwrap().to_string();
    let url = format!("{}/mcp", serve.ready["base_url"].as_str().unwrap());
    let def = McpServerDef {
        url: Some(url.clone()),
        headers: BTreeMap::from([("Authorization".to_string(), format!("Bearer {token}"))]),
        ..Default::default()
    };

    for (legacy_only, era) in [
        (false, Era::Modern("2026-07-28".into())),
        (true, Era::Legacy("2025-11-25".into())),
    ] {
        let c = connect(&def, legacy_only).await;
        assert_eq!(c.era(), era);
        let tools = names(&c);
        // The token has the read scope: only read tools are listed.
        assert!(tools.contains(&"list_models".to_string()), "{tools:?}");
        assert!(!tools.contains(&"load_model".to_string()), "{tools:?}");
        let ok = call(&c, "list_models", json!({})).await.unwrap();
        assert!(serde_json::from_str::<Value>(&ok).is_ok(), "{ok}");
        let err = call(&c, "get_model_info", json!({"model": "no-such-model.gguf"}))
            .await
            .unwrap_err();
        assert!(err.starts_with("Failed"), "{err}");
        c.close(Duration::from_secs(1)).await;
    }

    // A wrong token is refused with a message that names the way out, and is not mistaken for
    // an old server.
    let bad = McpServerDef {
        headers: BTreeMap::from([("Authorization".to_string(), "Bearer nope".to_string())]),
        ..def
    };
    let err = McpConnection::connect(
        "bad",
        McpTarget::from_def(&bad).unwrap(),
        ConnectOptions::default(),
    )
    .await
    .err()
    .expect("a wrong token must fail");
    assert!(
        matches!(
            err,
            McpError::Http { status: 401, .. } | McpError::Rpc { .. }
        ),
        "{err}"
    );

    common::run(dir.path(), &["shutdown", "--json"]);
    serve.wait_exit(Duration::from_secs(30));
}

/// Harness consuming its own `harness mcp` (default scopes, which include `tools`) with the
/// Harness tools offered: listing them must not start that data dir's MCP servers, or each
/// `harness mcp` would start the next one and the first never answers (found on screen).
#[tokio::test]
async fn harness_mcp_listed_in_its_own_config_does_not_start_itself_again() {
    let dir = common::data_dir(None, free_port(18780));
    let data = dir.path().to_str().unwrap().to_string();
    let env = [
        ("HARNESS_DATA_DIR", data.as_str()),
        ("HARNESS_MEMORY_SECRETS", "1"),
    ];
    let settings_path = dir.path().join("config/settings.json");
    let mut settings: Value =
        serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
    settings["tools"] = json!({"policy": "ask_every_time", "mcp_expose": true});
    std::fs::write(&settings_path, settings.to_string()).unwrap();
    std::fs::write(
        dir.path().join("config/mcp_servers.json"),
        json!({"schema_version": 1, "mcpServers": {
            "harness": {"command": bin(), "args": ["mcp"], "env": {
                "HARNESS_DATA_DIR": data, "HARNESS_MEMORY_SECRETS": "1"}}}})
        .to_string(),
    )
    .unwrap();
    // Through a running owner, as with the GUI: its `GET /tools` used to start the MCP servers.
    let mut serve = Serve::start(dir.path(), free_port(18790));
    let t0 = Instant::now();
    let c = connect(&stdio_def(bin(), &["mcp"], &env), false).await;
    let tools = names(&c);
    assert!(t0.elapsed() < Duration::from_secs(30), "{:?}", t0.elapsed());
    assert!(tools.iter().any(|n| n == "read_file"), "{tools:?}");
    assert!(tools.iter().any(|n| n == "list_models"), "{tools:?}");
    c.close(Duration::from_secs(2)).await;
    common::run(dir.path(), &["shutdown", "--json"]);
    serve.wait_exit(Duration::from_secs(30));
}

/// The hub: servers from a configuration, tools registered with the annotation class and the
/// `<server>_<tool>` name, runs through the tool service (policy, user class override, log
/// source), a changed configuration, and a clean shutdown.
#[tokio::test]
async fn hub_registers_namespaced_tools_with_annotation_classes() {
    let dir = common::data_dir(None, free_port(18760));
    let registry = Arc::new(ToolRegistry::with_builtin());
    let hub = McpHub::with_servers(
        registry.clone(),
        BTreeMap::from([
            ("harness".to_string(), harness_mcp_def(dir.path())),
            ("off".to_string(), {
                let mut d = harness_mcp_def(dir.path());
                d.disabled = true;
                d
            }),
            (
                "broken".to_string(),
                stdio_def("harness-no-such-program-xyz", &[], &[]),
            ),
        ]),
    );
    let service = ToolService::with_mcp(registry.clone(), LogSink::None, hub.clone());

    // Nothing is started before somebody asks.
    assert!(hub.pids().is_empty());
    assert!(hub
        .status()
        .iter()
        .all(|s| matches!(s.state, McpServerState::Idle | McpServerState::Disabled)));

    service.sync_mcp(false).await;
    let status = hub.status();
    let get = |n: &str| status.iter().find(|s| s.name == n).unwrap().clone();
    assert_eq!(get("harness").state, McpServerState::Connected);
    assert_eq!(get("harness").protocol.as_deref(), Some("2026-07-28"));
    assert!(get("harness").tools >= 15);
    assert_eq!(get("off").state, McpServerState::Disabled);
    assert_eq!(get("broken").state, McpServerState::Failed);
    assert!(get("broken")
        .error
        .as_deref()
        .unwrap()
        .contains("could not start"));

    let listing = service.listing(&ToolSettings::default());
    let by = |n: &str| {
        listing
            .tools
            .iter()
            .find(|t| t.def.name == n)
            .unwrap_or_else(|| panic!("no tool {n}"))
            .clone()
    };
    // readOnlyHint -> restricted; destructive / write tools -> dangerous; builtin untouched.
    assert_eq!(
        by("harness_list_models").def.class,
        PermissionClass::Restricted
    );
    assert_eq!(by("harness_list_models").server.as_deref(), Some("harness"));
    assert_eq!(
        by("harness_delete_model").def.class,
        PermissionClass::Dangerous
    );
    assert_eq!(
        by("harness_load_model").def.class,
        PermissionClass::Dangerous
    );
    assert_eq!(by("read_file").server, None);

    // A user override applies to the listing and to the decision.
    let mut settings = ToolSettings {
        policy: ToolPolicy::AskEveryTime,
        ..Default::default()
    };
    settings.mcp_classes = Some(BTreeMap::from([(
        "harness_delete_model".to_string(),
        PermissionClass::Safe,
    )]));
    let l = service.listing(&settings);
    let e = l
        .tools
        .iter()
        .find(|t| t.def.name == "harness_delete_model")
        .unwrap();
    assert_eq!(e.def.class, PermissionClass::Safe);
    assert_eq!(
        service.decision("harness_delete_model", &settings),
        Some(harness_core::tools::Decision::Run)
    );
    assert_eq!(
        service.decision("harness_list_models", &settings),
        Some(harness_core::tools::Decision::Ask)
    );

    // A run goes through the policy: asks, then runs when approved.
    let inv = || {
        ToolInvocation::new(
            "harness_hardware_status",
            json!({}),
            ToolOrigin::new("test"),
        )
    };
    let denied = service
        .run(
            inv(),
            RunOptions {
                settings: Some(settings.clone()),
                ..Default::default()
            },
        )
        .await;
    assert_eq!(denied.error, Some(ToolErrorCode::ApprovalRequired));
    let r = service
        .run(
            inv(),
            RunOptions {
                settings: Some(settings.clone()),
                approved: true,
                ..Default::default()
            },
        )
        .await;
    // No instance runs for this data dir: the server's own error comes back as a failure.
    assert_eq!(r.error, Some(ToolErrorCode::Failed), "{}", r.content);

    // Unknown to the registry but configured: the first run syncs by itself.
    let fresh_reg = Arc::new(ToolRegistry::with_builtin());
    let fresh_hub = McpHub::with_servers(
        fresh_reg.clone(),
        BTreeMap::from([("harness".to_string(), harness_mcp_def(dir.path()))]),
    );
    let fresh = ToolService::with_mcp(fresh_reg, LogSink::None, fresh_hub.clone());
    let r = fresh
        .run(
            ToolInvocation::new("harness_hardware_status", json!({}), ToolOrigin::new("t")),
            RunOptions {
                settings: Some(ToolSettings {
                    policy: ToolPolicy::AllowAll,
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .await;
    assert_eq!(r.error, Some(ToolErrorCode::Failed), "{}", r.content);
    let pids: Vec<u32> = fresh_hub.pids().into_iter().map(|(_, p)| p).collect();
    assert_eq!(pids.len(), 1);

    // Changing the configuration: the removed server's tools and process go.
    fresh_hub.set_servers(BTreeMap::new());
    fresh.sync_mcp(false).await;
    assert!(fresh
        .registry()
        .list()
        .iter()
        .all(|t| !t.name.starts_with("harness_")));
    wait_dead(pids[0]).await;

    // Shutdown ends the rest.
    let pids: Vec<u32> = hub.pids().into_iter().map(|(_, p)| p).collect();
    assert_eq!(pids.len(), 1);
    hub.shutdown(Duration::from_secs(3)).await;
    wait_dead(pids[0]).await;
    assert!(registry
        .list()
        .iter()
        .all(|t| !t.name.starts_with("harness_")));
}

/// A stdio server that died is replaced by the next call.
#[tokio::test]
async fn a_call_after_the_server_died_reconnects() {
    let dir = common::data_dir(None, free_port(18770));
    let registry = Arc::new(ToolRegistry::with_builtin());
    let hub = McpHub::with_servers(
        registry.clone(),
        BTreeMap::from([("harness".to_string(), harness_mcp_def(dir.path()))]),
    );
    let service = ToolService::with_mcp(registry, LogSink::None, hub.clone());
    service.sync_mcp(false).await;
    let first = hub.pids()[0].1;
    // Kill the server behind the client's back.
    let _ = if cfg!(windows) {
        std::process::Command::new("taskkill")
            .args(["/F", "/T", "/PID", &first.to_string()])
            .output()
    } else {
        std::process::Command::new("kill")
            .args(["-9", &first.to_string()])
            .output()
    };
    wait_dead(first).await;
    let r = service
        .run(
            ToolInvocation::new("harness_hardware_status", json!({}), ToolOrigin::new("t")),
            RunOptions {
                settings: Some(ToolSettings {
                    policy: ToolPolicy::AllowAll,
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .await;
    assert_eq!(r.error, Some(ToolErrorCode::Failed), "{}", r.content);
    assert!(
        r.content.contains("no_instance") || r.content.contains("instance"),
        "{}",
        r.content
    );
    let second = hub.pids()[0].1;
    assert_ne!(first, second);
    hub.shutdown(Duration::from_secs(2)).await;
    wait_dead(second).await;
}

// ---------------------------------------------------------------------------------------
// Real third-party server (npx) and the protocol fixture: ignored, need Node
// ---------------------------------------------------------------------------------------

fn npx_everything(args: &[&str]) -> McpServerDef {
    let mut a = vec!["-y", "@modelcontextprotocol/server-everything"];
    a.extend_from_slice(args);
    stdio_def("npx", &a, &[])
}

/// Descendants of Node still running a `server-everything`: none must survive a close.
fn everything_processes() -> usize {
    if cfg!(windows) {
        let out = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "(Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -like '*server-everything*' -and $_.Name -like 'node*' }).Count",
            ])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout)
            .trim()
            .parse()
            .unwrap_or(0)
    } else {
        let out = std::process::Command::new("pgrep")
            .args(["-f", "server-everything"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).lines().count()
    }
}

/// `@modelcontextprotocol/server-everything` over stdio: legacy handshake after a failed
/// probe, many tools, a successful call, a tool error, a timeout and a cancellation that reach
/// a real server, and no node process left after closing.
#[tokio::test]
#[ignore = "needs Node and the network (npx)"]
async fn e2e_everything_stdio() {
    let before = everything_processes();
    let c = connect(&npx_everything(&[]), false).await;
    eprintln!(
        "everything/stdio: era {:?}, server {:?}, {} tools",
        c.era(),
        c.server_info(),
        c.tools().len()
    );
    let tools = names(&c);
    assert!(tools.contains(&"echo".to_string()), "{tools:?}");
    assert!(tools.len() > 5, "{tools:?}");
    for t in c.tools() {
        let _ = &t.input_schema;
    }

    let ok = call(&c, "echo", json!({"message": "merhaba"}))
        .await
        .unwrap();
    assert!(ok.contains("merhaba"), "{ok}");

    // The tool ran and failed (a tool that always errors, or bad arguments).
    let err = call(&c, "echo", json!({"message": 5})).await.unwrap_err();
    eprintln!("bad arguments -> {err}");
    assert!(
        err.starts_with("Failed") || err.starts_with("InvalidArguments"),
        "{err}"
    );

    // Timeout: a long-running operation cut at 1 s; the server is told.
    let slow = tools
        .iter()
        .find(|t| t.to_lowercase().contains("longrunning") || t.contains("long-running"))
        .cloned()
        .expect("a long-running tool");
    let t = Instant::now();
    let r = c
        .call_tool(
            &slow,
            json!({"duration": 20, "steps": 4}),
            Duration::from_secs(1),
            &CancelToken::new(),
        )
        .await
        .unwrap_err();
    assert_eq!(r.code, ToolErrorCode::Timeout);
    assert!(t.elapsed() < Duration::from_secs(5));

    // Cancellation: a token cancelled mid-call.
    let cancel = CancelToken::new();
    let c2 = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(700)).await;
        c2.cancel();
    });
    let t = Instant::now();
    let r = c
        .call_tool(
            &slow,
            json!({"duration": 20, "steps": 4}),
            Duration::from_secs(30),
            &cancel,
        )
        .await
        .unwrap_err();
    assert_eq!(r.code, ToolErrorCode::Cancelled);
    assert!(t.elapsed() < Duration::from_secs(5));
    // The connection is still good afterwards.
    assert!(call(&c, "echo", json!({"message": "again"}))
        .await
        .unwrap()
        .contains("again"));

    let pid = c.pid().unwrap();
    c.close(Duration::from_secs(3)).await;
    drop(c);
    wait_dead(pid).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(everything_processes(), before, "orphaned node processes");
}

/// `server-everything streamableHttp`: a real Streamable HTTP server (legacy era, session id,
/// SSE replies).
#[tokio::test]
#[ignore = "needs Node and the network (npx)"]
async fn e2e_everything_streamable_http() {
    let (mut server, port) = common::start_http_process(|port| {
        let mut command = std::process::Command::new(if cfg!(windows) { "npx.cmd" } else { "npx" });
        command
            .args([
                "-y",
                "@modelcontextprotocol/server-everything",
                "streamableHttp",
            ])
            .env("PORT", port.to_string());
        command
    });
    let def = McpServerDef {
        url: Some(format!("http://127.0.0.1:{port}/mcp")),
        ..Default::default()
    };
    let c = connect(&def, false).await;
    eprintln!(
        "everything/http: era {:?}, server {:?}",
        c.era(),
        c.server_info()
    );
    assert!(matches!(c.era(), Era::Legacy(_)));
    assert!(names(&c).contains(&"echo".to_string()));
    let ok = call(&c, "echo", json!({"message": "http-merhaba"}))
        .await
        .unwrap();
    assert!(ok.contains("http-merhaba"), "{ok}");
    // A call whose reply is an SSE stream (progress notifications before the result).
    let slow = names(&c)
        .into_iter()
        .find(|t| t.to_lowercase().contains("longrunning") || t.contains("long-running"))
        .unwrap();
    let r = c
        .call_tool(
            &slow,
            json!({"duration": 2, "steps": 2}),
            Duration::from_secs(30),
            &CancelToken::new(),
        )
        .await
        .unwrap();
    eprintln!("long-running -> {}", r.content);
    c.close(Duration::from_secs(1)).await;
    // `npx` starts the server as a grandchild: end the whole tree.
    if cfg!(windows) {
        let _ = std::process::Command::new("taskkill")
            .args(["/F", "/T", "/PID", &server.id().to_string()])
            .output();
    } else {
        let _ = server.kill();
    }
    let _ = server.wait();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(everything_processes(), 0, "orphaned node processes");
}

/// Protocol fixture (Node): a legacy stdio server with a two-per-page `tools/list`, a tool that
/// changes the list and sends `notifications/tools/list_changed`, a tool that never answers,
/// and a tool that reports the `notifications/cancelled` it received.
const FIXTURE: &str = r#"
const readline = require('readline');
const send = (o) => process.stdout.write(JSON.stringify(o) + '\n');
let tools = ['t1', 't2', 't3', 't4', 't5'].map((n) => ({ name: n, description: n, inputSchema: { type: 'object', properties: {} } }));
tools.push({ name: 'add_tool', description: 'adds t6 and sends list_changed', inputSchema: { type: 'object', properties: {} } });
tools.push({ name: 'hang', description: 'never answers', inputSchema: { type: 'object', properties: {} } });
tools.push({ name: 'cancelled_ids', description: 'cancellations seen', inputSchema: { type: 'object', properties: {} } });
const cancelled = [];
readline.createInterface({ input: process.stdin }).on('line', (line) => {
  const m = JSON.parse(line);
  if (m.method === 'initialize') {
    send({ jsonrpc: '2.0', id: m.id, result: { protocolVersion: m.params.protocolVersion, capabilities: { tools: { listChanged: true } }, serverInfo: { name: 'fixture', version: '1' } } });
  } else if (m.method === 'notifications/cancelled') {
    cancelled.push(m.params.requestId + ':' + m.params.reason);
  } else if (m.method === 'tools/list') {
    const start = Number((m.params && m.params.cursor) || 0);
    const page = tools.slice(start, start + 2);
    const r = { tools: page };
    if (start + 2 < tools.length) r.nextCursor = String(start + 2);
    send({ jsonrpc: '2.0', id: m.id, result: r });
  } else if (m.method === 'tools/call') {
    const n = m.params.name;
    if (n === 'add_tool') {
      tools.push({ name: 't6', description: 't6', inputSchema: { type: 'object', properties: {} } });
      send({ jsonrpc: '2.0', id: m.id, result: { content: [{ type: 'text', text: 'added' }] } });
      send({ jsonrpc: '2.0', method: 'notifications/tools/list_changed' });
    } else if (n === 'cancelled_ids') {
      send({ jsonrpc: '2.0', id: m.id, result: { content: [{ type: 'text', text: cancelled.join(',') }] } });
    } else if (n === 'hang') {
      // no answer
    } else {
      send({ jsonrpc: '2.0', id: m.id, result: { isError: true, content: [{ type: 'text', text: 'boom from ' + n }] } });
    }
  } else if (m.id !== undefined) {
    send({ jsonrpc: '2.0', id: m.id, error: { code: -32601, message: 'Method not found' } });
  }
});
"#;

#[tokio::test]
#[ignore = "needs Node"]
async fn fixture_pagination_list_changed_cancellation_and_probe_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("fixture.js");
    std::fs::write(&script, FIXTURE).unwrap();
    let def = stdio_def("node", &[script.to_str().unwrap()], &[]);

    let changed = Arc::new(AtomicUsize::new(0));
    let c2 = changed.clone();
    let started = Instant::now();
    let c = McpConnection::connect(
        "fx",
        McpTarget::from_def(&def).unwrap(),
        ConnectOptions {
            on_tools_changed: Arc::new(move || {
                c2.fetch_add(1, Ordering::SeqCst);
            }),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    // The probe got "Method not found" -> legacy; the offered version was accepted.
    assert_eq!(c.era(), Era::Legacy("2025-11-25".into()));
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "{:?}",
        started.elapsed()
    );
    // Pagination: eight tools over four pages.
    assert_eq!(names(&c).len(), 8, "{:?}", names(&c));

    // A tool error is the server's text.
    let err = call(&c, "t1", json!({})).await.unwrap_err();
    assert!(err.contains("boom from t1"), "{err}");

    // list_changed: the list is fetched again and the callback runs.
    assert_eq!(call(&c, "add_tool", json!({})).await.unwrap(), "added");
    let t = Instant::now();
    while changed.load(Ordering::SeqCst) == 0 {
        assert!(t.elapsed() < Duration::from_secs(10), "no refresh");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(names(&c).contains(&"t6".to_string()), "{:?}", names(&c));
    assert_eq!(names(&c).len(), 9);

    // Timeout: the server is told with reason "timeout".
    let r = c
        .call_tool(
            "hang",
            json!({}),
            Duration::from_millis(400),
            &CancelToken::new(),
        )
        .await
        .unwrap_err();
    assert_eq!(r.code, ToolErrorCode::Timeout);
    // Cancellation: reason "cancelled".
    let cancel = CancelToken::new();
    let k = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        k.cancel();
    });
    let r = c
        .call_tool("hang", json!({}), Duration::from_secs(30), &cancel)
        .await
        .unwrap_err();
    assert_eq!(r.code, ToolErrorCode::Cancelled);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let seen = call(&c, "cancelled_ids", json!({})).await.unwrap();
    assert!(
        seen.contains(":timeout") && seen.contains(":cancelled"),
        "{seen}"
    );

    let pid = c.pid().unwrap();
    drop(c);
    wait_dead(pid).await;
    let _ = cmd; // (the helper is shared with other test files)
}
