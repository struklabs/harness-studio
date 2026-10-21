//! Proposals, permissions and the n8n tools against a mocked n8n that keeps workflows and
//! executions in memory and records every call.

use super::proposals::{
    sanitize_workflow, ProposalKind, ProposalStatus, ProposalStore, StageWorkflow,
};
use super::*;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{any, get, post};
use axum::{Json, Router};
use serde_json::{json, Value};

const KEY: &str = "write-key";

#[derive(Default)]
struct Mock {
    workflows: HashMap<String, Value>,
    executions: Vec<Value>,
    /// `METHOD /path` of every API call, in order.
    calls: Vec<String>,
    /// Bodies of POST/PUT workflow calls.
    bodies: Vec<Value>,
    hooks: Vec<(String, bool, String)>,
    next: u32,
    next_exec: u32,
    /// An n8n 1.x: no `/publish` or `/unpublish` routes.
    v1: bool,
    /// Every workflow write is refused with this message.
    refuse: Option<String>,
}

type Shared = Arc<Mutex<Mock>>;

fn authed(h: &HeaderMap) -> bool {
    h.get("x-n8n-api-key").and_then(|v| v.to_str().ok()) == Some(KEY)
}

fn stored(id: &str, body: &Value, active: bool, version: u32) -> Value {
    json!({
        "id": id,
        "name": body["name"],
        "nodes": body["nodes"],
        "connections": body["connections"],
        "settings": body["settings"],
        "active": active,
        "activeVersionId": if active { json!("v-live") } else { Value::Null },
        "updatedAt": format!("2026-10-01T10:00:{version:02}.000Z"),
    })
}

fn check_body(body: &Value) -> Result<(), (StatusCode, Json<Value>)> {
    let allowed = ["name", "nodes", "connections", "settings"];
    if body
        .as_object()
        .is_some_and(|o| o.keys().all(|k| allowed.contains(&k.as_str())))
        && body["nodes"].is_array()
    {
        return Ok(());
    }
    Err((
        StatusCode::BAD_REQUEST,
        Json(json!({"message": "request/body must NOT have additional properties"})),
    ))
}

