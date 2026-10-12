//! MCP over streamable HTTP (`POST /api/harness/v1/mcp`) against the real Control API on
//! 127.0.0.1 (ephemeral port, temp data dirs).
//!
//! The `#[ignore]` end-to-end test needs a real llama-server and model (ports 18900+):
//! HS_LLAMA_SERVER=<llama-server> HS_TEST_MODEL=<gguf> cargo test -p harness-api --test mcp
//!   -- --ignored --nocapture

mod common;

use common::{json as read_json, start, Api};
use serde_json::{json, Value};

const MODERN: &str = "2026-07-28";

fn meta() -> Value {
    json!({"io.modelcontextprotocol/protocolVersion": MODERN,
        "io.modelcontextprotocol/clientCapabilities": {},
        "io.modelcontextprotocol/clientInfo": {"name": "harness-test", "version": "1"}})
}

/// A modern request with the mirrored headers.
async fn modern(api: &Api, token: &str, method: &str, mut params: Value) -> (u16, Value) {
    params["_meta"] = meta();
    let body = json!({"jsonrpc": "2.0", "id": 7, "method": method, "params": params});
    let mut req = api
        .http
        .post(api.url("/mcp"))
        .bearer_auth(token)
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", MODERN)
        .header("mcp-method", method);
    if let Some(name) = body["params"]["name"].as_str() {
        req = req.header("mcp-name", name);
    }
    read_json(req.json(&body).send().await.unwrap()).await
}

/// A legacy (2025-06-18) request: `MCP-Protocol-Version` only.
async fn legacy(api: &Api, token: &str, method: &str, params: Value) -> (u16, Value) {
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
    read_json(
        api.http
            .post(api.url("/mcp"))
            .bearer_auth(token)
            .header("accept", "application/json, text/event-stream")
            .header("mcp-protocol-version", "2025-06-18")
            .json(&body)
            .send()
            .await
            .unwrap(),
    )
    .await
}

