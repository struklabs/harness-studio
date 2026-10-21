//! Real HTTP against the real Control API on 127.0.0.1 (ephemeral port, temp data dirs).

mod common;

use common::{json, start, start_with, Api};
use harness_core::control_api::{self, ControlApiSettings};
use reqwest::Method;
use serde_json::json;

fn assert_error(body: &serde_json::Value, code: &str) {
    assert_eq!(body["error"]["code"], code, "{body}");
    assert_eq!(body["error"]["message_key"], code, "{body}");
    assert!(body["error"]["params"].is_object(), "{body}");
}

/// The contract version is on every response (also errors and refused requests), and
/// `/capabilities` is public and says which versions and features this build has.
#[tokio::test]
async fn api_version_header_and_capabilities() {
    let api = start().await;
    let want = harness_core::control_api::API_VERSION.to_string();
    let header = harness_core::control_api::API_VERSION_HEADER;
    for path in ["/health", "/status", "/no-such-route"] {
        let resp = api.http.get(api.url(path)).send().await.unwrap();
        assert_eq!(resp.headers()[header], want.as_str(), "{path}");
    }
    let resp = api
        .http
        .get(api.url("/health"))
        .header("origin", "https://evil.example")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403);
    assert_eq!(resp.headers()[header], want.as_str());

    let resp = api.http.get(api.url("/capabilities")).send().await.unwrap();
    assert_eq!(resp.headers()[header], want.as_str());
    let (status, body) = json(resp).await;
    assert_eq!(status, 200);
    assert_eq!(body["api_version"], 1);
    assert_eq!(body["min_api_version"], 1);
    assert_eq!(body["max_api_version"], 1);
    assert_eq!(body["module_manifest_versions"], serde_json::json!([1]));
    assert_eq!(body["features"]["control_api"], true);
    assert_eq!(body["features"]["training_backend"], false);

    let doc = harness_api::openapi_document().to_json().unwrap();
    let doc: serde_json::Value = serde_json::from_str(&doc).unwrap();
    assert_eq!(doc["info"]["x-harness-api-version"], 1);
}

#[tokio::test]
async fn health_is_public_everything_else_needs_a_token() {
    let api = start().await;
    let (status, body) = json(api.http.get(api.url("/health")).send().await.unwrap()).await;
    assert_eq!(status, 200);
    assert_eq!(body["status"], "ok");

    for path in [
        "/status",
        "/models",
        "/servers",
        "/events",
        "/tokens",
        "/definitions",
    ] {
        let resp = api.http.get(api.url(path)).send().await.unwrap();
        assert_eq!(
            resp.headers()["www-authenticate"],
            "Bearer realm=\"harness\"",
            "{path}"
        );
        let (status, body) = json(resp).await;
        assert_eq!(status, 401, "{path}");
        assert_error(&body, "api.unauthorized");
    }
    // Wrong token (same length as the real one), wrong scheme, empty token.
    let mut wrong = api.token.clone();
    wrong.pop();
    wrong.push('x');
    for header in [
        format!("Bearer {}", wrong),
        format!("Basic {}", api.token),
        "Bearer ".to_string(),
    ] {
        let resp = api
            .http
            .get(api.url("/status"))
            .header("authorization", header.clone())
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 401, "{header}");
    }

    let (status, body) = json(api.get("/status").await).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["api_version"], "v1");
    assert_eq!(body["instance"]["kind"], "headless");
    assert_eq!(body["instance"]["pid"], std::process::id());
    assert!(body["hardware"]["cpu_cores"].as_u64().unwrap() > 0);
    assert!(body["runtime"]["mode"].is_string());
    let (status, hw) = json(api.get("/hardware").await).await;
    assert_eq!(status, 200);
    assert!(hw["total_ram_bytes"].as_u64().unwrap() > 0);
    api.server.stop().await;
}

#[tokio::test]
async fn named_token_scopes_are_enforced_and_revocation_is_immediate() {
    let api = start().await;
    let admin = api.token.clone();

    let (status, created) = json(
        api.post("/tokens", json!({"name": "reader", "scopes": ["read"]}))
            .await,
    )
    .await;
    assert_eq!(status, 201, "{created}");
    let reader = created["token"].as_str().unwrap().to_string();
    let reader_id = created["info"]["id"].as_str().unwrap().to_string();
    assert!(reader.starts_with("hs_"));

    // The listing never shows values or hashes.
    let (_, list) = json(api.get("/tokens").await).await;
    let text = list.to_string();
    assert!(text.contains("reader"));
    assert!(!text.contains(&reader[16..]));
    assert!(!text.contains("hash") && !text.contains("salt"));

    // read: GETs work, writes and admin routes do not.
    let resp = api.send(Method::GET, "/models", &reader, None).await;
    assert_eq!(resp.status(), 200);
    let (status, body) = json(
        api.send(
            Method::POST,
            "/definitions",
            &reader,
            Some(json!({"alias": "x", "base": {"kind": "hf", "repo": "a/b", "file": "c.gguf"}})),
        )
        .await,
    )
    .await;
    assert_eq!(status, 403);
    assert_error(&body, "api.insufficient_scope");
    assert_eq!(body["error"]["params"]["scope"], "models:write");
    let (status, body) = json(api.send(Method::GET, "/tokens", &reader, None).await).await;
    assert_eq!(status, 403);
    assert_eq!(body["error"]["params"]["scope"], "admin");
    let (status, _) = json(
        api.send(
            Method::POST,
            "/servers/x/exposure-ack",
            &reader,
            Some(json!({"host": "0.0.0.0"})),
        )
        .await,
    )
    .await;
    assert_eq!(status, 403);

    // servers:write can start/stop but not change models; it implies read.
    let (_, ops) = json(
        api.post(
            "/tokens",
            json!({"name": "ops", "scopes": ["servers:write"]}),
        )
        .await,
    )
    .await;
    let ops = ops["token"].as_str().unwrap().to_string();
    assert_eq!(
        api.send(Method::GET, "/servers", &ops, None).await.status(),
        200
    );
    assert_eq!(
        api.send(Method::DELETE, "/definitions/nope", &ops, None)
            .await
            .status(),
        403
    );
    // A servers:write token reaches the handler (404: no such server) instead of 403.
    assert_eq!(
        api.send(Method::POST, "/servers/nope/stop", &ops, None)
            .await
            .status(),
        404
    );

    // Invalid token requests.
    let (status, body) = json(
        api.post("/tokens", json!({"name": " ", "scopes": ["read"]}))
            .await,
    )
    .await;
    assert_eq!(status, 400);
    assert_error(&body, "api.token_name_invalid");
    let (status, _) = json(
        api.post("/tokens", json!({"name": "x", "scopes": ["root"]}))
            .await,
    )
    .await;
    assert_eq!(
        status, 422,
        "unknown scope is rejected by the JSON extractor"
    );

    // Revoke: next request with it is 401.
    let resp = api
        .send(
            Method::DELETE,
            &format!("/tokens/{}", reader_id),
            &admin,
            None,
        )
        .await;
    assert_eq!(resp.status(), 204);
    assert_eq!(
        api.send(Method::GET, "/models", &reader, None)
            .await
            .status(),
        401
    );
    let (status, body) = json(
        api.send(
            Method::DELETE,
            &format!("/tokens/{}", reader_id),
            &admin,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(status, 404);
    assert_error(&body, "api.token_not_found");

    // Hashes only on disk.
    let file =
        std::fs::read_to_string(api.dir.path().join("config").join("api_tokens.json")).unwrap();
    assert!(!file.contains(&ops[16..]));
    api.server.stop().await;
}

#[tokio::test]
async fn session_token_file_is_private_and_rotates_on_every_start() {
    let api = start().await;
    let path = control_api::session_token_path(api.dir.path());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), api.token);
    assert!(harness_core::instance::is_private_to_current_user(&path).unwrap());
    let status = api.server.status().await;
    assert!(status.running);
    assert_eq!(status.token_path.as_deref(), Some(path.as_path()));

    // Restart: new token, the old one is refused, the file follows.
    let old = api.token.clone();
    let port = status.port.unwrap();
    let restarted = api
        .server
        .start(api.ctx.clone(), &common::settings(port))
        .await
        .unwrap();
    let new = api.server.session_token().await.unwrap();
    assert_ne!(old, new);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), new);
    let url = format!("{}/status", restarted.base_url.unwrap());
    let resp = api.http.get(&url).bearer_auth(&old).send().await.unwrap();
    assert_eq!(resp.status(), 401);
    let resp = api.http.get(&url).bearer_auth(&new).send().await.unwrap();
    assert_eq!(resp.status(), 200);

    api.server.stop().await;
    assert!(!path.exists(), "stop removes the token file");
    assert!(!api.server.status().await.running);
    assert!(api.http.get(&url).send().await.is_err(), "listener closed");
}

#[tokio::test]
async fn instance_json_records_the_api_while_it_runs() {
    let dir = tempfile::tempdir().unwrap();
    let lock = std::sync::Arc::new(
        harness_core::instance::InstanceLock::acquire(
            dir.path(),
            harness_core::instance::InstanceKind::Headless,
            "test",
        )
        .unwrap(),
    );
    let ctx = common::context(dir.path());
    let server = harness_api::ApiServer::new();
    server.set_instance(Some(lock.clone()));
    let (listener, port) = common::listener().await;
    server
        .start_on(ctx, &common::settings(port), listener)
        .await
        .unwrap();
    let info = harness_core::instance::read_info(dir.path()).unwrap();
    assert_eq!(info.pid, std::process::id());
    assert_eq!(info.api.as_ref().unwrap().port, port);
    assert_eq!(info.api.as_ref().unwrap().host, "127.0.0.1");
    let token_path = info.token_path.unwrap();
    assert_eq!(
        std::fs::read_to_string(&token_path).unwrap(),
        server.session_token().await.unwrap()
    );
    server.stop().await;
    let info = harness_core::instance::read_info(dir.path()).unwrap();
    assert!(info.api.is_none() && info.token_path.is_none());
    server.set_instance(None);
    drop(lock);
    assert!(harness_core::instance::read_info(dir.path()).is_none());
}