async fn start() -> (String, Shared) {
    let mock: Shared = Arc::default();
    let app = Router::new()
        .route(
            "/api/v1/workflows",
            get(|State(s): State<Shared>, h: HeaderMap| async move {
                if !authed(&h) {
                    return StatusCode::UNAUTHORIZED.into_response();
                }
                let m = s.lock().unwrap();
                let mut list: Vec<Value> = m.workflows.values().cloned().collect();
                list.sort_by_key(|w| w["id"].as_str().unwrap_or("").to_string());
                Json(json!({"data": list, "nextCursor": null})).into_response()
            })
            .post(
                |State(s): State<Shared>, h: HeaderMap, Json(body): Json<Value>| async move {
                    if !authed(&h) {
                        return StatusCode::UNAUTHORIZED.into_response();
                    }
                    let mut m = s.lock().unwrap();
                    m.calls.push("POST /workflows".into());
                    m.bodies.push(body.clone());
                    if let Some(msg) = m.refuse.clone() {
                        return (StatusCode::BAD_REQUEST, Json(json!({"message": msg})))
                            .into_response();
                    }
                    if let Err(e) = check_body(&body) {
                        return e.into_response();
                    }
                    m.next += 1;
                    let id = format!("wf{}", m.next);
                    let wf = stored(&id, &body, false, 0);
                    m.workflows.insert(id, wf.clone());
                    Json(wf).into_response()
                },
            ),
        )
        .route(
            "/api/v1/workflows/{id}",
            get(
                |State(s): State<Shared>, h: HeaderMap, Path(id): Path<String>| async move {
                    if !authed(&h) {
                        return StatusCode::UNAUTHORIZED.into_response();
                    }
                    match s.lock().unwrap().workflows.get(&id) {
                        Some(w) => Json(w.clone()).into_response(),
                        None => (StatusCode::NOT_FOUND, Json(json!({"message": "Not Found"})))
                            .into_response(),
                    }
                },
            )
            .put(
                |State(s): State<Shared>,
                 h: HeaderMap,
                 Path(id): Path<String>,
                 Json(body): Json<Value>| async move {
                    if !authed(&h) {
                        return StatusCode::UNAUTHORIZED.into_response();
                    }
                    let mut m = s.lock().unwrap();
                    m.calls.push(format!("PUT /workflows/{id}"));
                    m.bodies.push(body.clone());
                    if let Err(e) = check_body(&body) {
                        return e.into_response();
                    }
                    let Some(old) = m.workflows.get(&id).cloned() else {
                        return (StatusCode::NOT_FOUND, Json(json!({"message": "Not Found"})))
                            .into_response();
                    };
                    let active = old["active"].as_bool().unwrap_or(false);
                    let n = m.workflows.len() as u32 + 10 + m.calls.len() as u32;
                    let wf = stored(&id, &body, active, n % 60);
                    m.workflows.insert(id, wf.clone());
                    Json(wf).into_response()
                },
            ),
        )
        .route(
            "/api/v1/workflows/{id}/{action}",
            post(
                |State(s): State<Shared>,
                 h: HeaderMap,
                 Path((id, action)): Path<(String, String)>| async move {
                    if !authed(&h) {
                        return StatusCode::UNAUTHORIZED.into_response();
                    }
                    let mut m = s.lock().unwrap();
                    m.calls.push(format!("POST /workflows/{id}/{action}"));
                    // An n8n 1.x has no publish routes: express' HTML 404.
                    if m.v1 && ["publish", "unpublish"].contains(&action.as_str()) {
                        return (StatusCode::NOT_FOUND, "<pre>Cannot POST</pre>").into_response();
                    }
                    let on = matches!(action.as_str(), "publish" | "activate");
                    match m.workflows.get_mut(&id) {
                        Some(w) => {
                            w["active"] = json!(on);
                            w["activeVersionId"] = if on { json!("v-live") } else { Value::Null };
                            Json(w.clone()).into_response()
                        }
                        None => (StatusCode::NOT_FOUND, Json(json!({"message": "Not Found"})))
                            .into_response(),
                    }
                },
            ),
        )
        .route(
            "/api/v1/executions",
            get(
                |State(s): State<Shared>,
                 h: HeaderMap,
                 RawQuery(raw): RawQuery| async move {
                    if !authed(&h) {
                        return StatusCode::UNAUTHORIZED.into_response();
                    }
                    let q: HashMap<String, String> = raw
                        .unwrap_or_default()
                        .split('&')
                        .filter_map(|kv| kv.split_once('='))
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                        .collect();
                    let m = s.lock().unwrap();
                    let mut list: Vec<Value> = m
                        .executions
                        .iter()
                        .filter(|e| {
                            q.get("workflowId")
                                .is_none_or(|w| e["workflowId"].as_str() == Some(w))
                        })
                        .cloned()
                        .collect();
                    list.reverse();
                    list.truncate(q.get("limit").and_then(|l| l.parse().ok()).unwrap_or(20));
                    Json(json!({"data": list, "nextCursor": null})).into_response()
                },
            ),
        )
        .route(
            "/api/v1/executions/{id}",
            get(
                |State(s): State<Shared>, h: HeaderMap, Path(id): Path<String>| async move {
                    if !authed(&h) {
                        return StatusCode::UNAUTHORIZED.into_response();
                    }
                    match s
                        .lock()
                        .unwrap()
                        .executions
                        .iter()
                        .find(|e| e["id"].to_string().trim_matches('"') == id)
                    {
                        Some(e) => Json(e.clone()).into_response(),
                        None => (StatusCode::NOT_FOUND, Json(json!({"message": "Not Found"})))
                            .into_response(),
                    }
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
                    let mut m = s.lock().unwrap();
                    m.hooks.push((format!("{method} {path}"), h.contains_key("x-n8n-api-key"), body));
                    m.next_exec += 1;
                    let id = 100 + m.next_exec;
                    let wf = m
                        .workflows
                        .values()
                        .find(|w| {
                            w["nodes"].as_array().is_some_and(|n| {
                                n.iter().any(|n| n["parameters"]["path"].as_str() == Some(path.as_str()))
                            })
                        })
                        .map(|w| w["id"].clone())
                        .unwrap_or(json!("?"));
                    m.executions.push(json!({
                        "id": id, "workflowId": wf, "status": "success", "mode": "webhook",
                        "startedAt": "2026-10-01T10:00:00.000Z", "stoppedAt": "2026-10-01T10:00:01.000Z",
                        "finished": true,
                        "data": {"resultData": {
                            "lastNodeExecuted": "Set",
                            "runData": {
                                "Webhook": [{"startTime": 1000, "executionTime": 3, "executionStatus": "success",
                                    "data": {"main": [[{"json": {"hello": "in"}}]]}}],
                                "Set": [{"startTime": 1010, "executionTime": 5, "executionStatus": "success",
                                    "data": {"main": [[{"json": {"hello": "world"}}, {"json": {"hello": "again"}}]]}}]
                            }
                        }}
                    }));
                    Json(json!({"message": "Workflow was started"})).into_response()
                },
            ),
        )
        .with_state(mock.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, mock)
}

