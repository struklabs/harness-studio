use super::*;

use std::sync::{Arc, Mutex};

use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{any, get};
use axum::Router;
use serde_json::json;

const KEY: &str = "test-key-123";

#[derive(Default)]
struct Seen {
    /// `(path, query, key header present and correct)` of every API request.
    api: Vec<(String, String, bool)>,
    /// `(method, path, had the API key header, body)` of every webhook call.
    hooks: Vec<(String, String, bool, String)>,
}

type Shared = Arc<Mutex<Seen>>;

fn json_body(v: serde_json::Value) -> axum::response::Response {
    ([("content-type", "application/json")], v.to_string()).into_response()
}

fn workflow_list() -> serde_json::Value {
    json!({
        "data": [
            {
                "id": "wf1", "name": "Chat bot", "active": true,
                "updatedAt": "2026-10-01T10:00:00.000Z",
                "nodes": [
                    {"type": "n8n-nodes-base.webhook", "name": "Webhook",
                     "parameters": {"path": "/chat-bot/", "httpMethod": "post"}},
                    {"type": "n8n-nodes-base.set", "name": "Set"}
                ]
            },
            {
                "id": "wf2", "name": "Nightly", "active": false,
                "nodes": [
                    {"type": "n8n-nodes-base.scheduleTrigger", "name": "Schedule"},
                    {"type": "n8n-nodes-base.manualTrigger", "name": "Manual", "disabled": true}
                ]
            },
            {
                "id": 7, "name": "Param path", "active": true,
                "nodes": [
                    {"type": "n8n-nodes-base.webhook", "name": "Webhook",
                     "parameters": {"path": "users/:id"}}
                ]
            }
        ],
        "nextCursor": null
    })
}

async fn api(
    seen: &Shared,
    headers: &HeaderMap,
    path: &str,
    query: Option<String>,
) -> Option<axum::response::Response> {
    let key_ok = headers.get("x-n8n-api-key").and_then(|v| v.to_str().ok()) == Some(KEY);
    seen.lock()
        .unwrap()
        .api
        .push((path.to_string(), query.unwrap_or_default(), key_ok));
    if !key_ok {
        return Some(StatusCode::UNAUTHORIZED.into_response());
    }
    None
}

async fn start() -> (String, Shared) {
    let seen: Shared = Arc::default();
    let app = Router::new()
        .route(
            "/api/v1/workflows",
            get(
                |State(s): State<Shared>, h: HeaderMap, RawQuery(q): RawQuery| async move {
                    if let Some(r) = api(&s, &h, "/workflows", q).await {
                        return r;
                    }
                    json_body(workflow_list())
                },
            ),
        )
        .route(
            "/api/v1/workflows/{id}",
            get(
                |State(s): State<Shared>, h: HeaderMap, Path(id): Path<String>| async move {
                    if let Some(r) = api(&s, &h, &format!("/workflows/{id}"), None).await {
                        return r;
                    }
                    let list = workflow_list();
                    match list["data"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|w| w["id"].to_string().trim_matches('"') == id)
                    {
                        Some(w) => json_body(w.clone()),
                        None => StatusCode::NOT_FOUND.into_response(),
                    }
                },
            ),
        )
        .route(
            "/api/v1/executions",
            get(
                |State(s): State<Shared>, h: HeaderMap, RawQuery(q): RawQuery| async move {
                    if let Some(r) = api(&s, &h, "/executions", q).await {
                        return r;
                    }
                    json_body(json!({"data": [
                        {"id": 11, "workflowId": "wf1", "status": "success", "mode": "webhook",
                         "startedAt": "2026-10-01T10:00:00.000Z", "stoppedAt": "2026-10-01T10:00:01.000Z",
                         "finished": true},
                        {"id": "12", "workflowId": "wf1", "status": "error", "mode": "trigger",
                         "startedAt": "2026-10-01T11:00:00.000Z", "stoppedAt": null, "finished": false}
                    ], "nextCursor": null}))
                },
            ),
        )
        .route(
            "/api/v1/executions/{id}",
            get(
                |State(s): State<Shared>,
                 h: HeaderMap,
                 Path(id): Path<String>,
                 RawQuery(q): RawQuery| async move {
                    if let Some(r) = api(&s, &h, &format!("/executions/{id}"), q).await {
                        return r;
                    }
                    json_body(json!({"id": id, "status": "error", "data": {"resultData": {
                        "lastNodeExecuted": "HTTP Request",
                        "error": {"message": "The service refused the connection",
                                  "node": {"name": "HTTP Request"}}
                    }}}))
                },
            ),
        )
        .route(
            "/webhook/{*path}",
            any(
                |State(s): State<Shared>,
                 method: axum::http::Method,
                 h: HeaderMap,
                 Path(path): Path<String>,
                 body: String| async move {
                    s.lock().unwrap().hooks.push((
                        method.to_string(),
                        path,
                        h.contains_key("x-n8n-api-key"),
                        body,
                    ));
                    json_body(json!({"message": "Workflow was started"}))
                },
            ),
        )
        .with_state(seen.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, seen)
}