#[tokio::test]
async fn bind_rules_and_settings_validation() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = common::context(dir.path());
    let server = harness_api::ApiServer::new();
    let lan = ControlApiSettings {
        enabled: true,
        host: "0.0.0.0".into(),
        port: common::free_port(),
        ..Default::default()
    };
    assert_eq!(
        server.start(ctx.clone(), &lan).await.unwrap_err(),
        control_api::ERR_NETWORK_NOT_ALLOWED
    );
    let lan = ControlApiSettings {
        allow_network: true,
        ..lan
    };
    assert_eq!(
        server.start(ctx.clone(), &lan).await.unwrap_err(),
        control_api::ERR_NETWORK_WITHOUT_TOKEN
    );
    assert_eq!(
        server.status().await.error.as_deref(),
        Some(control_api::ERR_NETWORK_WITHOUT_TOKEN)
    );
    let bad = ControlApiSettings {
        host: "my-pc.local".into(),
        ..common::settings(common::free_port())
    };
    assert_eq!(
        server.start(ctx.clone(), &bad).await.unwrap_err(),
        harness_core::network::ERR_INVALID_HOST
    );
    // A port held by another program: the API starts on a fallback port, says which program
    // holds the configured one and remembers the fallback (`config/ports.json`).
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let configured = taken.local_addr().unwrap().port();
    let status = server
        .start(ctx.clone(), &common::settings(configured))
        .await
        .unwrap();
    assert!(status.running);
    let port = status.port.unwrap();
    assert_ne!(port, configured);
    let fallback = status.port_fallback.expect("fallback reported");
    assert_eq!(fallback.configured, configured);
    assert_eq!(fallback.holder.map(|h| h.pid), Some(std::process::id()));
    assert_eq!(
        harness_core::ports::PortState::load(dir.path()).control_api,
        Some(port)
    );
    server.stop().await;
    assert!(!server.status().await.running);
}

#[tokio::test]
async fn host_header_must_be_loopback_on_a_loopback_bind() {
    let api = start().await;
    let port = api.server.status().await.port.unwrap();
    for (host, ok) in [
        (format!("127.0.0.1:{port}"), true),
        (format!("localhost:{port}"), true),
        (format!("evil.example:{port}"), false),
        ("attacker.test".to_string(), false),
    ] {
        let resp = api
            .http
            .get(api.url("/status"))
            .header("host", host.clone())
            .bearer_auth(&api.token)
            .send()
            .await
            .unwrap();
        let (status, body) = json(resp).await;
        if ok {
            assert_eq!(status, 200, "{host}");
        } else {
            assert_eq!(status, 403, "{host}");
            assert_error(&body, "api.forbidden_host");
        }
    }
    api.server.stop().await;
}

#[tokio::test]
async fn cors_is_off_by_default_and_follows_the_allow_list() {
    let api = start().await;
    let resp = api
        .http
        .get(api.url("/health"))
        .header("origin", "http://localhost:5173")
        .send()
        .await
        .unwrap();
    assert!(resp.headers().get("access-control-allow-origin").is_none());
    // No CORS list: a browser page is refused outright, not just kept from reading.
    assert_eq!(resp.status(), 403);
    api.server.stop().await;

    let api = start_with(|s| s.cors_origins = vec!["http://localhost:5173".into()]).await;
    let preflight = api
        .http
        .request(Method::OPTIONS, api.url("/status"))
        .header("origin", "http://localhost:5173")
        .header("access-control-request-method", "GET")
        .header("access-control-request-headers", "authorization")
        .send()
        .await
        .unwrap();
    assert!(preflight.status().is_success(), "{}", preflight.status());
    assert_eq!(
        preflight.headers()["access-control-allow-origin"],
        "http://localhost:5173"
    );
    let other = api
        .http
        .get(api.url("/health"))
        .header("origin", "http://evil.example")
        .send()
        .await
        .unwrap();
    assert!(other.headers().get("access-control-allow-origin").is_none());
    assert_eq!(other.status(), 403);
    api.server.stop().await;
}

#[tokio::test]
async fn foreign_browser_origins_are_refused_before_any_route() {
    let api = start_with(|s| s.cors_origins = vec!["http://localhost:5173".into()]).await;
    // A simple cross-site POST (no preflight), a preflight, `Origin: null` and a no-cors
    // request: all 403 api.forbidden_origin, even with a valid token.
    type Case<'a> = (Method, &'a str, Vec<(&'a str, &'a str)>);
    let cases: Vec<Case> = vec![
        (
            Method::POST,
            "/tokens",
            vec![
                ("origin", "http://evil.example"),
                ("content-type", "text/plain"),
            ],
        ),
        (
            Method::OPTIONS,
            "/status",
            vec![
                ("origin", "http://evil.example"),
                ("access-control-request-method", "GET"),
            ],
        ),
        (Method::GET, "/health", vec![("origin", "null")]),
        (
            Method::GET,
            "/health",
            vec![("sec-fetch-site", "cross-site")],
        ),
    ];
    for (method, path, headers) in cases {
        let mut req = api
            .http
            .request(method.clone(), api.url(path))
            .bearer_auth(&api.token);
        for (k, v) in &headers {
            req = req.header(*k, *v);
        }
        let (status, body) = json(req.send().await.unwrap()).await;
        assert_eq!(status, 403, "{method} {path} {headers:?}");
        assert_error(&body, "api.forbidden_origin");
    }
    // An allowed origin (cross-site fetch included) and no origin keep working.
    let ok = api
        .http
        .get(api.url("/health"))
        .header("origin", "http://localhost:5173")
        .header("sec-fetch-site", "cross-site")
        .send()
        .await
        .unwrap();
    assert_eq!(ok.status(), 200);
    assert_eq!(
        ok.headers()["access-control-allow-origin"],
        "http://localhost:5173"
    );
    let (status, _) = json(api.get("/status").await).await;
    assert_eq!(status, 200);
    api.server.stop().await;
}