fn client(url: &str) -> N8nClient {
    N8nClient::new(url, KEY).unwrap()
}

fn demo(name: &str, path: &str) -> Value {
    json!({
        "name": name,
        "nodes": [
            {"name": "Webhook", "type": "n8n-nodes-base.webhook", "typeVersion": 2, "position": [0, 0],
             "parameters": {"path": path, "httpMethod": "POST"}},
            {"name": "Set", "type": "n8n-nodes-base.set", "typeVersion": 3, "position": [220, 0],
             "parameters": {}}
        ],
        "connections": {"Webhook": {"main": [[{"node": "Set", "type": "main", "index": 0}]]}},
        "settings": {"executionOrder": "v1", "binaryMode": "separate", "bogus": true},
        "active": true, "id": "ignored", "createdAt": "x"
    })
}

fn create(workflow: Value, activate: Option<bool>) -> StageWorkflow {
    StageWorkflow {
        workflow: Some(workflow),
        activate,
        ..Default::default()
    }
}

fn calls(m: &Shared) -> Vec<String> {
    m.lock().unwrap().calls.clone()
}

fn writes(m: &Shared) -> Vec<String> {
    calls(m)
        .into_iter()
        .filter(|c| !c.starts_with("GET"))
        .collect()
}

fn error_key(e: &str) -> String {
    crate::error_key::parse(e)
        .map(|(k, _)| k.to_string())
        .unwrap_or_else(|| e.to_string())
}

#[test]
fn permissions_are_a_ladder() {
    use N8nPermission::*;
    assert_eq!(N8nPermission::default(), ReadOnly);
    assert!(!ReadOnly.can_draft() && !ReadOnly.can_write() && !ReadOnly.can_trigger());
    assert!(Draft.can_draft() && !Draft.can_write() && !Draft.can_trigger());
    assert!(Write.can_draft() && Write.can_write() && !Write.can_trigger());
    assert!(Trigger.can_draft() && Trigger.can_write() && Trigger.can_trigger());
    // Old settings files have no permission: they stay read-only.
    let old: N8nSettings = serde_json::from_str(r#"{"url":"https://n8n.example.com"}"#).unwrap();
    assert_eq!(old.permission, ReadOnly);
    assert_eq!(serde_json::to_value(Write).unwrap(), "write");
}

#[test]
fn workflow_json_is_checked_and_reduced_to_what_the_api_takes() {
    let body = sanitize_workflow(&demo("Demo", "demo"), None).unwrap();
    let keys: Vec<&str> = body
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, ["name", "nodes", "connections", "settings"]);
    // Unknown settings are dropped, the known ones kept.
    assert_eq!(body["settings"], json!({"executionOrder": "v1"}));
    // Ids are filled in.
    assert!(body["nodes"][0]["id"].as_str().unwrap().len() > 8);

    // Defaults for what a model leaves out.
    let min = sanitize_workflow(
        &json!({"name": "M", "nodes": [{"name": "A", "type": "n8n-nodes-base.noOp"}]}),
        None,
    )
    .unwrap();
    assert_eq!(min["nodes"][0]["typeVersion"], 1);
    assert_eq!(min["nodes"][0]["parameters"], json!({}));
    assert!(min["nodes"][0]["position"].is_array());
    assert_eq!(min["connections"], json!({}));

    let bad = |v: Value| error_key(&sanitize_workflow(&v, None).unwrap_err());
    let k = "automations.n8n_workflow_invalid";
    assert_eq!(bad(json!("x")), k);
    assert_eq!(
        bad(json!({"nodes": [{"name": "A", "type": "a.b"}]})),
        k,
        "no name"
    );
    assert_eq!(bad(json!({"name": "x", "nodes": []})), k, "no nodes");
    assert_eq!(
        bad(
            json!({"name": "x", "nodes": [{"name": "A", "type": "a.b"}, {"name": "A", "type": "a.b"}]})
        ),
        k,
        "duplicate"
    );
    assert_eq!(
        bad(json!({"name": "x", "nodes": [{"name": "A", "type": "webhook"}]})),
        k,
        "short type"
    );
    assert_eq!(
        bad(json!({"name": "x", "nodes": [{"name": "A", "type": "a.b"}],
            "connections": {"A": {"main": [[{"node": "Nope", "type": "main", "index": 0}]]}}})),
        k,
        "dangling connection"
    );
    assert_eq!(
        bad(json!({"name": "x", "nodes": [{"name": "A", "type": "a.b", "position": [1]}]})),
        k
    );
    // An edit may leave the name out.
    assert!(sanitize_workflow(
        &json!({"nodes": [{"name": "A", "type": "a.b"}]}),
        Some("Old")
    )
    .is_ok());
}

