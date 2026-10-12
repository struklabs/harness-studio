//! The `tools` scope and the MCP server's Harness tools over real HTTP: a token with `tools`
//! runs only what the saved policy lets through; the MCP surface offers the built-in tools only
//! when the user opted in, with annotations from the permission class, and logs the calls under
//! the MCP client's name. The policy is a process-wide setting of the test data root, so the
//! tests run one at a time.

mod common;

use common::{json, start, Api};
use reqwest::Method;
use serde_json::{json, Value};

static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn put_policy(api: &Api, expose: bool, policy: &str, trusted: Value, workdir: &str) {
    let body = json!({"policy": policy, "trusted": trusted, "workdir": workdir,
        "mcp_expose": expose});
    let (status, b) = json(
        api.send(Method::PUT, "/tools/policy", &api.token, Some(body))
            .await,
    )
    .await;
    assert_eq!(status, 200, "{b}");
}

fn mcp_meta() -> Value {
    json!({"io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {},
        "io.modelcontextprotocol/clientInfo": {"name": "tools-test-client", "version": "1"}})
}

/// A modern MCP request with the mirrored headers.
async fn mcp(api: &Api, token: &str, method: &str, mut params: Value) -> Value {
    params["_meta"] = mcp_meta();
    let body = json!({"jsonrpc": "2.0", "id": 3, "method": method, "params": params});
    let mut req = api
        .http
        .post(api.url("/mcp"))
        .bearer_auth(token)
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", "2026-07-28")
        .header("mcp-method", method);
    if let Some(name) = body["params"]["name"].as_str() {
        req = req.header("mcp-name", name);
    }
    json(req.json(&body).send().await.unwrap()).await.1
}

async fn call(api: &Api, token: &str, name: &str, arguments: Value) -> Value {
    mcp(
        api,
        token,
        "tools/call",
        json!({"name": name, "arguments": arguments}),
    )
    .await
}

async fn token(api: &Api, name: &str, scopes: Value) -> String {
    let (status, body) = json(
        api.post("/tokens", json!({"name": name, "scopes": scopes}))
            .await,
    )
    .await;
    assert_eq!(status, 201, "{body}");
    body["token"].as_str().unwrap().to_string()
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
async fn the_tools_scope_runs_only_what_the_policy_lets_through() {
    let _g = ONE_AT_A_TIME.lock().await;
    let api = start().await;
    let work = tempfile::tempdir().unwrap();
    std::fs::write(work.path().join("a.txt"), "hello\n").unwrap();
    let wd = work.path().to_string_lossy().to_string();
    put_policy(&api, false, "allow_trusted", json!(["read_file"]), &wd).await;
    let runner = token(&api, "runner", json!(["read", "tools"])).await;
    let run = |tool: &'static str, body: Value| {
        let (api, runner) = (&api, runner.clone());
        async move {
            json(
                api.send(
                    Method::POST,
                    &format!("/tools/{tool}/run"),
                    &runner,
                    Some(body),
                )
                .await,
            )
            .await
        }
    };

    // Trusted read inside the saved folder: runs.
    let (status, body) = run("read_file", json!({"args": {"path": "a.txt"}})).await;
    assert_eq!(status, 200, "{body}");
    assert!(body["content"].as_str().unwrap().contains("hello"));
    // Not trusted: refused, and nothing was written.
    let write = json!({"args": {"path": "w.txt", "content": "x"}});
    let (status, body) = run("write_file", write.clone()).await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(body["error"]["code"], "tools.approval_required");
    // The token cannot approve for itself or pick another folder: both are admin matters.
    for extra in [
        json!({"approved": true}),
        json!({"workdir": std::env::temp_dir()}),
    ] {
        let mut b = write.clone();
        b.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let (status, body) = run("write_file", b).await;
        assert_eq!(status, 403, "{body}");
        assert_eq!(body["error"]["code"], "api.insufficient_scope");
        assert_eq!(body["error"]["params"]["scope"], "admin");
    }
    assert!(!work.path().join("w.txt").exists());
    // Without the `tools` scope nothing runs.
    let read_only = token(&api, "just-read", json!(["read"])).await;
    let (status, body) = json(
        api.send(
            Method::POST,
            "/tools/read_file/run",
            &read_only,
            Some(json!({"args": {"path": "a.txt"}})),
        )
        .await,
    )
    .await;
    assert_eq!(status, 403);
    assert_eq!(body["error"]["params"]["scope"], "tools");
    put_policy(&api, false, "ask_every_time", json!([]), "").await;
    api.server.stop().await;
}

