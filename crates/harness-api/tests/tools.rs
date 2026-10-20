//! The tool engine over real HTTP: listing, the policy gate (403 for what needs approval), the
//! working-directory scope, scopes of named tokens and the tool log. The policy is a process-wide
//! setting of the test data root, so these tests run one at a time.

mod common;

use common::{json, start, Api};
use reqwest::Method;
use serde_json::{json, Value};

static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn set_policy(api: &Api, policy: &str, trusted: &[&str], workdir: &str) -> Value {
    let resp = api
        .send(
            Method::PUT,
            "/tools/policy",
            &api.token,
            Some(json!({"policy": policy, "trusted": trusted, "workdir": workdir})),
        )
        .await;
    let (status, body) = json(resp).await;
    assert_eq!(status, 200, "{body}");
    body
}

async fn run(api: &Api, tool: &str, body: Value) -> (u16, Value) {
    json(api.post(&format!("/tools/{tool}/run"), body).await).await
}

#[tokio::test]
async fn web_search_provider_key_is_write_only_and_not_in_settings() {
    let _g = ONE_AT_A_TIME.lock().await;
    let api = start().await;
    let response = api
        .send(
            Method::PUT,
            "/tools/search-provider",
            &api.token,
            Some(json!({"provider":"brave","searxng_url":"","api_key":"secret-test-key"})),
        )
        .await;
    let (status, body) = json(response).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["provider"], "brave");
    assert_eq!(body["api_key_set"], true);
    assert!(body.get("api_key").is_none());
    let (_, read) = json(api.get("/tools/search-provider").await).await;
    assert!(read.get("api_key").is_none());
    let settings =
        std::fs::read_to_string(harness_core::config::AppSettings::config_path()).unwrap();
    assert!(!settings.contains("secret-test-key"));
    let _ = api
        .send(
            Method::PUT,
            "/tools/search-provider",
            &api.token,
            Some(json!({"provider":"none","searxng_url":""})),
        )
        .await;
    let (_, read) = json(api.get("/tools/search-provider").await).await;
    assert_eq!(read["provider"], "none");
    let response = api
        .send(
            Method::PUT,
            "/tools/search-provider",
            &api.token,
            Some(json!({"provider":"duckduckgo"})),
        )
        .await;
    let (status, body) = json(response).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["provider"], "duckduckgo");
    assert_eq!(body["api_key_set"], false);
    let (_, read) = json(api.get("/tools/search-provider").await).await;
    assert_eq!(read["provider"], "duckduckgo");
}