#[tokio::test]
async fn nothing_is_written_until_the_user_approves() {
    let (url, mock) = start().await;
    let c = client(&url);
    let store = ProposalStore::new();

    // Read only: no proposal at all.
    let e = store
        .stage_workflow(
            &c,
            N8nPermission::ReadOnly,
            create(demo("Demo", "demo"), None),
        )
        .await
        .unwrap_err();
    assert_eq!(error_key(&e), "automations.n8n_permission_denied");

    let p = store
        .stage_workflow(&c, N8nPermission::Draft, create(demo("Demo", "demo"), None))
        .await
        .unwrap();
    assert_eq!(p.status, ProposalStatus::Pending);
    assert_eq!(p.kind, ProposalKind::CreateWorkflow);
    assert_eq!(p.workflow_name, "Demo");
    assert_eq!(p.node_changes.len(), 2);
    assert!(p.diff.added > 10 && p.diff.removed == 0);
    assert!(p.warnings.iter().any(|w| w.code == "new_webhook"));
    assert!(
        writes(&mock).is_empty(),
        "staging must not write: {:?}",
        calls(&mock)
    );

    // Reject: still nothing written, and it cannot be applied afterwards.
    let r = store.reject(&p.id).unwrap();
    assert_eq!(r.status, ProposalStatus::Rejected);
    let e = store
        .apply(&p.id, &c, N8nPermission::Draft)
        .await
        .unwrap_err();
    assert_eq!(error_key(&e), "automations.n8n_proposal_closed");
    assert!(writes(&mock).is_empty());
}

#[tokio::test]
async fn an_approved_draft_creates_an_inactive_workflow_with_a_clean_body() {
    let (url, mock) = start().await;
    let c = client(&url);
    let store = ProposalStore::new();
    let p = store
        .stage_workflow(&c, N8nPermission::Draft, create(demo("Demo", "demo"), None))
        .await
        .unwrap();
    let done = store.apply(&p.id, &c, N8nPermission::Draft).await.unwrap();
    assert_eq!(done.status, ProposalStatus::Applied, "{:?}", done.error);
    let result = done.result.unwrap();
    assert_eq!(result.steps, ["created"]);
    let wf = result.workflow.unwrap();
    assert_eq!(
        (wf.id.as_str(), wf.name.as_str(), wf.active),
        ("wf1", "Demo", false)
    );
    assert!(wf.webhook.is_some());
    assert_eq!(writes(&mock), ["POST /workflows"]);
    // The body holds exactly the four accepted keys (the mock rejects anything else).
    let body = mock.lock().unwrap().bodies[0].clone();
    assert_eq!(body.as_object().unwrap().len(), 4);
    // It cannot be applied twice.
    let e = store
        .apply(&p.id, &c, N8nPermission::Draft)
        .await
        .unwrap_err();
    assert_eq!(error_key(&e), "automations.n8n_proposal_closed");
    assert_eq!(writes(&mock).len(), 1);
}