#[tokio::test]
async fn openapi_document_is_valid_and_lists_every_route() {
    let api = start().await;
    // Public on loopback.
    let resp = api.http.get(api.url("/openapi.json")).send().await.unwrap();
    assert_eq!(resp.status(), 200);
    let doc: serde_json::Value = resp.json().await.unwrap();
    assert!(doc["openapi"].as_str().unwrap().starts_with("3.1"));
    assert_eq!(doc["info"]["title"], "Harness Control API");
    assert_eq!(
        doc["components"]["securitySchemes"]["bearer"]["scheme"],
        "bearer"
    );
    let paths = doc["paths"].as_object().unwrap();
    let expected: &[(&str, &[&str])] = &[
        ("/browser/backup", &["post"]),
        ("/browser/restore", &["post"]),
        ("/browser/service", &["get", "post"]),
        ("/browser/logs", &["get"]),
        ("/browser/update", &["get", "post"]),
        ("/health", &["get"]),
        ("/capabilities", &["get"]),
        ("/browser/session", &["get"]),
        ("/browser/login", &["post"]),
        ("/browser/logout", &["post"]),
        ("/browser/sessions", &["delete"]),
        ("/browser/settings", &["get", "put"]),
        ("/status", &["get"]),
        ("/settings", &["get", "patch"]),
        ("/hardware", &["get"]),
        ("/hardware/devices", &["get"]),
        ("/gateway", &["get"]),
        ("/jobs", &["get"]),
        ("/runtime", &["get"]),
        ("/runtime/flags", &["get"]),
        ("/runtime/managed", &["get"]),
        ("/runtime/managed/install", &["post"]),
        ("/runtime/managed/rollback", &["post"]),
        ("/modules", &["get"]),
        ("/modules/{id}", &["post", "delete"]),
        ("/modules/{id}/cancel", &["post"]),
        ("/modules/{id}/action", &["post"]),
        ("/modules/{id}/remove-data", &["post"]),
        ("/modules/{id}/export", &["post"]),
        ("/modules/import", &["post"]),
        ("/runtimes", &["get", "post"]),
        ("/runtimes/{id}", &["get", "delete"]),
        ("/runtimes/{id}/verify", &["post"]),
        ("/runtimes/release", &["get"]),
        ("/runtimes/advice", &["get"]),
        ("/runtimes/progress", &["get"]),
        ("/onnx-runtime", &["get"]),
        ("/onnx-runtime/progress", &["get"]),
        ("/onnx-runtime/{variant}", &["post", "delete"]),
        ("/onnx-runtime/{variant}/verify", &["post"]),
        ("/lemonade", &["get"]),
        ("/lemonade/release", &["get"]),
        ("/lemonade/install", &["post"]),
        ("/lemonade/installs/{id}", &["delete"]),
        ("/lemonade/settings", &["put"]),
        ("/lemonade/start", &["post"]),
        ("/lemonade/stop", &["post"]),
        ("/lemonade/logs", &["get"]),
        ("/lemonade/models", &["get"]),
        ("/lemonade/progress", &["get"]),
        ("/lemonade/models/pull", &["post"]),
        ("/lemonade/models/delete", &["post"]),
        ("/lemonade/backends/install", &["post"]),
        ("/openapi.json", &["get"]),
        ("/shutdown", &["post"]),
        ("/models", &["get"]),
        ("/models/{id}", &["get", "delete"]),
        ("/models/{id}/fit", &["get"]),
        ("/models/{id}/automatic", &["get"]),
        ("/models/pull", &["post"]),
        ("/convert", &["get", "post"]),
        ("/convert/plan", &["get"]),
        ("/convert/env", &["get", "post", "delete"]),
        ("/convert/{id}", &["get", "delete"]),
        ("/convert/{id}/cancel", &["post"]),
        ("/downloads", &["get"]),
        ("/downloads/{id}", &["get", "delete"]),
        ("/downloads/{id}/pause", &["post"]),
        ("/downloads/{id}/resume", &["post"]),
        ("/definitions", &["get", "post"]),
        ("/definitions/{id}", &["get", "put", "delete"]),
        ("/definitions/{id}/duplicate", &["post"]),
        ("/modelfile/preview", &["post"]),
        ("/modelfile/import", &["post"]),
        ("/presets", &["get", "post"]),
        ("/presets/{id}", &["get", "put", "delete"]),
        ("/servers", &["get", "post"]),
        ("/servers/{id}", &["get", "put", "delete"]),
        ("/servers/{id}/start", &["post"]),
        ("/servers/{id}/stop", &["post"]),
        ("/servers/{id}/recovery", &["get"]),
        ("/servers/{id}/retry", &["post"]),
        ("/servers/{id}/router/models", &["get"]),
        ("/servers/{id}/router/models/load", &["post"]),
        ("/servers/{id}/router/models/unload", &["post"]),
        ("/servers/{id}/exposure-ack", &["post"]),
        ("/load", &["post"]),
        ("/unload", &["post"]),
        ("/servers/{id}/metrics", &["get"]),
        ("/servers/{id}/effective", &["get"]),
        ("/servers/{id}/api-key", &["get"]),
        ("/servers/{id}/lora", &["get", "post"]),
        ("/servers/{id}/logs", &["get"]),
        ("/servers/{id}/logs/stream", &["get"]),
        ("/benchmarks", &["get", "post"]),
        ("/benchmarks/{id}", &["get"]),
        ("/benchmarks/{id}/cancel", &["post"]),
        ("/events", &["get"]),
        ("/tokens", &["get", "post"]),
        ("/tokens/{id}", &["get", "patch", "delete"]),
        ("/cloud/providers", &["get", "post"]),
        ("/cloud/providers/{id}", &["put", "delete"]),
        ("/cloud/providers/{id}/test", &["post"]),
        ("/cloud/models", &["get"]),
        ("/otel", &["get", "put"]),
        ("/otel/test", &["post"]),
        ("/automations/n8n", &["get", "put"]),
        ("/automations/n8n/test", &["post"]),
        ("/automations/n8n/workflows", &["get"]),
        ("/automations/n8n/executions", &["get"]),
        ("/automations/n8n/executions/{id}/error", &["get"]),
        ("/automations/n8n/workflows/{id}/trigger", &["post"]),
        ("/automations/n8n/openai", &["get"]),
        ("/automations/n8n/permission", &["put"]),
        ("/automations/n8n/proposals", &["get"]),
        ("/automations/n8n/proposals/{id}", &["get"]),
        ("/automations/n8n/proposals/{id}/apply", &["post"]),
        ("/automations/n8n/proposals/{id}/reject", &["post"]),
        ("/analytics/summary", &["get"]),
        ("/analytics/requests", &["get"]),
        ("/analytics/requests/{id}", &["get"]),
        ("/analytics/queue-hints", &["get"]),
        ("/conversations", &["get"]),
        ("/conversations/{id}", &["get"]),
        ("/conversations/{id}/export", &["get"]),
        ("/conversations/{id}/documents", &["get"]),
        ("/conversations/{id}/compact", &["post"]),
        ("/conversations/{id}/compact/cancel", &["post"]),
        ("/conversations/{id}/compactions/{message_id}", &["put"]),
        ("/metrics", &["get"]),
        ("/tools", &["get"]),
        ("/tools/inventory", &["get"]),
        ("/tools/automatic", &["get"]),
        ("/tools/automatic/start", &["post"]),
        ("/tools/automatic/action", &["post"]),
        ("/tools/policy", &["get", "put"]),
        ("/tools/search-provider", &["get", "put"]),
        ("/tools/log", &["get"]),
        ("/tools/registry", &["get"]),
        ("/tools/registry/install", &["post"]),
        ("/tools/installed", &["get"]),
        ("/tools/installed/{name}", &["delete"]),
        ("/tools/installed/{name}/enable", &["post"]),
        ("/tools/installed/{name}/disable", &["post"]),
        ("/skills", &["get"]),
        ("/skills/{name}", &["delete"]),
        ("/skills/{name}/enable", &["post"]),
        ("/skills/{name}/disable", &["post"]),
        ("/tools/mcp", &["get"]),
        ("/tools/mcp/{server}/resources", &["get"]),
        ("/tools/mcp/{server}/resource", &["get"]),
        ("/tools/mcp/{server}/authorize", &["post"]),
        ("/tools/mcp/{server}/logout", &["post"]),
        ("/tools/{name}/run", &["post"]),
        ("/training", &["get", "post"]),
        ("/training/compatibility", &["get"]),
        ("/training/dataset", &["post"]),
        ("/training/env", &["get", "post"]),
        ("/training/name", &["post"]),
        ("/training/presets", &["post"]),
        ("/training/preview", &["post"]),
        ("/training/{id}", &["get", "delete"]),
        ("/training/{id}/cancel", &["post"]),
        ("/training/{id}/export", &["post"]),
        ("/training/{id}/compare", &["post"]),
        ("/training/{id}/log", &["get"]),
        ("/training/{id}/metrics", &["get"]),
        ("/training/{id}/pause", &["post"]),
        ("/training/{id}/rename", &["post"]),
        ("/training/{id}/restart", &["post"]),
        ("/training/{id}/resume", &["post"]),
        ("/training/{id}/start", &["post"]),
        ("/permissions", &["get"]),
        ("/permissions/grants", &["post"]),
        ("/permissions/{kind}/{id}", &["delete"]),
        ("/permissions/{kind}/{id}/grants/{grant_id}", &["delete"]),
        ("/permissions/{kind}/{id}/trust", &["put"]),
        ("/workloads", &["get"]),
        ("/workloads/check", &["post"]),
        ("/workloads/consents", &["post"]),
        ("/workloads/consents/{id}", &["delete"]),
        ("/checkpoints", &["get", "delete"]),
        ("/checkpoints/{id}/undo", &["post"]),
        ("/checkpoints/undo-run", &["post"]),
    ];
    for (path, methods) in expected {
        let full = format!("{}{}", control_api::BASE_PATH, path);
        let item = paths.get(&full).unwrap_or_else(|| panic!("missing {full}"));
        for m in *methods {
            assert!(item.get(*m).is_some(), "{m} {full}");
        }
    }
    // Gateway-listener operations are documented in the same snapshot but are not Control API routes.
    let gateway = ["/v1/ocr", "/v1/document/qa"];
    for path in gateway {
        assert!(paths[path]["post"]["requestBody"].is_object(), "{path}");
    }
    assert_eq!(
        paths.len(),
        expected.len() + gateway.len(),
        "{:?}",
        paths.keys().collect::<Vec<_>>()
    );
    // Every listed route answers (no 404 from the router) — spot-check with the session token.
    for path in [
        "/runtime",
        "/definitions",
        "/presets",
        "/servers",
        "/benchmarks",
        "/downloads",
    ] {
        let status = api.get(path).await.status();
        assert_ne!(status, 404, "{path}");
    }
    // Public operations declare empty security; protected operations inherit bearer/cookie auth.
    let health = &paths[&format!("{}/health", control_api::BASE_PATH)]["get"];
    assert_eq!(health["security"], json!([{}]));
    // Every $ref resolves.
    let schemas = doc["components"]["schemas"].as_object().unwrap();
    let text = doc.to_string();
    for part in text.split("\"$ref\":\"#/components/schemas/").skip(1) {
        let name = part.split('"').next().unwrap();
        assert!(schemas.contains_key(name), "unresolved $ref {name}");
    }
    // No secret fields: API keys are never part of a schema (only `api_key_set` / refs).
    for name in ["ActiveServerInfo", "LaunchConfig"] {
        let props = schemas[name]["properties"].as_object().unwrap();
        assert!(!props.contains_key("api_key"), "{name}");
    }
    assert!(schemas["ActiveServerInfo"]["properties"]
        .get("api_key_set")
        .is_some());
    assert!(schemas.contains_key("ProfileView"));
    assert!(schemas.contains_key("CoreEvent"));
    // Registry installs must not replace ONNX's Installed schema. Their enum variants also
    // need a distinct prefix so Python SDK generation cannot collide with InstalledTool.
    assert!(schemas["Installed"]["properties"].get("variant").is_some());
    assert!(schemas["RegistryInstalled"]["oneOf"].is_array());
    let registry_install = &paths[&format!("{}/tools/registry/install", control_api::BASE_PATH)]
        ["post"]["responses"]["200"]["content"]["application/json"]["schema"];
    assert_eq!(
        registry_install["$ref"],
        "#/components/schemas/RegistryInstalled"
    );
    // Same document as the library function.
    let lib_doc: serde_json::Value =
        serde_json::from_str(&harness_api::openapi_document().to_json().unwrap()).unwrap();
    assert_eq!(lib_doc, doc);
    api.server.stop().await;
}

/// What the SDK generators need: one unique `operationId` per operation (they become method
/// names), and the auth errors on every operation behind the guard.
#[test]
fn openapi_operations_have_unique_ids_and_auth_errors() {
    let doc: serde_json::Value =
        serde_json::from_str(&harness_api::openapi_document().to_json().unwrap()).unwrap();
    let mut ids = std::collections::HashSet::new();
    for (path, item) in doc["paths"].as_object().unwrap() {
        for (method, op) in item.as_object().unwrap() {
            let id = op["operationId"].as_str().unwrap_or_default();
            assert!(!id.is_empty(), "{method} {path}: no operationId");
            assert!(
                ids.insert(id.to_string()),
                "{method} {path}: duplicate {id}"
            );
            // A path parameter is one of the path's `{name}`s: a query struct without
            // `#[into_params(parameter_in = Query)]` would land in the path.
            let params = op["parameters"].as_array().cloned().unwrap_or_default();
            for p in &params {
                let name = p["name"].as_str().unwrap();
                if p["in"] == "path" {
                    assert!(
                        path.contains(&format!("{{{name}}}")),
                        "{method} {path}: `{name}` is not a path parameter"
                    );
                }
            }
            for seg in path.split('/').filter(|s| s.starts_with('{')) {
                let name = seg.trim_matches(['{', '}']);
                assert!(
                    params
                        .iter()
                        .any(|p| p["in"] == "path" && p["name"] == name),
                    "{method} {path}: `{name}` undeclared"
                );
            }
            if op["security"] == json!([{}]) {
                continue;
            }
            for code in ["401", "403"] {
                assert!(
                    op["responses"].get(code).is_some(),
                    "{method} {path}: no {code}"
                );
            }
        }
    }
}

/// `sdk/openapi.json` is the document the TypeScript and Python clients are generated from.
/// After an API change: `node sdk/generate.mjs`.
#[test]
fn sdk_openapi_snapshot_is_current() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sdk/openapi.json");
    let snapshot: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let doc: serde_json::Value =
        serde_json::from_str(&harness_api::openapi_document().to_json().unwrap()).unwrap();
    assert!(
        snapshot == doc,
        "sdk/openapi.json is out of date: run `node sdk/generate.mjs`"
    );
}