#[tokio::test]
async fn listing_policy_and_the_403_for_what_needs_approval() {
    let _g = ONE_AT_A_TIME.lock().await;
    let api = start().await;
    let work = tempfile::tempdir().unwrap();
    std::fs::write(work.path().join("a.txt"), "hello\n").unwrap();
    let wd = work.path().to_string_lossy().to_string();

    // The default policy asks for everything that is not safe.
    set_policy(&api, "ask_every_time", &[], "").await;
    let (status, list) = json(api.get("/tools").await).await;
    assert_eq!(status, 200, "{list}");
    assert_eq!(list["policy"], "ask_every_time");
    let tools = list["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        [
            "read_file",
            "file_glob_search",
            "grep_search",
            "exec_shell_command",
            "write_file",
            "edit_file",
            "get_info",
            "load_skill",
            "current_datetime",
            "calculator",
            "fetch_url",
            "web_search",
            "geocode",
            "weather"
        ]
    );
    let tool = |n: &str| tools.iter().find(|t| t["name"] == n).unwrap();
    assert_eq!(tool("read_file")["class"], "restricted");
    assert_eq!(tool("write_file")["class"], "dangerous");
    assert_eq!(tool("exec_shell_command")["source"], "builtin");
    assert_eq!(
        tool("edit_file")["input_schema"]["required"],
        json!(["path", "edits"])
    );
    assert_eq!(tool("calculator")["decision"], "run");
    assert_eq!(tool("load_skill")["class"], "safe");
    assert_eq!(tool("load_skill")["decision"], "run");
    assert_eq!(tool("fetch_url")["decision"], "ask");

    // Dangerous without the policy: 403, and nothing happened.
    let (status, body) = run(
        &api,
        "write_file",
        json!({"args": {"path": "w.txt", "content": "x"}, "workdir": wd}),
    )
    .await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(body["error"]["code"], "tools.approval_required");
    assert_eq!(body["error"]["params"]["tool"], "write_file");
    assert_eq!(body["error"]["params"]["class"], "dangerous");
    assert!(!work.path().join("w.txt").exists());
    let (status, body) = run(
        &api,
        "exec_shell_command",
        json!({"args": {"command": "echo hi > shell.txt"}, "workdir": wd}),
    )
    .await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (403, Some("tools.approval_required"))
    );
    assert!(!work.path().join("shell.txt").exists());
    // A restricted tool asks too under this policy.
    let (status, _) = run(
        &api,
        "read_file",
        json!({"args": {"path": "a.txt"}, "workdir": wd}),
    )
    .await;
    assert_eq!(status, 403);

    // Allow trusted: only the trusted tool runs.
    set_policy(&api, "allow_trusted", &["write_file"], "").await;
    let (status, body) = run(
        &api,
        "write_file",
        json!({"args": {"path": "w.txt", "content": "x"}, "workdir": wd}),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["tool"], "write_file");
    assert!(body["error"].is_null());
    assert_eq!(
        std::fs::read_to_string(work.path().join("w.txt")).unwrap(),
        "x"
    );
    let (status, _) = run(
        &api,
        "exec_shell_command",
        json!({"args": {"command": "echo hi"}, "workdir": wd}),
    )
    .await;
    assert_eq!(status, 403);

    // The approval flag (an interactive client asked the human) opens an "ask", nothing more.
    set_policy(&api, "ask_every_time", &[], "").await;
    let (status, body) = run(
        &api,
        "read_file",
        json!({"args": {"path": "a.txt"}, "workdir": wd, "approved": true}),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["content"], "hello\n");
    assert_eq!(body["truncated"], false);
    assert!(body["duration_ms"].is_number() && body["id"].is_string());

    // Off: nothing runs, approval or not.
    set_policy(&api, "off", &[], "").await;
    let (status, body) = run(
        &api,
        "read_file",
        json!({"args": {"path": "a.txt"}, "workdir": wd, "approved": true}),
    )
    .await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (403, Some("tools.disabled"))
    );

    // Allow all: tools run; failures of the tool itself are a 200 with an error code.
    set_policy(&api, "allow_all", &[], "").await;
    let (status, body) = run(
        &api,
        "exec_shell_command",
        json!({"args": {"command": "echo from-shell"}, "workdir": wd}),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert!(
        body["content"].as_str().unwrap().contains("from-shell"),
        "{body}"
    );
    assert!(body["content"]
        .as_str()
        .unwrap()
        .ends_with("[exit code: 0]"));
    let (status, body) = run(
        &api,
        "read_file",
        json!({"args": {"path": "../outside.txt"}, "workdir": wd}),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["error"], "outside_workspace");
    let (status, body) = run(&api, "read_file", json!({"args": {}, "workdir": wd})).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (200, Some("invalid_arguments"))
    );
    let (status, body) = run(&api, "nope", json!({"args": {}})).await;
    assert_eq!(status, 404, "{body}");
    assert_eq!(body["error"]["code"], "api.not_found");
    // No working directory anywhere: the tool says so instead of guessing.
    let (status, body) = run(&api, "get_info", json!({"args": {}})).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (200, Some("no_workspace"))
    );
    // The default working directory from the policy settings.
    set_policy(&api, "allow_all", &[], &wd).await;
    let (status, body) = run(&api, "get_info", json!({"args": {}})).await;
    assert_eq!(status, 200, "{body}");
    assert!(body["error"].is_null());
    assert!(body["content"].as_str().unwrap().contains("\"cwd\""));

    // A policy with a working directory that does not exist is refused.
    let resp = api
        .send(
            Method::PUT,
            "/tools/policy",
            &api.token,
            Some(json!({"policy": "allow_all", "workdir": work.path().join("nope")})),
        )
        .await;
    let (status, body) = json(resp).await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["error"]["code"], "tools.workdir_invalid");

    set_policy(&api, "ask_every_time", &[], "").await;
    api.server.stop().await;
}