#[test]
fn urls_are_normalized_and_validated() {
    assert_eq!(
        normalize_url(" https://n8n.example.com/ ").unwrap(),
        "https://n8n.example.com"
    );
    assert_eq!(
        normalize_url("http://127.0.0.1:5678/api/v1/").unwrap(),
        "http://127.0.0.1:5678"
    );
    assert_eq!(
        normalize_url("https://host.example/n8n/home/workflows").unwrap(),
        "https://host.example/n8n"
    );
    for bad in [
        "",
        "n8n.example.com",
        "ftp://n8n.example.com",
        "https://user:pw@n8n.example.com",
        "https://n8n.example.com/?a=1",
        "https://n8n.example.com/#x",
    ] {
        assert_eq!(normalize_url(bad).unwrap_err(), ERR_URL_INVALID, "{bad}");
    }
}

#[test]
fn ids_are_path_safe() {
    assert_eq!(segment("abc-12_X").unwrap(), "abc-12_X");
    for bad in ["", "a/b", "..", "a?b", "a b"] {
        assert!(segment(bad).is_err(), "{bad}");
    }
}

#[tokio::test]
async fn connection_test_sends_the_key_and_reports_clear_errors() {
    let (url, seen) = start().await;
    let ok = N8nClient::new(&format!("{url}/"), KEY)
        .unwrap()
        .test()
        .await
        .unwrap();
    assert!(ok.ok && ok.has_workflows);
    assert_eq!(ok.url, url);
    {
        let seen = seen.lock().unwrap();
        assert_eq!(seen.api[0].0, "/workflows");
        assert_eq!(seen.api[0].1, "limit=1");
        assert!(seen.api[0].2, "key header must be sent");
    }

    let bad = N8nClient::new(&url, "wrong").unwrap().test().await;
    assert_eq!(bad.unwrap_err(), "automations.n8n_unauthorized");

    // A server without the public API (404) and a refused connection.
    let closed = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        format!("http://{}", l.local_addr().unwrap())
    };
    let down = N8nClient::new(&closed, KEY).unwrap().test().await;
    assert!(down.unwrap_err().starts_with("http.connect_failed"));

    assert_eq!(N8nClient::new(&url, "  ").err().unwrap(), ERR_KEY_MISSING);
}

#[test]
fn http_statuses_map_to_keys() {
    assert_eq!(status_error(401), "automations.n8n_unauthorized");
    assert_eq!(status_error(403), "automations.n8n_forbidden");
    assert_eq!(status_error(404), "automations.n8n_api_missing");
    assert_eq!(status_error(302), "automations.n8n_redirect");
    assert!(status_error(502).starts_with("automations.n8n_server_error"));
    assert!(status_error(429).starts_with("automations.n8n_http_error"));
}

#[tokio::test]
async fn workflows_show_state_triggers_and_webhooks() {
    let (url, _) = start().await;
    let list = N8nClient::new(&url, KEY)
        .unwrap()
        .workflows()
        .await
        .unwrap();
    assert_eq!(list.len(), 3);

    let chat = &list[0];
    assert_eq!(chat.name, "Chat bot");
    assert!(chat.active);
    assert_eq!(chat.triggers, vec!["webhook"]);
    let hook = chat.webhook.as_ref().unwrap();
    assert_eq!(hook.method, "POST");
    assert_eq!(hook.url, format!("{url}/webhook/chat-bot"));
    assert_eq!(chat.editor_url, format!("{url}/workflow/wf1"));

    // A disabled manual trigger is not listed; a schedule-only workflow cannot be run here.
    let nightly = &list[1];
    assert!(!nightly.active);
    assert_eq!(nightly.triggers, vec!["scheduleTrigger"]);
    assert!(nightly.webhook.is_none());

    // Numeric ids work; a webhook path with parameters is not a plain run.
    assert_eq!(list[2].id, "7");
    assert_eq!(list[2].triggers, vec!["webhook"]);
    assert!(list[2].webhook.is_none());
}