#[test]
fn training_export_schema_exposes_quantization_and_adapter_mode() {
    let doc: serde_json::Value =
        serde_json::from_str(&harness_api::openapi_document().to_json().unwrap()).unwrap();
    let schemas = &doc["components"]["schemas"];
    assert_eq!(
        schemas["TrainingQuant"]["enum"],
        json!(["F16", "Q8_0", "Q4_K_M", "Q5_K_M"])
    );
    let properties = &schemas["ExportRequest"]["properties"];
    assert_eq!(
        properties["quant"]["$ref"],
        "#/components/schemas/TrainingQuant"
    );
    assert_eq!(properties["adapter_only"]["type"], "boolean");
    assert_eq!(
        properties["quants"]["items"]["$ref"],
        "#/components/schemas/TrainingQuant"
    );
    assert_eq!(
        schemas["TrainingComparisonRow"]["properties"]["prompt"]["type"],
        "string"
    );
    assert_eq!(
        schemas["Metric"]["properties"]["samples"]["items"]["$ref"],
        "#/components/schemas/TrainingCheckpointSample"
    );
    assert!(
        doc["paths"]["/api/harness/v1/training/{id}/export"]["post"]["requestBody"].is_object()
    );
    assert_eq!(
        doc["paths"]["/api/harness/v1/training/{id}/compare"]["post"]["operationId"],
        "compare_training_job"
    );
    assert_eq!(
        doc["paths"]["/api/harness/v1/training/{id}/pause"]["post"]["operationId"],
        "pause_training_job"
    );
}

#[tokio::test]
async fn training_compare_rejects_invalid_prompts_before_starting_a_worker() {
    let api = start().await;
    for prompts in [
        json!([]),
        json!([" "]),
        json!(["a".repeat(2001)]),
        json!(vec!["p"; 9]),
    ] {
        let (status, body) = json(
            api.post("/training/0/compare", json!({"prompts": prompts}))
                .await,
        )
        .await;
        assert_eq!(status, 422, "{body}");
        assert_error(&body, "training.prompts_invalid");
    }
    api.server.stop().await;
}

/// The analytics routes answer from the real (possibly empty) `analytics.db` with the read
/// scope; `/metrics` is the Prometheus text format.
#[tokio::test]
async fn analytics_routes_need_read_and_answer() {
    let api = start().await;
    for path in ["/analytics/summary", "/analytics/requests", "/metrics"] {
        let resp = api.http.get(api.url(path)).send().await.unwrap();
        assert_eq!(resp.status(), 401, "{path}");
    }
    let (status, created) = json(
        api.post("/tokens", json!({"name": "an-reader", "scopes": ["read"]}))
            .await,
    )
    .await;
    assert_eq!(status, 201, "{created}");
    let reader = created["token"].as_str().unwrap().to_string();
    let get = |path: &str| api.http.get(api.url(path)).bearer_auth(&reader).send();
    let (status, s) = json(get("/analytics/summary?range=24h").await.unwrap()).await;
    assert_eq!(status, 200, "{s}");
    assert!(s["models"].is_array());
    assert_eq!(s["totals"]["alias"], "*");
    assert!(s["from_ms"].as_u64().is_some());
    assert_eq!(s["retention_days"], 30);
    let (status, s) = json(get("/analytics/summary?range=all").await.unwrap()).await;
    assert_eq!(status, 200);
    assert!(s["from_ms"].is_null());
    let (status, body) = json(get("/analytics/summary?range=12w").await.unwrap()).await;
    assert_eq!(status, 400);
    assert_error(&body, "api.invalid_range");
    let (status, page) = json(
        get("/analytics/requests?limit=5&status=error")
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, 200, "{page}");
    assert!(page["requests"].is_array());
    assert!(page["total"].is_u64());
    let (status, body) = json(get("/analytics/requests/req_nothing").await.unwrap()).await;
    assert_eq!(status, 404);
    assert_error(&body, "api.not_found");
    let resp = get("/metrics").await.unwrap();
    assert_eq!(resp.status(), 200);
    assert!(resp.headers()["content-type"]
        .to_str()
        .unwrap()
        .starts_with("text/plain"));
    let text = resp.text().await.unwrap();
    assert!(text.contains("# TYPE harness_gateway_up gauge"), "{text}");
    assert!(text.contains("harness_analytics_records_dropped_total"));
    api.server.stop().await;
}