#[tokio::test]
async fn named_tokens_the_log_and_the_clients_label() {
    let _g = ONE_AT_A_TIME.lock().await;
    let api = start().await;
    let work = tempfile::tempdir().unwrap();
    std::fs::write(work.path().join("a.txt"), "hello\n").unwrap();
    let wd = work.path().to_string_lossy().to_string();
    set_policy(&api, "allow_all", &[], "").await;

    // A read token lists tools and reads the policy; it cannot run, change or read the log.
    let (status, body) = json(
        api.post("/tokens", json!({"name": "reader", "scopes": ["read"]}))
            .await,
    )
    .await;
    assert_eq!(status, 201, "{body}");
    let reader = body["token"].as_str().unwrap().to_string();
    for path in ["/tools", "/tools/policy", "/tools/search-provider"] {
        assert_eq!(
            api.send(Method::GET, path, &reader, None).await.status(),
            200,
            "{path}"
        );
    }
    let (status, body) = json(
        api.send(
            Method::POST,
            "/tools/read_file/run",
            &reader,
            Some(json!({"args": {"path": "a.txt"}, "workdir": wd})),
        )
        .await,
    )
    .await;
    assert_eq!(status, 403);
    assert_eq!(body["error"]["code"], "api.insufficient_scope");
    assert_eq!(body["error"]["params"]["scope"], "tools");
    for (method, path) in [
        (Method::GET, "/tools/log"),
        (Method::PUT, "/tools/policy"),
        (Method::PUT, "/tools/search-provider"),
    ] {
        let resp = api
            .send(method, path, &reader, Some(json!({"policy": "allow_all"})))
            .await;
        assert_eq!(resp.status(), 403, "{path}");
    }
    // Without a token: 401.
    let resp = api.http.get(api.url("/tools")).send().await.unwrap();
    assert_eq!(resp.status(), 401);

    // An admin named token is logged under its name; the client label can be set.
    let (status, body) = json(
        api.post("/tokens", json!({"name": "ci", "scopes": ["admin"]}))
            .await,
    )
    .await;
    assert_eq!(status, 201, "{body}");
    let admin = body["token"].as_str().unwrap().to_string();
    let marker = format!("marker-{}", uuid_like());
    let (status, _) = json(
        api.send(
            Method::POST,
            "/tools/grep_search/run",
            &admin,
            Some(json!({"args": {"path": ".", "pattern": marker}, "workdir": wd, "model": "qwen", "conversation": "c-1"})),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200);
    let (status, _) = json(
        api.send(
            Method::POST,
            "/tools/read_file/run",
            &api.token,
            Some(json!({"args": {"path": "a.txt"}, "workdir": wd, "client": "cli"})),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200);
    // A refused call is logged too.
    set_policy(&api, "off", &[], "").await;
    let (status, _) = run(
        &api,
        "read_file",
        json!({"args": {"path": "a.txt"}, "workdir": wd}),
    )
    .await;
    assert_eq!(status, 403);
    set_policy(&api, "ask_every_time", &[], "").await;

    let (status, log) = json(api.get("/tools/log?limit=20").await).await;
    assert_eq!(status, 200, "{log}");
    let rows = log.as_array().unwrap();
    let grep = rows
        .iter()
        .find(|r| r["args"].as_str().unwrap_or("").contains(&marker))
        .unwrap();
    assert_eq!(grep["client"], "api:ci");
    assert_eq!(grep["tool"], "grep_search");
    assert_eq!(grep["class"], "restricted");
    assert_eq!(grep["model"], "qwen");
    assert_eq!(grep["conversation"], "c-1");
    assert!(grep["error_code"].is_null());
    assert!(grep["duration_ms"].is_number() && grep["result_chars"].as_u64().unwrap() > 0);
    assert!(rows
        .iter()
        .any(|r| r["client"] == "cli" && r["tool"] == "read_file"));
    assert!(rows
        .iter()
        .any(|r| r["tool"] == "read_file" && r["error_code"] == "policy_off"));
    // Filters.
    let (_, only) = json(api.get("/tools/log?tool=grep_search&client=api:ci").await).await;
    assert!(only
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["tool"] == "grep_search"));
    assert!(!only.as_array().unwrap().is_empty());
    api.server.stop().await;
}

fn uuid_like() -> String {
    format!(
        "{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

#[tokio::test]
async fn automatic_state_is_admin_and_inventory_is_readable() {
    let _g = ONE_AT_A_TIME.lock().await;
    let api = common::start().await;
    let (_, body) = common::json(
        api.post("/tokens", json!({"name":"auto-reader","scopes":["read"]}))
            .await,
    )
    .await;
    let token = body["token"].as_str().unwrap();
    assert_eq!(
        api.send(Method::GET, "/tools/automatic", token, None)
            .await
            .status(),
        reqwest::StatusCode::FORBIDDEN
    );
    assert_eq!(
        api.send(Method::GET, "/tools/inventory", token, None)
            .await
            .status(),
        reqwest::StatusCode::OK
    );
    let (_, state) = common::json(api.get("/tools/automatic").await).await;
    assert_eq!(state["version"], 1);
    assert!(state.get("endpoint").is_none());
}

const REGISTRY_COMMIT: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn registry_entry(kind: &str, dir: &str, network: &[&str]) -> Value {
    json!({
        "id": format!("struklabs.{kind}.{dir}"),
        "type": kind,
        "publisher": "StrukLabs",
        "name": dir,
        "description": {"en": "An entry.", "tr": "Bir girdi."},
        "version": "1.0.0",
        "license": "MIT",
        "homepage": "https://github.com/struklabs/harness-registry",
        "source": "https://github.com/struklabs/harness-registry",
        "platforms": ["windows", "macos", "linux"],
        "permissions": {"network": network, "fs": {"read": [], "write": []}, "shell": false},
        "tags": ["test"],
        "min_harness_version": "0.1.0",
        "artifact": match kind {
            "skill" => format!("skills/{dir}/SKILL.md"),
            "mcp" => format!("mcp/{dir}/config.json"),
            _ => format!("tools/{dir}"),
        },
    })
}

#[tokio::test]
async fn installed_tools_and_skills_can_be_switched_and_removed_over_the_api() {
    let _one = ONE_AT_A_TIME.lock().await;
    let api = start().await;
    let root = harness_core::tools::installed::tools_root();
    let tool = root.join("api_demo");
    std::fs::create_dir_all(&tool).unwrap();
    std::fs::write(
        tool.join("manifest.json"),
        json!({"name": "api_demo", "version": "1.0.0", "description": "Demo", "input_schema": {"type": "object"},
            "output": "text", "runtime": "js", "entry": "index.js",
            "permissions": {"network": [], "fs": {"read": [], "write": []}, "shell": false}})
        .to_string(),
    )
    .unwrap();
    std::fs::write(tool.join("index.js"), "function run(args) { return 'ok'; }").unwrap();
    let skill = harness_core::skills::root().join("api-demo-skill");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: api-demo-skill\ndescription: A demo skill.\n---\nDo the demo.",
    )
    .unwrap();

    let (status, body) = json(api.get("/tools/installed").await).await;
    assert_eq!(status, 200, "{body}");
    let listed = body
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "api_demo")
        .unwrap();
    assert_eq!(listed["enabled"], true);

    let (status, body) = json(
        api.post("/tools/installed/api_demo/disable", json!({}))
            .await,
    )
    .await;
    assert_eq!((status, &body["enabled"]), (200, &json!(false)), "{body}");
    let (status, body) = json(
        api.post("/tools/installed/api_demo/enable", json!({}))
            .await,
    )
    .await;
    assert_eq!((status, &body["enabled"]), (200, &json!(true)), "{body}");
    let (status, body) = json(api.post("/tools/installed/nope/enable", json!({})).await).await;
    assert_eq!(status, 422, "{body}");
    assert_eq!(body["error"]["code"], "tools.package.not_installed");

    let (status, body) = json(api.get("/skills").await).await;
    assert_eq!(status, 200, "{body}");
    assert!(body
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["name"] == "api-demo-skill" && s["enabled"] == true));
    let (status, body) = json(api.post("/skills/api-demo-skill/disable", json!({})).await).await;
    assert_eq!((status, &body["enabled"]), (200, &json!(false)), "{body}");
    let (status, body) = json(api.post("/skills/api-demo-skill/enable", json!({})).await).await;
    assert_eq!((status, &body["enabled"]), (200, &json!(true)), "{body}");
    let (status, body) = json(api.post("/skills/missing/enable", json!({})).await).await;
    assert_eq!(status, 404, "{body}");

    // A `tools` token runs tools but cannot install, switch or remove anything.
    let (_, body) = json(
        api.post("/tokens", json!({"name": "runner", "scopes": ["tools"]}))
            .await,
    )
    .await;
    let runner = body["token"].as_str().unwrap().to_string();
    for (method, path) in [
        (Method::POST, "/tools/installed/api_demo/disable"),
        (Method::DELETE, "/tools/installed/api_demo"),
        (Method::POST, "/skills/api-demo-skill/disable"),
        (Method::DELETE, "/skills/api-demo-skill"),
        (Method::POST, "/tools/registry/install"),
    ] {
        let (status, body) = json(api.send(method, path, &runner, Some(json!({}))).await).await;
        assert_eq!(status, 403, "{path}: {body}");
        assert_eq!(body["error"]["code"], "api.insufficient_scope");
    }

    let resp = api
        .send(
            Method::DELETE,
            "/tools/installed/api_demo",
            &api.token,
            None,
        )
        .await;
    assert_eq!(resp.status(), 204);
    let resp = api
        .send(Method::DELETE, "/skills/api-demo-skill", &api.token, None)
        .await;
    assert_eq!(resp.status(), 204);
    assert!(!tool.exists() && !skill.exists());
    let resp = api
        .send(Method::DELETE, "/skills/api-demo-skill", &api.token, None)
        .await;
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn registry_install_is_never_silent_and_leaves_mcp_to_the_app() {
    let _one = ONE_AT_A_TIME.lock().await;
    let api = start().await;
    let snapshot = json!({
        "commit": REGISTRY_COMMIT,
        "etag": null,
        "offline": false,
        "index": {"schema_version": 1, "entries": [
            registry_entry("skill", "api-demo", &["example.com"]),
            registry_entry("mcp", "api-demo", &["example.com"]),
        ]},
    });
    let cache = harness_core::paths::data_root().join("registry/index.json");
    std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
    std::fs::write(&cache, snapshot.to_string()).unwrap();
    let skill = "struklabs.skill.api-demo";
    let declared =
        json!({"network": ["example.com"], "fs": {"read": [], "write": []}, "shell": false});

    // No approval, or a different one: refused before anything is downloaded, with the
    // declaration in the error so the caller can show it.
    for approved in [
        None,
        Some(json!({"network": [], "fs": {"read": [], "write": []}, "shell": false})),
    ] {
        let mut request = json!({"commit": REGISTRY_COMMIT, "id": skill});
        if let Some(approved) = approved {
            request["approved_permissions"] = approved;
        }
        let (status, body) = json(api.post("/tools/registry/install", request).await).await;
        assert_eq!(status, 403, "{body}");
        assert_eq!(body["error"]["code"], "tools.install_approval_required");
        let shown: Value =
            serde_json::from_str(body["error"]["params"]["permissions"].as_str().unwrap()).unwrap();
        assert_eq!(shown, declared);
    }
    assert!(!harness_core::skills::root().join("api-demo").exists());

    // MCP entries change the server list, which stays in the app.
    let (status, body) = json(
        api.post(
            "/tools/registry/install",
            json!({"commit": REGISTRY_COMMIT, "id": "struklabs.mcp.api-demo", "approved_permissions": declared}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 422, "{body}");
    assert_eq!(body["error"]["code"], "tools.registry_kind_unsupported");

    // A stale listing or an unknown entry is a registry error, not an install.
    let (status, body) = json(
        api.post(
            "/tools/registry/install",
            json!({"commit": "cccccccccccccccccccccccccccccccccccccccc", "id": skill, "approved_permissions": declared}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 422, "{body}");
    assert_eq!(body["error"]["code"], "tools.registry_failed");
}