#[tokio::test]
async fn executions_and_their_errors() {
    let (url, seen) = start().await;
    let client = N8nClient::new(&url, KEY).unwrap();
    let runs = client.executions(Some("wf1"), 500).await.unwrap();
    assert_eq!(runs.len(), 2);
    assert_eq!(
        (runs[0].id.as_str(), runs[0].status.as_str()),
        ("11", "success")
    );
    assert_eq!(runs[1].status, "error");
    assert_eq!(runs[1].stopped_at, None);
    assert_eq!(
        runs[1].editor_url,
        format!("{url}/workflow/wf1/executions/12")
    );
    {
        let seen = seen.lock().unwrap();
        // The limit is capped and the workflow filter is passed on.
        assert_eq!(seen.api[0].1, "limit=100&workflowId=wf1");
    }

    let e = client.execution_error("12").await.unwrap();
    assert_eq!(
        e.message.as_deref(),
        Some("The service refused the connection")
    );
    assert_eq!(e.node.as_deref(), Some("HTTP Request"));
    assert!(seen
        .lock()
        .unwrap()
        .api
        .iter()
        .any(|(p, q, _)| p == "/executions/12" && q == "includeData=true"));

    assert!(client.executions(Some("../x"), 5).await.is_err());
}

#[tokio::test]
async fn trigger_calls_the_webhook_without_the_api_key() {
    let (url, seen) = start().await;
    let client = N8nClient::new(&url, KEY).unwrap();

    let r = client
        .trigger("wf1", Some(&json!({"prompt": "hi"})))
        .await
        .unwrap();
    assert!(r.ok);
    assert_eq!(r.status, 200);
    assert!(r.body.contains("Workflow was started"));
    {
        let seen = seen.lock().unwrap();
        let (method, path, had_key, body) = &seen.hooks[0];
        assert_eq!((method.as_str(), path.as_str()), ("POST", "chat-bot"));
        assert!(!had_key, "the API key must not reach the webhook");
        assert_eq!(body, r#"{"prompt":"hi"}"#);
    }

    // Inactive or webhook-less workflows are refused before any call.
    assert_eq!(
        client.trigger("wf2", None).await.unwrap_err(),
        "automations.n8n_no_webhook"
    );
    assert_eq!(
        client.trigger("7", None).await.unwrap_err(),
        "automations.n8n_no_webhook"
    );
    assert_eq!(seen.lock().unwrap().hooks.len(), 1);
    assert_eq!(
        client.trigger("nope", None).await.unwrap_err(),
        "automations.n8n_api_missing"
    );
}

#[tokio::test]
async fn redirects_are_not_followed() {
    use axum::response::Redirect;
    let hit = Arc::new(Mutex::new(false));
    let h2 = hit.clone();
    let target = Router::new().route(
        "/api/v1/workflows",
        get(move || {
            let h = h2.clone();
            async move {
                *h.lock().unwrap() = true;
                json_body(json!({"data": []}))
            }
        }),
    );
    let tl = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target_url = format!("http://{}", tl.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(tl, target).await.unwrap() });
    let redir = Router::new().route(
        "/api/v1/workflows",
        get(move || {
            let to = format!("{target_url}/api/v1/workflows");
            async move { Redirect::temporary(&to) }
        }),
    );
    let rl = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", rl.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(rl, redir).await.unwrap() });

    let r = N8nClient::new(&url, KEY).unwrap().test().await;
    assert_eq!(r.unwrap_err(), "automations.n8n_redirect");
    assert!(
        !*hit.lock().unwrap(),
        "the redirect target must not be called"
    );
}

#[test]
fn key_goes_to_the_keychain_not_to_settings() {
    save("https://n8n.example.com/", Some("  s3cret-key  ")).unwrap();
    let v = view().unwrap();
    assert_eq!(v.url, "https://n8n.example.com");
    assert!(v.key_set);
    assert_eq!(
        crate::secrets::n8n_key_get().unwrap().as_deref(),
        Some("s3cret-key")
    );

    // The settings file holds the address only.
    let file = std::fs::read_to_string(crate::paths::data_root().join("config/settings.json"))
        .or_else(|_| std::fs::read_to_string(crate::paths::data_root().join("settings.json")))
        .unwrap_or_default();
    assert!(file.contains("n8n.example.com"));
    assert!(!file.contains("s3cret-key"));

    // `None` keeps the key, an invalid address changes nothing.
    save("https://n8n2.example.com", None).unwrap();
    assert!(view().unwrap().key_set);
    assert_eq!(save("not a url", None).unwrap_err(), ERR_URL_INVALID);
    assert_eq!(view().unwrap().url, "https://n8n2.example.com");

    // An empty address disconnects and removes the key.
    let off = save("", None).unwrap();
    assert_eq!(off, N8nView::default());
    assert!(crate::secrets::n8n_key_get().unwrap().is_none());
}