fn names(list: &Value) -> Vec<String> {
    list["result"]["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("{list}"))
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn http_transport_auth_eras_and_scopes() {
    let api = start().await;
    let admin = api.token.clone();

    // No token: 401 like every endpoint.
    let resp = api
        .http
        .post(api.url("/mcp"))
        .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "ping"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    // Legacy handshake; the server keeps no session.
    let (status, init) = legacy(
        &api,
        &admin,
        "initialize",
        json!({"protocolVersion": "2025-06-18", "capabilities": {},
               "clientInfo": {"name": "t", "version": "1"}}),
    )
    .await;
    assert_eq!(status, 200, "{init}");
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(init["result"]["serverInfo"]["name"], "harness");
    // Notification: 202, no body.
    let resp = api
        .http
        .post(api.url("/mcp"))
        .bearer_auth(&admin)
        .header("mcp-protocol-version", "2025-06-18")
        .json(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 202);
    assert!(resp.headers().get("mcp-session-id").is_none());
    assert!(resp.text().await.unwrap().is_empty());

    // The session token is full access: every tool is listed.
    let (_, list) = legacy(&api, &admin, "tools/list", json!({})).await;
    let all = names(&list);
    assert_eq!(all.len(), 20, "{all:?}");
    assert!(all.contains(&"pull_model".to_string()));
    assert!(list["result"].get("resultType").is_none());

    // A read-only named token sees only read tools and cannot call the others.
    let (_, created) = read_json(
        api.post("/tokens", json!({"name": "agent", "scopes": ["read"]}))
            .await,
    )
    .await;
    let reader = created["token"].as_str().unwrap().to_string();
    let (status, list) = modern(&api, &reader, "tools/list", json!({})).await;
    assert_eq!(status, 200, "{list}");
    assert_eq!(list["result"]["resultType"], "complete");
    assert_eq!(list["result"]["cacheScope"], "private");
    let read_only = names(&list);
    assert!(read_only.contains(&"list_models".to_string()));
    assert!(read_only.contains(&"get_endpoint".to_string()));
    assert!(!read_only.contains(&"load_model".to_string()));
    assert!(!read_only.contains(&"delete_model".to_string()));
    for t in list["result"]["tools"].as_array().unwrap() {
        assert_eq!(t["annotations"]["readOnlyHint"], true, "{t}");
    }
    let (_, refused) = modern(
        &api,
        &reader,
        "tools/call",
        json!({"name": "load_model", "arguments": {"model": "x"}}),
    )
    .await;
    assert_eq!(refused["result"]["isError"], true, "{refused}");
    assert_eq!(
        refused["result"]["structuredContent"]["error"]["params"]["scope"],
        "servers:control"
    );

    // Read tools run through the API with the caller's token.
    let (status, hw) = modern(
        &api,
        &reader,
        "tools/call",
        json!({"name": "hardware_status", "arguments": {}}),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(hw["result"]["isError"], false, "{hw}");
    assert!(
        hw["result"]["structuredContent"]["hardware"]["total_ram_bytes"]
            .as_u64()
            .unwrap()
            > 0
    );
    let (_, servers) = modern(
        &api,
        &reader,
        "tools/call",
        json!({"name": "list_servers", "arguments": {"all": true}}),
    )
    .await;
    assert_eq!(servers["result"]["isError"], false, "{servers}");

    // server/discover (modern only).
    let (status, discover) = modern(&api, &reader, "server/discover", json!({})).await;
    assert_eq!(status, 200);
    assert_eq!(discover["result"]["supportedVersions"][0], MODERN);
    assert_eq!(
        discover["result"]["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
        "harness"
    );

    // Header rules of the modern transport.
    let body = json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": {"name": "list_models", "arguments": {}, "_meta": meta()}});
    let (status, mismatch) = read_json(
        api.http
            .post(api.url("/mcp"))
            .bearer_auth(&admin)
            .header("mcp-protocol-version", MODERN)
            .header("mcp-method", "tools/call")
            .header("mcp-name", "unload_model")
            .json(&body)
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, 400);
    assert_eq!(mismatch["error"]["code"], -32020);
    let (status, unsupported) = read_json(
        api.http
            .post(api.url("/mcp"))
            .bearer_auth(&admin)
            .header("mcp-protocol-version", "2099-01-01")
            .json(&body)
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, 400);
    assert_eq!(unsupported["error"]["code"], -32022);
    // Missing clientCapabilities in _meta: malformed (400, -32602).
    let bad_meta = json!({"jsonrpc": "2.0", "id": 4, "method": "tools/list",
        "params": {"_meta": {"io.modelcontextprotocol/protocolVersion": MODERN}}});
    let (status, bad) = read_json(
        api.http
            .post(api.url("/mcp"))
            .bearer_auth(&admin)
            .header("mcp-protocol-version", MODERN)
            .header("mcp-method", "tools/list")
            .json(&bad_meta)
            .send()
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, 400);
    assert_eq!(bad["error"]["code"], -32602);
    // Unknown method: 404 with a JSON-RPC body (modern).
    let (status, unknown) = modern(&api, &admin, "resources/list", json!({})).await;
    assert_eq!(status, 404);
    assert_eq!(unknown["error"]["code"], -32601);

    // No GET stream, no sessions.
    let resp = api
        .http
        .get(api.url("/mcp"))
        .bearer_auth(&admin)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 405);
    // A browser page's Origin is refused.
    let resp = api
        .http
        .post(api.url("/mcp"))
        .bearer_auth(&admin)
        .header("origin", "http://evil.example")
        .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "ping"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403);
    // Not JSON.
    let resp = api
        .http
        .post(api.url("/mcp"))
        .bearer_auth(&admin)
        .body("{nope")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);

    api.server.stop().await;
}

#[tokio::test]
#[ignore]
async fn e2e_mcp_http_loads_a_real_model() {
    let (Some(_bin), Some(model)) = (common::llama_server(), common::test_model()) else {
        eprintln!("HS_LLAMA_SERVER / HS_TEST_MODEL not set, skipping");
        return;
    };
    let api = start().await;
    let token = api.token.clone();
    let model = model.to_str().unwrap().to_string();
    // Default port of new servers: 18900+ (the GPU is shared with other test suites).
    let port = api
        .ctx
        .registry
        .suggest_port("127.0.0.1", 18900, &[])
        .expect("free port");
    let mut settings = harness_core::config::AppSettings::load();
    settings.server.port = port;
    settings.server.default_ctx = 4096;
    settings.save().unwrap();

    let call = |name: &'static str, args: Value| {
        let api = &api;
        let token = token.clone();
        async move {
            let (status, r) = modern(
                api,
                &token,
                "tools/call",
                json!({"name": name, "arguments": args}),
            )
            .await;
            assert_eq!(status, 200, "{name}: {r}");
            assert_eq!(r["result"]["isError"], false, "{name}: {r}");
            r["result"]["structuredContent"].clone()
        }
    };

    let loaded = call("load_model", json!({"model": model})).await;
    eprintln!("load_model: {loaded}");
    assert_eq!(loaded["ready"], true);
    let server_id = loaded["server_id"].as_str().unwrap().to_string();
    let ep = call("get_endpoint", json!({"server_id": server_id})).await;
    assert_eq!(ep["kind"], "direct");
    let base = ep["base_url"].as_str().unwrap().to_string();
    let reply: Value = reqwest::Client::new()
        .post(format!("{}/chat/completions", base))
        .json(
            &json!({"model": ep["model"], "max_tokens": 16, "temperature": 0,
            "messages": [{"role": "user", "content": "Say hello in three words."}]}),
        )
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    eprintln!("chat: {}", reply["choices"][0]["message"]["content"]);
    assert!(reply["usage"]["completion_tokens"].as_u64().unwrap() > 0);

    let unloaded = call("unload_model", json!({"server_id": server_id})).await;
    assert_eq!(unloaded["stopped"], json!([server_id]));
    let running = call("list_servers", json!({})).await;
    assert_eq!(running["count"], 0, "{running}");
    api.server.stop().await;
}