#[tokio::test]
async fn the_mcp_surface_offers_harness_tools_only_when_opted_in() {
    let _g = ONE_AT_A_TIME.lock().await;
    let api = start().await;
    let work = tempfile::tempdir().unwrap();
    std::fs::write(work.path().join("a.txt"), "hello\n").unwrap();
    let wd = work.path().to_string_lossy().to_string();
    let runner = token(&api, "mcp-runner", json!(["read", "tools"])).await;
    let reader = token(&api, "mcp-reader", json!(["read"])).await;

    // Default (off): the management tools only, even for a token with the `tools` scope; a
    // Harness tool is unknown.
    put_policy(&api, false, "allow_trusted", json!(["read_file"]), &wd).await;
    let list = mcp(&api, &runner, "tools/list", json!({})).await;
    assert!(names(&list).contains(&"list_models".to_string()), "{list}");
    assert!(!names(&list).contains(&"read_file".to_string()), "{list}");
    let r = call(&api, &runner, "read_file", json!({"path": "a.txt"})).await;
    assert!(r["error"].is_object(), "{r}");

    // On: listed with annotations from the class; a token without the scope still sees none.
    put_policy(&api, true, "allow_trusted", json!(["read_file"]), &wd).await;
    let list = mcp(&api, &runner, "tools/list", json!({})).await;
    let listed = names(&list);
    for n in ["read_file", "write_file", "calculator", "list_models"] {
        assert!(listed.contains(&n.to_string()), "{n} in {listed:?}");
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
    assert_eq!(tool("write_file")["annotations"]["readOnlyHint"], false);
    assert_eq!(tool("write_file")["annotations"]["destructiveHint"], true);
    assert_eq!(tool("fetch_url")["annotations"]["openWorldHint"], true);
    assert_eq!(tool("calculator")["annotations"]["readOnlyHint"], true);
    let desc = |n: &str| tool(n)["description"].as_str().unwrap().to_string();
    assert!(desc("write_file").contains("Settings > Tools"));
    assert!(!desc("calculator").contains("approval"));
    let list_reader = mcp(&api, &reader, "tools/list", json!({})).await;
    assert!(!names(&list_reader).contains(&"read_file".to_string()));

    // A trusted tool runs, logged under the client's name from `_meta` ...
    let r = call(&api, &runner, "read_file", json!({"path": "a.txt"})).await;
    assert_eq!(r["result"]["isError"], false, "{r}");
    assert!(r["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("hello"));
    let r = call(&api, &runner, "calculator", json!({"expression": "6*7"})).await;
    assert_eq!(r["result"]["isError"], false, "{r}");
    assert!(r["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("42"));
    // ... one that needs approval is a tool error that says how to trust it ...
    let write = json!({"path": "w.txt", "content": "x"});
    let r = call(&api, &runner, "write_file", write.clone()).await;
    assert_eq!(r["result"]["isError"], true, "{r}");
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("Settings > Tools") && text.contains("approval"),
        "{text}"
    );
    assert!(!work.path().join("w.txt").exists());
    // ... a bad argument is a tool error, not a protocol error ...
    let r = call(&api, &runner, "read_file", json!({})).await;
    assert_eq!(r["result"]["isError"], true, "{r}");
    // ... a connection without the `tools` scope cannot call them ...
    let r = call(&api, &reader, "read_file", json!({"path": "a.txt"})).await;
    assert!(r["error"].is_object(), "{r}");
    // ... with the policy off nothing is offered ...
    put_policy(&api, true, "off", json!([]), &wd).await;
    let list = mcp(&api, &runner, "tools/list", json!({})).await;
    assert!(!names(&list).contains(&"calculator".to_string()));
    // ... and "allow all" lets the write through.
    put_policy(&api, true, "allow_all", json!([]), &wd).await;
    let r = call(&api, &runner, "write_file", write).await;
    assert_eq!(r["result"]["isError"], false, "{r}");
    assert!(work.path().join("w.txt").exists());

    let (_, log) = json(api.get("/tools/log?limit=30").await).await;
    let rows = log.as_array().unwrap();
    let logged = |tool: &str, err: Option<&str>| {
        rows.iter().any(|r| {
            r["tool"] == tool
                && r["client"] == "mcp:tools-test-client"
                && match err {
                    None => r["error_code"].is_null(),
                    Some(e) => r["error_code"] == e,
                }
        })
    };
    assert!(logged("read_file", None), "{rows:?}");
    assert!(logged("write_file", Some("approval_required")));
    assert!(logged("write_file", None));
    put_policy(&api, false, "ask_every_time", json!([]), "").await;
    api.server.stop().await;
}