#[tokio::test]
async fn errors_are_structured() {
    let api = start().await;
    let (status, body) = json(api.get("/definitions/md-does-not-exist").await).await;
    assert_eq!(status, 404);
    assert_error(&body, "api.not_found");
    assert_eq!(body["error"]["params"]["resource"], "definition");
    assert_eq!(body["error"]["params"]["id"], "md-does-not-exist");

    // Core validation error: its key and parameters.
    let (status, body) = json(
        api.post(
            "/definitions",
            json!({"alias": "bad alias!", "base": {"kind": "hf", "repo": "a/b", "file": "c.gguf"}}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 422);
    assert_error(&body, "definitions.invalid_alias");
    assert_eq!(body["error"]["params"]["alias"], "bad alias!");

    // Core error keys keep their code.
    let (status, body) = json(
        api.post("/load", json!({"alias": "no-such-alias-xyz"}))
            .await,
    )
    .await;
    assert_eq!(status, 404);
    assert_error(&body, "definitions.alias_not_found");
    let (status, body) = json(api.post("/load", json!({})).await).await;
    assert_eq!(status, 400);
    assert_error(&body, "api.definition_or_alias_required");
    let (status, body) = json(api.get("/downloads/424242").await).await;
    assert_eq!(status, 404);
    assert_error(&body, "api.not_found");
    let (status, _) = json(api.get("/servers/no-such-server").await).await;
    assert_eq!(status, 404);
    let (status, body) = json(api.get("/no/such/route").await).await;
    assert_eq!(status, 404);
    assert_error(&body, "api.no_route");
    let (status, body) = json(api.get("/servers/..%2Fx/metrics").await).await;
    assert_eq!(status, 422, "{body}");
    let (status, body) = json(
        api.send(
            Method::DELETE,
            "/models/C%3A%5Cnope%5Cmissing.gguf",
            &api.token,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(status, 404);
    assert_error(&body, "models.not_found");
    let (status, body) = json(api.post("/benchmarks/nope/cancel", json!({})).await).await;
    assert_eq!(status, 404);
    assert_error(&body, "api.not_found");
    // Malformed JSON is rejected by the extractor.
    let resp = api
        .http
        .post(api.url("/definitions"))
        .bearer_auth(&api.token)
        .header("content-type", "application/json")
        .body("{not json")
        .send()
        .await
        .unwrap();
    let (status, body) = json(resp).await;
    assert_eq!(status, 400);
    assert_error(&body, "api.invalid_body");
    let (status, body) = json(api.get("/downloads/not-a-number").await).await;
    assert_eq!(status, 400);
    assert_error(&body, "api.invalid_path");
    let (status, body) = json(api.get("/servers/default/logs?since=abc").await).await;
    assert_eq!(status, 400);
    assert_error(&body, "api.invalid_query");
    api.server.stop().await;
}

async fn crud_definition_and_preset(api: &Api) {
    let alias = format!("api-test-{}", std::process::id());
    let (status, def) = json(
        api.post(
            "/definitions",
            json!({"id": "ignored", "alias": alias, "base": {"kind": "hf", "repo": "owner/repo", "file": "m.gguf"},
                   "load": {"context_size": 4096}}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 201, "{def}");
    let id = def["id"].as_str().unwrap().to_string();
    assert_ne!(id, "ignored");
    let (status, got) = json(api.get(&format!("/definitions/{id}")).await).await;
    assert_eq!(status, 200);
    assert_eq!(got["alias"], alias.as_str());
    assert_eq!(got["load"]["context_size"], 4096);

    let mut changed = got.clone();
    changed["description"] = json!("from the API");
    let (status, put) = json(
        api.send(
            Method::PUT,
            &format!("/definitions/{id}"),
            &api.token,
            Some(changed),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200, "{put}");
    assert_eq!(put["description"], "from the API");

    let (status, copy) = json(
        api.post(
            &format!("/definitions/{id}/duplicate"),
            json!({"alias": format!("{alias}-copy")}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 201, "{copy}");
    let copy_id = copy["id"].as_str().unwrap().to_string();
    let (_, list) = json(api.get("/definitions").await).await;
    assert!(list
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["id"] == copy_id.as_str()));
    // The same file the GUI reads (Domain in the data root).
    let on_disk = harness_core::domain::store::Domain::global()
        .definition(&copy_id)
        .unwrap();
    assert_eq!(on_disk.alias, format!("{alias}-copy"));

    for d in [&id, &copy_id] {
        assert_eq!(
            api.send(
                Method::DELETE,
                &format!("/definitions/{d}"),
                &api.token,
                None
            )
            .await
            .status(),
            204
        );
    }
    assert_eq!(api.get(&format!("/definitions/{id}")).await.status(), 404);

    // Presets: built-ins first and read-only.
    let (_, presets) = json(api.get("/presets").await).await;
    let builtin = presets
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["builtin"] == true)
        .unwrap()
        .clone();
    let bid = builtin["id"].as_str().unwrap();
    let (status, _) = json(api.get(&format!("/presets/{bid}")).await).await;
    assert_eq!(status, 200);
    let (status, body) = json(
        api.send(
            Method::PUT,
            &format!("/presets/{bid}"),
            &api.token,
            Some(builtin.clone()),
        )
        .await,
    )
    .await;
    assert_eq!(status, 422, "{body}");
    let (status, p) = json(
        api.post(
            "/presets",
            json!({"id": "", "name": "API preset", "sampling": {"temperature": 0.3}}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 201, "{p}");
    let pid = p["id"].as_str().unwrap().to_string();
    assert_eq!(p["sampling"]["temperature"], 0.3);
    assert_eq!(
        api.send(Method::DELETE, &format!("/presets/{pid}"), &api.token, None)
            .await
            .status(),
        204
    );
}

#[tokio::test]
async fn definitions_and_presets_round_trip() {
    let api = start().await;
    crud_definition_and_preset(&api).await;
    api.server.stop().await;
}

#[tokio::test]
async fn server_profiles_round_trip() {
    let api = start().await;
    let name = format!("API profile {}", std::process::id());
    let port = common::free_port();
    let (status, created) = json(
        api.post(
            "/servers",
            json!({"id": "", "name": name, "config": {"model_path": "C:/models/none.gguf",
                   "gpu_layers": 0, "context_size": 2048, "host": "127.0.0.1", "port": port}}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 201, "{created}");
    let id = created["id"].as_str().unwrap().to_string();
    assert!(id.starts_with("m-api-profile"), "{id}");
    assert_eq!(created["is_profile"], true);
    assert_eq!(created["status"], "Stopped");
    assert_eq!(created["profile"]["config"]["port"], port);

    let (_, list) = json(api.get("/servers").await).await;
    let ids: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    assert!(
        ids.contains(&"default") && ids.contains(&id.as_str()),
        "{ids:?}"
    );

    // Same id again -> 409.
    let (status, body) = json(
        api.post(
            "/servers",
            json!({"id": id, "name": "dup", "config": {"model_path": "x.gguf", "gpu_layers": 0,
                   "context_size": 2048, "host": "127.0.0.1", "port": port}}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 409);
    assert_error(&body, "servers.exists");

    let mut profile = created["profile"].clone();
    profile["description"] = json!("edited");
    let (status, put) = json(
        api.send(
            Method::PUT,
            &format!("/servers/{id}"),
            &api.token,
            Some(profile),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200, "{put}");
    assert_eq!(put["profile"]["description"], "edited");
    assert!(harness_core::servers::load_profiles()
        .iter()
        .any(|p| p.id == id));

    assert_eq!(
        api.send(Method::DELETE, &format!("/servers/{id}"), &api.token, None)
            .await
            .status(),
        204
    );
    assert_eq!(api.get(&format!("/servers/{id}")).await.status(), 404);
    api.server.stop().await;
}

/// A network bind without a key is refused through the same `prepare_launch` path as the GUI
/// (409 + i18n code); "open anyway" is admin-only. Needs a real llama-server binary to resolve
/// the runtime (it is not started: the refusal comes first).
#[tokio::test]
async fn network_rules_apply_to_api_starts() {
    if common::llama_server().is_none() {
        eprintln!("HS_LLAMA_SERVER not set, skipping");
        return;
    }
    let api = start().await;
    let id = format!("api-lan-{}", std::process::id());
    let (status, body) = json(
        api.post(
            "/servers",
            json!({"id": id, "name": "LAN", "config": {"model_path": "C:/models/none.gguf",
                   "gpu_layers": 0, "context_size": 2048, "host": "0.0.0.0",
                   "port": common::free_port()}}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 201, "{body}");
    let (status, body) = json(api.post(&format!("/servers/{id}/start"), json!({})).await).await;
    assert_eq!(status, 409, "{body}");
    assert_error(&body, "network.lan_without_key");
    assert!(api.ctx.registry.get(&id).is_none_or(|pm| !pm.has_child()));

    // A read-only token cannot confirm "open anyway".
    let (_, reader) = json(
        api.post(
            "/tokens",
            json!({"name": "r", "scopes": ["read", "servers:write"]}),
        )
        .await,
    )
    .await;
    let reader = reader["token"].as_str().unwrap().to_string();
    let resp = api
        .send(
            Method::POST,
            &format!("/servers/{id}/exposure-ack"),
            &reader,
            Some(json!({"host": "0.0.0.0"})),
        )
        .await;
    assert_eq!(resp.status(), 403);
    assert!(!harness_core::config::AppSettings::load()
        .network
        .exposure_acks
        .contains_key(&id));
    // Admin can; the confirmation is the same record the GUI writes.
    let resp = api
        .send(
            Method::POST,
            &format!("/servers/{id}/exposure-ack"),
            &api.token,
            Some(json!({"host": "0.0.0.0"})),
        )
        .await;
    assert_eq!(resp.status(), 204);
    assert!(harness_core::config::AppSettings::load()
        .network
        .exposure_acks
        .contains_key(&id));
    harness_core::network::revoke(&id).unwrap();
    assert_eq!(
        api.send(Method::DELETE, &format!("/servers/{id}"), &api.token, None)
            .await
            .status(),
        204
    );
    api.server.stop().await;
}

/// `GET /events` delivers the status changes of a real start that fails (the model file does
/// not exist): `Starting`, then `Error`. Needs the real llama-server binary path.
#[tokio::test]
async fn events_stream_delivers_server_status_changes() {
    if common::llama_server().is_none() {
        eprintln!("HS_LLAMA_SERVER not set, skipping");
        return;
    }
    let api = start().await;
    let id = format!("api-ev-{}", std::process::id());
    let port = common::free_port();
    let missing = api.dir.path().join("missing-model.gguf");
    let (status, body) = json(
        api.post(
            "/servers",
            json!({"id": id, "name": "events", "config": {"model_path": missing,
                   "gpu_layers": 0, "context_size": 1024, "host": "127.0.0.1", "port": port}}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 201, "{body}");

    let stream = api
        .http
        .get(api.url("/events?types=server_status"))
        .bearer_auth(&api.token)
        .send()
        .await
        .unwrap();
    assert_eq!(stream.status(), 200);
    assert_eq!(stream.headers()["content-type"], "text/event-stream");
    let reader = tokio::spawn({
        let id = id.clone();
        common::read_sse(
            stream,
            std::time::Duration::from_secs(20),
            move |event, data| {
                event == "server_status"
                    && data["server_id"] == id.as_str()
                    && data["status"].get("Error").is_some()
            },
        )
    });
    // Let the subscription register before the start.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let (status, body) = json(api.post(&format!("/servers/{id}/start"), json!({})).await).await;
    // The start itself fails synchronously (model missing). "Not found" keys map to 404
    // (`error_key::http_status`; the CLI turns it into exit 3), not 422.
    assert_eq!(status, 404, "{body}");
    assert_error(&body, "process.model_not_found");
    let events = reader.await.unwrap();
    let ours: Vec<&serde_json::Value> = events
        .iter()
        .filter(|(e, d)| e == "server_status" && d["server_id"] == id.as_str())
        .map(|(_, d)| &d["status"])
        .collect();
    assert_eq!(
        ours.first().copied(),
        Some(&json!("Starting")),
        "{events:?}"
    );
    assert!(ours.last().unwrap().get("Error").is_some(), "{events:?}");
    assert!(
        events.iter().all(|(e, _)| e == "server_status"),
        "filter applies"
    );
    api.send(Method::DELETE, &format!("/servers/{id}"), &api.token, None)
        .await;
    api.server.stop().await;
}

#[tokio::test]
async fn shutdown_is_admin_only_and_needs_a_headless_owner() {
    // Without a hook (the GUI): 409.
    let api = start().await;
    let (status, body) = json(api.post("/shutdown", json!({})).await).await;
    assert_eq!(status, 409, "{body}");
    assert_error(&body, "api.shutdown_unsupported");
    api.server.stop().await;

    // With a hook (`harness serve`): a read token is refused, the session token notifies.
    let dir = tempfile::tempdir().unwrap();
    let ctx = common::context(dir.path());
    let server = harness_api::ApiServer::new();
    let hook = std::sync::Arc::new(tokio::sync::Notify::new());
    server.set_shutdown_hook(Some(hook.clone()));
    let (listener, port) = common::listener().await;
    let status = server
        .start_on(ctx, &common::settings(port), listener)
        .await
        .unwrap();
    let base = status.base_url.unwrap();
    let token = server.session_token().await.unwrap();
    let http = reqwest::Client::new();
    let created: serde_json::Value = http
        .post(format!("{base}/tokens"))
        .bearer_auth(&token)
        .json(&json!({"name": "r", "scopes": ["read"]}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let reader = created["token"].as_str().unwrap();
    let resp = http
        .post(format!("{base}/shutdown"))
        .bearer_auth(reader)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403);
    let notified = hook.notified();
    let resp = http
        .post(format!("{base}/shutdown"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 202);
    tokio::time::timeout(std::time::Duration::from_secs(2), notified)
        .await
        .expect("hook notified");
    server.stop().await;
}

/// Ollama Modelfile import over HTTP: preview saves nothing, import creates the definition
/// and its preset, a taken alias is refused, and the scope is `models:write`.
#[tokio::test]
async fn modelfile_preview_and_import() {
    let api = start().await;
    let text = "FROM ./no-such-model.gguf
PARAMETER temperature 0.4
PARAMETER stop \"</s>\"
PARAMETER bogus 1
SYSTEM \"\"\"Be brief.\"\"\"
MESSAGE user hi
";
    let alias = format!("mf-api-{}", std::process::id());
    let (status, p) = json(
        api.post("/modelfile/preview", json!({"text": text, "alias": alias}))
            .await,
    )
    .await;
    assert_eq!(status, 200, "{p}");
    assert_eq!(p["import"]["suggested_alias"], alias.as_str());
    assert_eq!(p["import"]["preset"]["sampling"]["stop"], json!(["</s>"]));
    let codes: Vec<&str> = p["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["code"].as_str().unwrap())
        .collect();
    assert_eq!(
        codes,
        [
            "model_file_missing",
            "unknown_parameter",
            "messages_as_examples"
        ]
    );
    let (_, defs) = json(api.get("/definitions").await).await;
    assert!(defs
        .as_array()
        .unwrap()
        .iter()
        .all(|d| d["alias"] != alias.as_str()));

    let (status, c) = json(
        api.post("/modelfile/import", json!({"text": text, "alias": alias}))
            .await,
    )
    .await;
    assert_eq!(status, 201, "{c}");
    let preset_id = c["definition"]["default_preset_id"]
        .as_str()
        .unwrap()
        .to_string();
    let (_, preset) = json(api.get(&format!("/presets/{preset_id}")).await).await;
    // SYSTEM plus the MESSAGE turn as an example conversation.
    let sys = preset["system_prompt"].as_str().unwrap();
    assert!(
        sys.starts_with("Be brief.\n\nExample conversation"),
        "{sys}"
    );
    assert!(sys.ends_with("User: hi"), "{sys}");
    assert_eq!(preset["sampling"]["temperature"], 0.4);
    let (status, dup) = json(
        api.post("/modelfile/import", json!({"text": text, "alias": alias}))
            .await,
    )
    .await;
    assert_eq!(status, 422, "{dup}");
    let (status, bad) = json(
        api.post("/modelfile/preview", json!({"text": "PARAMETER x 1"}))
            .await,
    )
    .await;
    assert_eq!(status, 422, "{bad}");

    let (_, created) = json(
        api.post("/tokens", json!({"name": "mf-reader", "scopes": ["read"]}))
            .await,
    )
    .await;
    let reader = created["token"].as_str().unwrap().to_string();
    let resp = api
        .send(
            Method::POST,
            "/modelfile/import",
            &reader,
            Some(json!({"text": text})),
        )
        .await;
    assert_eq!(resp.status(), 403);
    api.server.stop().await;
}

/// Runtime registry routes: `read` lists, `admin` changes; errors are keyed. With
/// `HS_LLAMA_SERVER` set, its folder is registered in place (read-only), listed, verified and
/// removed again (the folder stays).
#[tokio::test]
async fn runtime_registry_routes() {
    let api = start().await;
    let (status, list) = json(api.get("/runtimes").await).await;
    assert_eq!(status, 200, "{list}");
    assert_eq!(list[0]["id"], "official");
    assert_eq!(list[0]["kind"], "official");

    // A reader can list but not add.
    let (_, created) = json(
        api.post(
            "/tokens",
            json!({"name": "rt-reader", "scopes": ["read", "servers:write", "models:write"]}),
        )
        .await,
    )
    .await;
    let reader = created["token"].as_str().unwrap().to_string();
    let resp = api.send(Method::GET, "/runtimes", &reader, None).await;
    assert_eq!(resp.status(), 200);
    let (status, body) = json(
        api.send(
            Method::POST,
            "/runtimes",
            &reader,
            Some(json!({"kind": "local", "path": "C:/nope", "name": "x"})),
        )
        .await,
    )
    .await;
    assert_eq!(status, 403);
    assert_eq!(body["error"]["params"]["scope"], "admin");
    let (status, _) = json(api.send(Method::DELETE, "/runtimes/x", &reader, None).await).await;
    assert_eq!(status, 403);

    // Keyed errors.
    let empty = std::env::temp_dir().join(format!(
        "{}rt-empty",
        harness_core::paths::test_dir_prefix("api")
    ));
    std::fs::create_dir_all(&empty).unwrap();
    let (status, body) = json(
        api.post(
            "/runtimes",
            json!({"kind": "local", "path": empty, "name": "empty"}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 422, "{body}");
    assert_error(&body, "runtime.no_server_binary");
    let (status, body) = json(api.get("/runtimes/nope").await).await;
    assert_eq!(status, 404, "{body}");
    assert_error(&body, "runtime.not_found");
    let (status, body) = json(
        api.send(Method::DELETE, "/runtimes/official", &api.token, None)
            .await,
    )
    .await;
    assert_eq!(status, 422, "{body}");
    assert_error(&body, "runtime.official_fixed");
    let (status, body) = json(api.get("/runtimes/release?repo=not%20a%20repo").await).await;
    assert_eq!(status, 422, "{body}");
    assert_error(&body, "runtime.bad_repo");
    let _ = std::fs::remove_dir_all(&empty);

    if let Ok(bin) = std::env::var("HS_LLAMA_SERVER") {
        let dir = std::path::Path::new(&bin).parent().unwrap().to_path_buf();
        let (status, entry) = json(
            api.post(
                "/runtimes",
                json!({"kind": "local", "path": dir, "name": "API test build", "id": "api-test"}),
            )
            .await,
        )
        .await;
        assert_eq!(status, 201, "{entry}");
        assert_eq!(entry["trust"], "local_files");
        assert!(entry["build"].as_u64().is_some(), "{entry}");
        let (status, view) = json(api.get("/runtimes/api-test").await).await;
        assert_eq!(status, 200, "{view}");
        assert_eq!(view["available"], true, "{view}");
        assert!(view["flag_count"].as_u64().unwrap() > 50, "{view}");
        let (status, report) = json(api.post("/runtimes/api-test/verify", json!({})).await).await;
        assert_eq!(status, 200, "{report}");
        assert_eq!(report["ok"], true, "{report}");
        let (status, body) = json(
            api.post(
                "/runtimes",
                json!({"kind": "local", "path": dir, "name": "dup", "id": "api-test"}),
            )
            .await,
        )
        .await;
        assert_eq!(status, 409, "{body}");
        assert_error(&body, "runtime.id_taken");
        let resp = api
            .send(Method::DELETE, "/runtimes/api-test", &api.token, None)
            .await;
        assert_eq!(resp.status(), 204);
        assert!(std::path::Path::new(&bin).is_file(), "in-place folder kept");
    }
    api.server.stop().await;
}

/// `(token, id)` of a new named token.
async fn named(api: &Api, name: &str, scopes: serde_json::Value) -> (String, String) {
    let (status, body) = json(
        api.post("/tokens", json!({"name": name, "scopes": scopes}))
            .await,
    )
    .await;
    assert_eq!(status, 201, "{body}");
    (
        body["token"].as_str().unwrap().to_string(),
        body["info"]["id"].as_str().unwrap().to_string(),
    )
}

/// The narrower scopes: `servers:control` starts/stops but cannot edit servers,
/// `downloads:write` manages downloads but cannot delete models; the wider scopes imply
/// them. PATCH changes scopes and the gateway policy in place, keeping the token value.
#[tokio::test]
async fn narrow_scopes_and_token_update() {
    let api = start().await;
    let admin = api.token.clone();

    let (control, control_id) = named(&api, "control", json!(["servers:control"])).await;
    // Reaches the handlers (they refuse the unknown server themselves), not 403.
    for path in ["/servers/nope/start", "/servers/nope/stop"] {
        let (status, body) = json(api.send(Method::POST, path, &control, None).await).await;
        assert!(![401, 403].contains(&status), "{path}: {status} {body}");
        assert!(status == 404 || status == 422, "{path}: {status} {body}");
    }
    let (status, body) = json(
        api.send(Method::DELETE, "/servers/nope", &control, None)
            .await,
    )
    .await;
    assert_eq!(status, 403);
    assert_error(&body, "api.insufficient_scope");
    assert_eq!(body["error"]["params"]["scope"], "servers:write");
    // Implies read.
    assert_eq!(
        api.send(Method::GET, "/servers", &control, None)
            .await
            .status(),
        200
    );

    let (dl, _) = named(&api, "downloads", json!(["downloads:write"])).await;
    let resp = api
        .send(Method::POST, "/downloads/999999/pause", &dl, None)
        .await;
    assert_eq!(resp.status(), 404);
    let (status, body) = json(api.send(Method::DELETE, "/models/nope", &dl, None).await).await;
    assert_eq!(status, 403);
    assert_eq!(body["error"]["params"]["scope"], "models:write");
    let (status, body) = json(
        api.send(Method::POST, "/servers/nope/start", &dl, None)
            .await,
    )
    .await;
    assert_eq!(status, 403);
    assert_eq!(body["error"]["params"]["scope"], "servers:control");

    // models:write implies downloads:write.
    let (models, _) = named(&api, "models", json!(["models:write"])).await;
    let resp = api
        .send(Method::POST, "/downloads/999999/resume", &models, None)
        .await;
    assert_eq!(resp.status(), 404);

    // Create with a policy, read it back (canonical form).
    let (status, created) = json(
        api.post(
            "/tokens",
            json!({
                "name": "agent",
                "scopes": ["inference"],
                "models": ["qwen3-*", "QWEN3-*"],
                "endpoints": ["models", "chat", "chat"],
                "limits": {"requests_per_minute": 30, "max_concurrent": 2}
            }),
        )
        .await,
    )
    .await;
    assert_eq!(status, 201, "{created}");
    let agent_id = created["info"]["id"].as_str().unwrap().to_string();
    let (status, info) = json(api.get(&format!("/tokens/{agent_id}")).await).await;
    assert_eq!(status, 200);
    assert_eq!(info["models"], json!(["qwen3-*"]), "{info}");
    assert_eq!(info["endpoints"], json!(["chat", "models"]), "{info}");
    assert_eq!(info["limits"]["requests_per_minute"], 30);
    assert!(info["limits"]["tokens_per_minute"].is_null());
    assert_eq!(info["limits"]["max_concurrent"], 2);

    // PATCH: widen the control token; the same value now passes the servers:write check.
    let (status, patched) = json(
        api.send(
            Method::PATCH,
            &format!("/tokens/{control_id}"),
            &admin,
            Some(json!({"name": "ops", "scopes": ["servers:write"]})),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200, "{patched}");
    assert_eq!(patched["name"], "ops");
    assert_eq!(patched["scopes"], json!(["servers:write"]));
    let resp = api
        .send(Method::DELETE, "/servers/nope", &control, None)
        .await;
    assert_eq!(resp.status(), 404);

    // PATCH the policy only: limits cleared, models replaced, the rest untouched.
    let (status, patched) = json(
        api.send(
            Method::PATCH,
            &format!("/tokens/{agent_id}"),
            &admin,
            Some(json!({"models": ["llama-*"], "limits": {}})),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200, "{patched}");
    assert_eq!(patched["models"], json!(["llama-*"]));
    assert_eq!(patched["endpoints"], json!(["chat", "models"]));
    assert_eq!(patched["scopes"], json!(["inference"]));
    assert!(patched["limits"]["requests_per_minute"].is_null());
    assert!(patched["limits"]["max_concurrent"].is_null());

    // Invalid policy values are refused with their own keys.
    for (change, key) in [
        (
            json!({"limits": {"requests_per_minute": 0}}),
            "api.token_limit_invalid",
        ),
        (
            json!({"models": ["has space"]}),
            "api.token_model_pattern_invalid",
        ),
    ] {
        let (status, body) = json(
            api.send(
                Method::PATCH,
                &format!("/tokens/{agent_id}"),
                &admin,
                Some(change),
            )
            .await,
        )
        .await;
        assert_eq!(status, 400, "{body}");
        assert_error(&body, key);
    }

    // Only admin edits tokens; unknown ids are 404.
    let resp = api
        .send(
            Method::PATCH,
            &format!("/tokens/{agent_id}"),
            &control,
            Some(json!({"name": "x"})),
        )
        .await;
    assert_eq!(resp.status(), 403);
    let (status, body) = json(
        api.send(
            Method::PATCH,
            "/tokens/nope",
            &admin,
            Some(json!({"name": "x"})),
        )
        .await,
    )
    .await;
    assert_eq!(status, 404);
    assert_error(&body, "api.token_not_found");

    // The stored file is at the current schema version.
    let file: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(api.dir.path().join("config").join("api_tokens.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(file["schema_version"], 2, "{file}");
    api.server.stop().await;
}

/// n8n connection over the API: everything needs `admin`; the key is stored (never returned),
/// the test and the workflow list reach a local mock of n8n's public API, and a run needs
/// an explicit confirmation.
#[tokio::test]
async fn n8n_connection_workflows_and_confirmed_trigger() {
    let api = start().await;
    let admin = api.token.clone();
    let (reader, _) = named(&api, "n8n-reader", json!(["read"])).await;

    let (status, body) = json(
        api.send(Method::GET, "/automations/n8n", &reader, None)
            .await,
    )
    .await;
    assert_eq!(status, 403);
    assert_eq!(body["error"]["params"]["scope"], "admin");

    let app = axum::Router::new()
        .route(
            "/api/v1/workflows",
            axum::routing::get(|headers: axum::http::HeaderMap| async move {
                if headers.get("x-n8n-api-key").and_then(|v| v.to_str().ok()) != Some("n8n-key") {
                    return (axum::http::StatusCode::UNAUTHORIZED, String::new());
                }
                (
                    axum::http::StatusCode::OK,
                    json!({"data": [{"id": "w1", "name": "Hello", "active": true, "nodes": []}]})
                        .to_string(),
                )
            }),
        )
        .route(
            "/api/v1/workflows/w1",
            axum::routing::get(|| async {
                json!({"id": "w1", "name": "Hello", "active": true, "nodes": [
                    {"type": "n8n-nodes-base.webhook", "parameters": {"path": "hello", "httpMethod": "POST"}}
                ]})
                .to_string()
            }),
        )
        .route(
            "/webhook/hello",
            axum::routing::post(|| async { json!({"ok": true}).to_string() }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let n8n = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let (status, body) = json(
        api.send(
            Method::PUT,
            "/automations/n8n",
            &admin,
            Some(json!({"url": "not a url"})),
        )
        .await,
    )
    .await;
    assert_eq!(status, 400);
    assert_error(&body, "automations.n8n_url_invalid");

    let (status, view) = json(
        api.send(
            Method::PUT,
            "/automations/n8n",
            &admin,
            Some(json!({"url": n8n, "api_key": "n8n-key"})),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200, "{view}");
    assert_eq!(
        view,
        json!({"url": n8n, "key_set": true, "permission": "read_only"})
    );

    let (status, test) = json(
        api.send(
            Method::POST,
            "/automations/n8n/test",
            &admin,
            Some(json!({})),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200, "{test}");
    assert_eq!(test["ok"], true);
    let (status, body) = json(
        api.send(
            Method::POST,
            "/automations/n8n/test",
            &admin,
            Some(json!({"api_key": "wrong"})),
        )
        .await,
    )
    .await;
    assert_eq!(status, 502, "{body}");
    assert_error(&body, "automations.n8n_unauthorized");

    let (status, list) = json(
        api.send(Method::GET, "/automations/n8n/workflows", &admin, None)
            .await,
    )
    .await;
    assert_eq!(status, 200, "{list}");
    assert_eq!(list[0]["name"], "Hello");
    assert_eq!(list[0]["editor_url"], format!("{n8n}/workflow/w1"));

    let trigger = "/automations/n8n/workflows/w1/trigger";
    let (status, body) = json(
        api.send(
            Method::POST,
            trigger,
            &admin,
            Some(json!({"confirm": false})),
        )
        .await,
    )
    .await;
    assert_eq!(status, 400);
    assert_error(&body, "automations.n8n_confirm_required");
    let (status, run) = json(
        api.send(
            Method::POST,
            trigger,
            &admin,
            Some(json!({"confirm": true, "payload": {"a": 1}})),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200, "{run}");
    assert_eq!(run["ok"], true);

    // Disconnecting removes the key.
    let (_, view) = json(
        api.send(
            Method::PUT,
            "/automations/n8n",
            &admin,
            Some(json!({"url": ""})),
        )
        .await,
    )
    .await;
    assert_eq!(
        view,
        json!({"url": "", "key_set": false, "permission": "read_only"})
    );
    api.server.stop().await;
}

/// OTel settings over the API: reading needs `read`, changing and testing `admin`; bad
/// values are 400; "Test export" really posts to a local HTTP receiver and reports its 401
/// and a closed port as failures. Export stays disabled, so no exporter runs here.
#[tokio::test]
async fn otel_settings_and_test_export() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    let api = start().await;
    let admin = api.token.clone();
    let (reader, _) = named(&api, "otel-reader", json!(["read"])).await;

    let (status, view) = json(api.send(Method::GET, "/otel", &reader, None).await).await;
    assert_eq!(status, 200, "{view}");
    assert_eq!(view["settings"]["enabled"], false);
    assert_eq!(view["status"]["active"], false);
    assert_eq!(view["status"]["built"], true);
    // OTLP/gRPC only in builds with `otel-grpc`; the UI and CLI read this flag.
    assert_eq!(
        view["status"]["grpc"],
        harness_core::otel::grpc_built(),
        "{view}"
    );

    let (status, body) = json(
        api.send(Method::POST, "/otel/test", &reader, Some(json!({})))
            .await,
    )
    .await;
    assert_eq!(status, 403);
    assert_eq!(body["error"]["params"]["scope"], "admin");

    let mut settings = view["settings"].clone();
    settings["endpoint"] = json!("ftp://collector");
    let (status, body) = json(
        api.send(
            Method::PUT,
            "/otel",
            &admin,
            Some(json!({"settings": settings})),
        )
        .await,
    )
    .await;
    assert_eq!(status, 400);
    assert_error(&body, "otel.endpoint_invalid");

    if !harness_core::otel::grpc_built() {
        // Choosing gRPC in a build without it: a keyed 400, for saving and for testing.
        let mut grpc = view["settings"].clone();
        grpc["protocol"] = json!("grpc");
        let (status, body) = json(
            api.send(
                Method::PUT,
                "/otel",
                &admin,
                Some(json!({"settings": grpc.clone()})),
            )
            .await,
        )
        .await;
        assert_eq!(status, 400, "{body}");
        assert_error(&body, "otel.grpc_not_built");
        let (status, body) = json(
            api.send(
                Method::POST,
                "/otel/test",
                &admin,
                Some(json!({"settings": grpc})),
            )
            .await,
        )
        .await;
        assert_eq!(status, 400, "{body}");
        assert_error(&body, "otel.grpc_not_built");
    }

    // A real receiver: counts trace posts that carry the right header, 401 otherwise.
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    let app = axum::Router::new().route(
        "/v1/traces",
        axum::routing::post(
            move |headers: axum::http::HeaderMap, body: axum::body::Bytes| {
                let counter = counter.clone();
                async move {
                    if headers.get("x-otel-key").map(|v| v.as_bytes()) != Some(&b"s3cret"[..]) {
                        return axum::http::StatusCode::UNAUTHORIZED;
                    }
                    assert!(!body.is_empty());
                    counter.fetch_add(1, Ordering::SeqCst);
                    axum::http::StatusCode::OK
                }
            },
        ),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    // Save (still disabled) with a header: only its name comes back.
    settings["endpoint"] = json!(format!("http://127.0.0.1:{port}"));
    let (status, saved) = json(
        api.send(
            Method::PUT,
            "/otel",
            &admin,
            Some(json!({"settings": settings, "headers": "x-otel-key=s3cret"})),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200, "{saved}");
    assert_eq!(saved["settings"]["header_names"], json!(["x-otel-key"]));
    assert!(!saved.to_string().contains("s3cret"));
    assert_eq!(saved["status"]["active"], false);

    // Test with the saved settings and headers.
    let (status, result) = json(
        api.send(Method::POST, "/otel/test", &admin, Some(json!({})))
            .await,
    )
    .await;
    assert_eq!(status, 200, "{result}");
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    assert_eq!(result["trace_id"].as_str().unwrap().len(), 32);

    // Unsaved headers replace the saved ones: a wrong value is the collector's 401.
    let (_, result) = json(
        api.send(
            Method::POST,
            "/otel/test",
            &admin,
            Some(json!({"headers": "x-otel-key=wrong"})),
        )
        .await,
    )
    .await;
    assert_eq!(result["ok"], false, "{result}");
    assert!(
        result["error"].as_str().unwrap().contains("401"),
        "{result}"
    );
    assert_eq!(hits.load(Ordering::SeqCst), 1);

    // Nothing listens there: a reported failure.
    let mut other = settings.clone();
    other["endpoint"] = json!(format!("http://127.0.0.1:{}", common::free_port()));
    let (_, result) = json(
        api.send(
            Method::POST,
            "/otel/test",
            &admin,
            Some(json!({"settings": other})),
        )
        .await,
    )
    .await;
    assert_eq!(result["ok"], false, "{result}");
    assert!(result["error"].as_str().is_some_and(|e| !e.is_empty()));

    // Removing the headers.
    let (_, saved) = json(
        api.send(
            Method::PUT,
            "/otel",
            &admin,
            Some(json!({"settings": settings, "headers": ""})),
        )
        .await,
    )
    .await;
    assert_eq!(saved["settings"]["header_names"], json!([]));
    api.server.stop().await;
}

/// The managed runtime routes: status without an installed build, an empty backup is refused
/// (409, the installed build is never removed for it), and changes need `admin`.
#[tokio::test]
async fn managed_runtime_routes() {
    let api = start().await;
    let (status, body) = json(api.get("/runtime/managed").await).await;
    assert_eq!(status, 200, "{body}");
    assert!(body["backup"].is_null(), "{body}");
    assert!(body.get("installed_version").is_some(), "{body}");
    assert!(body.get("backend").is_some(), "{body}");

    let (status, body) = json(api.post("/runtime/managed/rollback", json!({})).await).await;
    assert_eq!(status, 409, "{body}");
    assert_error(&body, "runtime.no_backup");

    let created: serde_json::Value = api
        .http
        .post(format!("{}/tokens", api.base))
        .bearer_auth(&api.token)
        .json(&json!({"name": "r", "scopes": ["read"]}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let reader = created["token"].as_str().unwrap();
    for path in ["/runtime/managed/install", "/runtime/managed/rollback"] {
        let resp = api
            .http
            .post(format!("{}{}", api.base, path))
            .bearer_auth(reader)
            .json(&json!({"backend": "vulkan"}))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 403, "{path}");
    }
    api.server.stop().await;
}

/// Chats are private: `read` does not open `/conversations`, the `conversations` scope does.
/// Real messages in the real `state.db` of the test data root.
#[tokio::test]
async fn conversations_have_their_own_scope_and_read_back() {
    use harness_core::state::{AppendMessage, NewConversation, VariantInput};
    let api = start().await;
    let db = harness_core::state::shared().unwrap();
    let conv = db
        .create_conversation(&NewConversation {
            title: Some("Vault notes".into()),
            model_alias: Some("api-test-model".into()),
            system_prompt: Some("Be brief.".into()),
            ..Default::default()
        })
        .unwrap();
    let user = db
        .append_message(AppendMessage {
            conversation_id: conv.id.clone(),
            parent_id: None,
            role: "user".into(),
            variant: VariantInput {
                content: "The vault code is 7421.".into(),
                ..Default::default()
            },
            id: None,
            variant_id: None,
            any_parent_variant: false,
        })
        .unwrap();
    db.append_message(AppendMessage {
        conversation_id: conv.id.clone(),
        parent_id: Some(user.id.clone()),
        role: "assistant".into(),
        variant: VariantInput {
            content: "Noted.".into(),
            model_alias: Some("api-test-model".into()),
            ..Default::default()
        },
        id: None,
        variant_id: None,
        any_parent_variant: false,
    })
    .unwrap();

    let (reader, _) = named(&api, "chat-reader", json!(["read"])).await;
    let (chats, _) = named(&api, "chat-owner", json!(["conversations"])).await;
    let path = format!("/conversations/{}", conv.id);
    for p in [
        "/conversations".to_string(),
        path.clone(),
        format!("{path}/export"),
        format!("{path}/documents"),
    ] {
        let (status, body) = json(api.send(Method::GET, &p, &reader, None).await).await;
        assert_eq!(status, 403, "{p}");
        assert_error(&body, "api.insufficient_scope");
        assert_eq!(body["error"]["params"]["scope"], "conversations");
    }
    let (status, _) = json(
        api.send(Method::POST, &format!("{path}/compact"), &reader, None)
            .await,
    )
    .await;
    assert_eq!(status, 403);
    // The conversations scope does not read anything else.
    let (status, _) = json(api.send(Method::GET, "/models", &chats, None).await).await;
    assert_eq!(status, 403);

    // List (with a search) and read.
    let (status, list) = json(
        api.send(Method::GET, "/conversations?limit=10", &chats, None)
            .await,
    )
    .await;
    assert_eq!(status, 200, "{list}");
    assert!(list
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["id"] == conv.id.as_str() && c["title"] == "Vault notes"));
    let (_, found) = json(
        api.send(Method::GET, "/conversations?query=7421", &chats, None)
            .await,
    )
    .await;
    assert!(
        found
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == conv.id.as_str()),
        "{found}"
    );
    let (status, view) = json(api.send(Method::GET, &path, &chats, None).await).await;
    assert_eq!(status, 200, "{view}");
    assert_eq!(view["conversation"]["system_prompt"], "Be brief.");
    let msgs = view["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0]["variant"]["content"], "The vault code is 7421.");
    assert_eq!(msgs[1]["role"], "assistant");
    let (status, body) = json(
        api.send(Method::GET, "/conversations/nope", &chats, None)
            .await,
    )
    .await;
    assert_eq!(status, 404, "{body}");

    // Export: JSON (the file the app imports) and Markdown.
    let (status, doc) = json(
        api.send(Method::GET, &format!("{path}/export"), &chats, None)
            .await,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(doc["schema"], "harness.conversation.v1");
    assert_eq!(doc["messages"].as_array().unwrap().len(), 2);
    let resp = api
        .send(
            Method::GET,
            &format!("{path}/export?format=markdown"),
            &chats,
            None,
        )
        .await;
    assert_eq!(resp.status(), 200);
    assert!(resp.headers()["content-type"]
        .to_str()
        .unwrap()
        .starts_with("text/markdown"));
    let md = resp.text().await.unwrap();
    assert!(
        md.contains("# Vault notes") && md.contains("The vault code is 7421."),
        "{md}"
    );
    let (status, body) = json(
        api.send(
            Method::GET,
            &format!("{path}/export?format=docx"),
            &chats,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(status, 400);
    assert_error(&body, "api.invalid_format");

    // Documents: metadata of the conversation's documents (none here).
    let (status, docs) = json(
        api.send(Method::GET, &format!("{path}/documents"), &chats, None)
            .await,
    )
    .await;
    assert_eq!((status, docs), (200, json!([])));

    // Compaction needs the gateway: none runs in this test.
    let (status, body) = json(
        api.send(
            Method::POST,
            &format!("{path}/compact"),
            &chats,
            Some(json!({})),
        )
        .await,
    )
    .await;
    assert_eq!(status, 409, "{body}");
    assert_error(&body, "compaction.gateway_required");
    // No model at all: say so before anything else.
    let bare = db.create_conversation(&NewConversation::default()).unwrap();
    let (status, body) = json(
        api.send(
            Method::POST,
            &format!("/conversations/{}/compact", bare.id),
            &chats,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(status, 422, "{body}");
    assert_error(&body, "compaction.no_model");
    // Cancelling when nothing runs is not an error; an unknown marker is a 404.
    let (status, body) = json(
        api.send(
            Method::POST,
            &format!("{path}/compact/cancel"),
            &chats,
            None,
        )
        .await,
    )
    .await;
    assert_eq!((status, body["cancelled"].clone()), (200, json!(false)));
    let (status, body) = json(
        api.send(
            Method::PUT,
            &format!("{path}/compactions/not-a-message"),
            &chats,
            Some(json!({"undone": true})),
        )
        .await,
    )
    .await;
    assert_eq!(status, 404, "{body}");

    // Queue hints are analytics: read is enough.
    let (status, hints) = json(
        api.send(Method::GET, "/analytics/queue-hints", &reader, None)
            .await,
    )
    .await;
    assert_eq!(status, 200, "{hints}");
    assert!(hints.is_array());
    api.server.stop().await;
}

/// `GET /servers/{id}/api-key`: the session token on a loopback API reads a running server's key
/// (never cached); a named token, even an admin one, gets 403 without learning whether the server
/// exists; an unknown id is 404; the key shows up nowhere else (views, errors, logs).
#[tokio::test]
async fn only_the_local_session_reads_a_server_key() {
    let api = start().await;
    let key = format!("k-{}-not-a-real-key", std::process::id());
    api.ctx
        .registry
        .get_or_create("worker")
        .simulate_running_for_tests(harness_core::process::ActiveServerInfo {
            connect_host: "127.0.0.1".into(),
            bind_host: "127.0.0.1".into(),
            port: 1,
            api_key: Some(key.clone()),
            api_key_set: false,
            api_key_required: true,
            model_path: "model".into(),
            router_mode: false,
            exposure_acknowledged: false,
            cors_origins: None,
            serve_mode: Default::default(),
            engine_family: "mlx".into(),
        });

    let resp = api.get("/servers/worker/api-key").await;
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers()
            .get(reqwest::header::CACHE_CONTROL)
            .and_then(|v| v.to_str().ok()),
        Some("no-store")
    );
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["api_key"], key.as_str());

    // A stopped (or never started) server has no key; an unknown one is 404.
    let (status, body) = json(api.get("/servers/default/api-key").await).await;
    assert_eq!(status, 200, "{body}");
    assert!(body["api_key"].is_null());
    let (status, body) = json(api.get("/servers/nope/api-key").await).await;
    assert_eq!(status, 404, "{body}");

    // Named tokens never read it, whatever their scope, and learn nothing about the id.
    let (_, admin) = json(
        api.post(
            "/tokens",
            json!({"name": "remote-admin", "scopes": ["admin"]}),
        )
        .await,
    )
    .await;
    let admin = admin["token"].as_str().unwrap().to_string();
    for id in ["worker", "nope"] {
        let (status, body) = json(
            api.send(Method::GET, &format!("/servers/{id}/api-key"), &admin, None)
                .await,
        )
        .await;
        assert_eq!(status, 403, "{body}");
        assert_error(&body, "api.server_key_local_only");
        assert!(!body.to_string().contains(&key));
    }

    // The key is in no other answer and in no log line.
    let (_, view) = json(api.get("/servers/worker").await).await;
    assert!(!view.to_string().contains(&key), "{view}");
    let (_, logs) = json(api.get("/servers/worker/logs").await).await;
    assert!(!logs.to_string().contains(&key));
}

#[tokio::test]
async fn jobs_exposes_the_shared_queue_with_read_authentication() {
    let api = start().await;
    let label = "api-queue-read-auth".to_string();
    let ticket = harness_core::jobs::global().submit(
        label.clone(),
        harness_core::jobs::Priority::Background,
        12345,
    );
    let (status, body) = json(api.get("/jobs").await).await;
    assert_eq!(status, 200);
    let job = body
        .as_array()
        .unwrap()
        .iter()
        .find(|job| job["label"] == label)
        .unwrap();
    assert_eq!(job["status"], "queued");
    assert_eq!(job["priority"], "background");
    assert_eq!(job["memory_bytes"], 12345);
    let response = api
        .http
        .get(format!("{}/jobs", api.base))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 401);
    drop(ticket);
    api.server.stop().await;
}

#[test]
fn jobs_schema_preserves_module_status() {
    let document = serde_json::to_value(harness_api::openapi_document()).unwrap();
    let schemas = &document["components"]["schemas"];
    assert_eq!(schemas["Status"]["enum"][0], "not_installed");
    assert_eq!(schemas["GpuJobState"]["enum"][0], "queued");
    assert_eq!(schemas["GpuJobPriority"]["enum"][0], "interactive");
    assert_eq!(
        schemas["JobStatus"]["properties"]["status"]["$ref"],
        "#/components/schemas/GpuJobState"
    );
}