#[tokio::test]
async fn going_live_needs_write_and_falls_back_to_activate_on_n8n_1() {
    let (url, mock) = start().await;
    let c = client(&url);
    let store = ProposalStore::new();
    let e = store
        .stage_workflow(
            &c,
            N8nPermission::Draft,
            create(demo("Demo", "demo"), Some(true)),
        )
        .await
        .unwrap_err();
    assert_eq!(error_key(&e), "automations.n8n_permission_denied");

    let p = store
        .stage_workflow(
            &c,
            N8nPermission::Write,
            create(demo("Demo", "demo"), Some(true)),
        )
        .await
        .unwrap();
    assert!(p.warnings.iter().any(|w| w.code == "goes_live"));
    assert_eq!(p.required_permission, N8nPermission::Write);
    // The permission was lowered after staging: refused, and the proposal stays pending.
    let e = store
        .apply(&p.id, &c, N8nPermission::Draft)
        .await
        .unwrap_err();
    assert_eq!(error_key(&e), "automations.n8n_permission_denied");
    assert_eq!(store.get(&p.id).unwrap().status, ProposalStatus::Pending);
    assert!(writes(&mock).is_empty());

    let done = store.apply(&p.id, &c, N8nPermission::Write).await.unwrap();
    assert_eq!(done.status, ProposalStatus::Applied, "{:?}", done.error);
    assert_eq!(
        done.result.as_ref().unwrap().steps,
        ["created", "activated"]
    );
    assert!(done.result.unwrap().workflow.unwrap().active);
    assert_eq!(
        writes(&mock),
        ["POST /workflows", "POST /workflows/wf1/publish"]
    );

    // An instance without /publish (n8n 1.x) is activated through /activate.
    let (url1, mock1) = start().await;
    mock1.lock().unwrap().v1 = true;
    let c1 = client(&url1);
    let p = store
        .stage_workflow(
            &c1,
            N8nPermission::Write,
            create(demo("Old", "old"), Some(true)),
        )
        .await
        .unwrap();
    let done = store.apply(&p.id, &c1, N8nPermission::Write).await.unwrap();
    assert_eq!(done.status, ProposalStatus::Applied, "{:?}", done.error);
    assert_eq!(
        writes(&mock1),
        [
            "POST /workflows",
            "POST /workflows/wf1/publish",
            "POST /workflows/wf1/activate"
        ]
    );
    assert!(done.result.unwrap().workflow.unwrap().active);
}

#[tokio::test]
async fn edits_show_a_diff_and_live_workflows_need_write() {
    let (url, mock) = start().await;
    let c = client(&url);
    let store = ProposalStore::new();
    for (name, path, active) in [("Draft one", "one", false), ("Live one", "two", true)] {
        let body = sanitize_workflow(&demo(name, path), None).unwrap();
        let mut m = mock.lock().unwrap();
        m.next += 1;
        let id = format!("wf{}", m.next);
        let wf = stored(&id, &body, active, 1);
        m.workflows.insert(id, wf);
    }
    let mut changed = demo("Draft one", "one");
    changed["nodes"][1]["parameters"] =
        json!({"values": {"string": [{"name": "a", "value": "b"}]}});

    // The inactive one is editable with draft.
    let p = store
        .stage_workflow(
            &c,
            N8nPermission::Draft,
            StageWorkflow {
                workflow_id: Some("wf1".into()),
                workflow: Some(changed.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(p.kind, ProposalKind::UpdateWorkflow);
    assert_eq!(p.node_changes.len(), 1);
    assert_eq!(p.node_changes[0].name, "Set");
    assert!(p.diff.removed >= 1 && p.diff.added >= 1);
    assert!(p.diff.lines.iter().any(|l| l.text.contains("\"values\"")));

    // The live one is not, with draft.
    let e = store
        .stage_workflow(
            &c,
            N8nPermission::Draft,
            StageWorkflow {
                workflow_id: Some("wf2".into()),
                workflow: Some(demo("Live one", "two")),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error_key(&e), "automations.n8n_permission_denied");
    // ... and an unknown one is "not found".
    let e = store
        .stage_workflow(
            &c,
            N8nPermission::Write,
            StageWorkflow {
                workflow_id: Some("nope".into()),
                workflow: Some(changed.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error_key(&e), "automations.n8n_workflow_not_found");
    // Editing to the same content is not a proposal.
    let e = store
        .stage_workflow(
            &c,
            N8nPermission::Draft,
            StageWorkflow {
                workflow_id: Some("wf1".into()),
                workflow: Some(demo("Draft one", "one")),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error_key(&e), "automations.n8n_workflow_invalid");

    let done = store.apply(&p.id, &c, N8nPermission::Draft).await.unwrap();
    assert_eq!(done.status, ProposalStatus::Applied, "{:?}", done.error);
    assert_eq!(done.result.unwrap().steps, ["updated"]);
    assert_eq!(writes(&mock), ["PUT /workflows/wf1"]);

    // With write the live one can be switched off without touching its content.
    let p = store
        .stage_workflow(
            &c,
            N8nPermission::Write,
            StageWorkflow {
                workflow_id: Some("wf2".into()),
                activate: Some(false),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(p.diff.lines.is_empty());
    let done = store.apply(&p.id, &c, N8nPermission::Write).await.unwrap();
    assert_eq!(done.result.unwrap().steps, ["deactivated"]);
}

#[tokio::test]
async fn a_workflow_that_changed_meanwhile_is_not_overwritten() {
    let (url, mock) = start().await;
    let c = client(&url);
    let store = ProposalStore::new();
    let body = sanitize_workflow(&demo("Demo", "demo"), None).unwrap();
    mock.lock()
        .unwrap()
        .workflows
        .insert("wf1".into(), stored("wf1", &body, false, 1));
    let mut changed = demo("Demo", "demo");
    changed["nodes"][1]["parameters"] = json!({"x": 1});
    let p = store
        .stage_workflow(
            &c,
            N8nPermission::Draft,
            StageWorkflow {
                workflow_id: Some("wf1".into()),
                workflow: Some(changed),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    // Somebody edits it in n8n after the proposal was staged.
    mock.lock()
        .unwrap()
        .workflows
        .insert("wf1".into(), stored("wf1", &body, false, 2));
    let done = store.apply(&p.id, &c, N8nPermission::Draft).await.unwrap();
    assert_eq!(done.status, ProposalStatus::Failed);
    assert_eq!(
        error_key(done.error.as_deref().unwrap()),
        "automations.n8n_proposal_stale"
    );
    assert!(
        writes(&mock).is_empty(),
        "no write after a stale check: {:?}",
        calls(&mock)
    );
}

#[tokio::test]
async fn n8n_refusals_come_back_with_its_message_and_a_proposal_for_another_connection_is_refused()
{
    let (url, mock) = start().await;
    let c = client(&url);
    let store = ProposalStore::new();
    mock.lock().unwrap().refuse = Some("request/body/settings is invalid".into());
    let p = store
        .stage_workflow(&c, N8nPermission::Draft, create(demo("Demo", "demo"), None))
        .await
        .unwrap();
    let done = store.apply(&p.id, &c, N8nPermission::Draft).await.unwrap();
    assert_eq!(done.status, ProposalStatus::Failed);
    let (key, params) = crate::error_key::parse(done.error.as_deref().unwrap()).unwrap();
    assert_eq!(key, "automations.n8n_rejected");
    assert_eq!(params["message"], "request/body/settings is invalid");
    assert_eq!(params["status"], 400);

    // Another n8n address than the one the proposal was staged for.
    let (other_url, _) = start().await;
    let p = store
        .stage_workflow(
            &c,
            N8nPermission::Draft,
            create(demo("Demo2", "demo2"), None),
        )
        .await
        .unwrap();
    let e = store
        .apply(&p.id, &client(&other_url), N8nPermission::Draft)
        .await
        .unwrap_err();
    assert_eq!(error_key(&e), "automations.n8n_connection_changed");
    assert_eq!(store.get(&p.id).unwrap().status, ProposalStatus::Pending);
}

#[tokio::test]
async fn a_run_needs_trigger_and_reports_the_execution() {
    let (url, mock) = start().await;
    let c = client(&url);
    let store = ProposalStore::new();
    let body = sanitize_workflow(&demo("Runner", "run-me"), None).unwrap();
    mock.lock()
        .unwrap()
        .workflows
        .insert("wf1".into(), stored("wf1", &body, true, 1));
    // An older run exists: it must not be reported as the new one.
    mock.lock().unwrap().executions.push(json!({
        "id": 50, "workflowId": "wf1", "status": "error", "mode": "webhook", "finished": true
    }));

    let e = store
        .stage_run(&c, N8nPermission::Write, None, "wf1", None, None)
        .await
        .unwrap_err();
    assert_eq!(error_key(&e), "automations.n8n_permission_denied");

    let p = store
        .stage_run(
            &c,
            N8nPermission::Trigger,
            Some("call-1".into()),
            "wf1",
            Some(json!({"q": "hi"})),
            Some("check it".into()),
        )
        .await
        .unwrap();
    assert_eq!(p.kind, ProposalKind::RunWorkflow);
    assert_eq!(p.webhook.as_ref().unwrap().method, "POST");
    assert_eq!(store.for_call("call-1").unwrap().id, p.id);
    assert!(
        mock.lock().unwrap().hooks.is_empty(),
        "nothing runs before approval"
    );

    let done = store
        .apply(&p.id, &c, N8nPermission::Trigger)
        .await
        .unwrap();
    assert_eq!(done.status, ProposalStatus::Applied, "{:?}", done.error);
    let r = done.result.unwrap();
    assert_eq!(r.steps, ["triggered"]);
    assert!(r.trigger.as_ref().unwrap().ok);
    let m = mock.lock().unwrap();
    assert_eq!(m.hooks.len(), 1);
    assert_eq!(m.hooks[0].0, "POST run-me");
    assert!(!m.hooks[0].1, "the API key must not go to the webhook");
    assert_eq!(
        serde_json::from_str::<Value>(&m.hooks[0].2).unwrap(),
        json!({"q": "hi"})
    );
    drop(m);
    let ex = r.execution.unwrap();
    assert_eq!(ex.id, "101", "the new run, not the old one");
    assert_eq!(ex.status, "success");
    assert_eq!(
        ex.nodes
            .iter()
            .map(|n| (n.name.as_str(), n.items))
            .collect::<Vec<_>>(),
        [("Webhook", Some(1)), ("Set", Some(2))]
    );
    assert_eq!(ex.output.as_deref(), Some(r#"{"hello":"world"}"#));
}

#[tokio::test]
async fn a_run_of_an_inactive_or_webhookless_workflow_is_refused_up_front() {
    let (url, mock) = start().await;
    let c = client(&url);
    let store = ProposalStore::new();
    let body = sanitize_workflow(&demo("Idle", "idle"), None).unwrap();
    mock.lock()
        .unwrap()
        .workflows
        .insert("wf1".into(), stored("wf1", &body, false, 1));
    let e = store
        .stage_run(&c, N8nPermission::Trigger, None, "wf1", None, None)
        .await
        .unwrap_err();
    assert_eq!(error_key(&e), "automations.n8n_workflow_inactive");
    let manual = sanitize_workflow(
        &json!({"name": "M", "nodes": [{"name": "Manual", "type": "n8n-nodes-base.manualTrigger"}]}),
        None,
    )
    .unwrap();
    mock.lock()
        .unwrap()
        .workflows
        .insert("wf2".into(), stored("wf2", &manual, true, 1));
    let e = store
        .stage_run(&c, N8nPermission::Trigger, None, "wf2", None, None)
        .await
        .unwrap_err();
    assert_eq!(error_key(&e), "automations.n8n_no_webhook");
}

#[tokio::test]
async fn a_waiting_caller_wakes_when_the_user_decides() {
    let (url, _mock) = start().await;
    let c = client(&url);
    let store = Arc::new(ProposalStore::new());
    let p = store
        .stage_workflow(&c, N8nPermission::Draft, create(demo("Demo", "demo"), None))
        .await
        .unwrap();
    // Undecided: wait returns after the limit, still pending.
    let waited = store.wait(&p.id, Duration::from_millis(100)).await.unwrap();
    assert_eq!(waited.status, ProposalStatus::Pending);

    let s2 = store.clone();
    let id = p.id.clone();
    let waiter = tokio::spawn(async move { s2.wait(&id, Duration::from_secs(10)).await.unwrap() });
    tokio::time::sleep(Duration::from_millis(50)).await;
    store.reject(&p.id).unwrap();
    assert_eq!(waiter.await.unwrap().status, ProposalStatus::Rejected);
}

#[test]
fn the_tools_offered_follow_the_permission() {
    use super::tools::*;
    use N8nPermission::*;
    assert_eq!(
        tool_names(ReadOnly),
        [LIST_WORKFLOWS, GET_WORKFLOW, LIST_EXECUTIONS, GET_EXECUTION]
    );
    assert!(tool_names(Draft).contains(&PROPOSE_WORKFLOW));
    assert!(tool_names(Draft).contains(&PROPOSAL_STATUS));
    assert!(!tool_names(Draft).contains(&PROPOSE_RUN));
    assert!(!tool_names(Write).contains(&PROPOSE_RUN));
    assert!(tool_names(Trigger).contains(&PROPOSE_RUN));
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn the_tools_run_through_the_registry_and_obey_the_saved_permission() {
    use crate::tools::{RunLimits, ToolCtx, ToolInvocation, ToolOrigin, ToolRegistry};
    let _guard = super::tests::SAVED
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (url, mock) = start().await;
    save(&url, Some(KEY)).unwrap();
    let registry = Arc::new(ToolRegistry::new());
    let run = |tool: &str, args: Value, limits: RunLimits| {
        let reg = registry.clone();
        let inv = ToolInvocation::new(tool, args, ToolOrigin::new("test"));
        async move { reg.run(&inv, &ToolCtx::new(None), &limits).await }
    };

    // Connected, read only: reading tools, no proposals.
    tools::sync(&registry);
    let names: Vec<String> = registry.list().into_iter().map(|d| d.name).collect();
    assert_eq!(names, tools::tool_names(N8nPermission::ReadOnly));
    let r = run(tools::LIST_WORKFLOWS, json!({}), RunLimits::default()).await;
    assert!(!r.is_error(), "{}", r.content);
    assert_eq!(r.content, "[]");

    // Draft: the propose tool stages and waits; the "user" rejects meanwhile.
    set_permission(N8nPermission::Draft).unwrap();
    tools::sync(&registry);
    assert!(registry.get(tools::PROPOSE_WORKFLOW).is_some());
    let limits = RunLimits {
        timeout: Duration::from_secs(20),
        ..Default::default()
    };
    let call = tokio::spawn({
        let args = json!({"workflow": demo("Via tool", "via-tool"), "reason": "demo"});
        let inv = ToolInvocation::new(tools::PROPOSE_WORKFLOW, args, ToolOrigin::new("test"))
            .with_id("call-xyz");
        let reg = registry.clone();
        let limits = limits.clone();
        async move { reg.run(&inv, &ToolCtx::new(None), &limits).await }
    });
    let store = ProposalStore::global();
    let p = loop {
        if let Some(p) = store.for_call("call-xyz") {
            break p;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert_eq!(p.note.as_deref(), Some("demo"));
    assert!(writes(&mock).is_empty());
    let c = client(&url);
    let done = store.apply(&p.id, &c, N8nPermission::Draft).await.unwrap();
    assert_eq!(done.status, ProposalStatus::Applied, "{:?}", done.error);
    let r = call.await.unwrap();
    assert!(!r.is_error(), "{}", r.content);
    let out: Value = serde_json::from_str(&r.content).unwrap();
    assert_eq!(out["status"], "applied");
    assert_eq!(out["workflow_id"], "wf1");
    assert_eq!(out["active"], false);
    assert_eq!(out["steps"], json!(["created"]));

    // The permission is lowered while the tool is still registered: it refuses.
    set_permission(N8nPermission::ReadOnly).unwrap();
    let inv = ToolInvocation::new(
        tools::PROPOSE_WORKFLOW,
        json!({"workflow": demo("X", "x")}),
        ToolOrigin::new("test"),
    );
    let r = registry
        .run(&inv, &ToolCtx::new(None), &RunLimits::default())
        .await;
    assert!(r.is_error());
    assert!(r.content.contains("does not allow"), "{}", r.content);
    // Syncing takes the propose tools away; disconnecting removes them all.
    tools::sync(&registry);
    assert!(registry.get(tools::PROPOSE_WORKFLOW).is_none());
    save("", None).unwrap();
    tools::sync(&registry);
    assert!(registry.list().is_empty());
}

#[tokio::test]
async fn execution_reports_cover_errors() {
    let (url, mock) = start().await;
    let c = client(&url);
    mock.lock().unwrap().executions.push(json!({
        "id": 7, "workflowId": "wf1", "status": "error", "mode": "webhook", "finished": true,
        "data": {"resultData": {
            "lastNodeExecuted": "HTTP",
            "error": {"message": "connect ECONNREFUSED", "node": {"name": "HTTP"}},
            "runData": {"HTTP": [{"startTime": 5, "executionStatus": "error",
                "error": {"message": "connect ECONNREFUSED"}}]}
        }}
    }));
    let r = c.execution_report("7").await.unwrap();
    assert_eq!(r.status, "error");
    assert_eq!(r.error.as_ref().unwrap().node.as_deref(), Some("HTTP"));
    assert_eq!(r.nodes[0].status, "error");
    assert_eq!(r.nodes[0].error.as_deref(), Some("connect ECONNREFUSED"));
    assert!(r.output.is_none());
}
