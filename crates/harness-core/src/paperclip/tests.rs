use super::*;

use std::sync::{Arc, Mutex};

use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use serde_json::json;

const KEY: &str = "pcp-board-key";
const CO: &str = "c0ffee00-0000-0000-0000-000000000001";

#[derive(Default)]
struct Seen {
    /// `(path, query, authorization header)` of every request.
    calls: Vec<(String, String, Option<String>)>,
}

type Shared = Arc<Mutex<Seen>>;

fn body(v: Value) -> axum::response::Response {
    ([("content-type", "application/json")], v.to_string()).into_response()
}

fn record(s: &Shared, h: &HeaderMap, path: &str, q: Option<String>) {
    let auth = h
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    s.lock()
        .unwrap()
        .calls
        .push((path.to_string(), q.unwrap_or_default(), auth));
}

fn authorized(h: &HeaderMap) -> bool {
    h.get("authorization").and_then(|v| v.to_str().ok()) == Some(&format!("Bearer {KEY}"))
}

/// A mocked Paperclip: a CEO, an engineer under it, three tasks, two runs and a budget.
async fn start() -> (String, Shared) {
    let seen: Shared = Arc::default();
    let app = Router::new()
        .route(
            "/api/health",
            get(|| async {
                body(json!({"status": "ok", "version": "2026.1.0", "deploymentMode": "authenticated"}))
            }),
        )
        .route(
            "/api/companies",
            get(
                |State(s): State<Shared>, h: HeaderMap, RawQuery(q): RawQuery| async move {
                    record(&s, &h, "/companies", q);
                    if !authorized(&h) {
                        return StatusCode::UNAUTHORIZED.into_response();
                    }
                    body(json!([{
                        "id": CO, "name": "Acme AI", "status": "active", "budgetMonthlyCents": 100000
                    }]))
                },
            ),
        )
        .route(
            "/api/companies/{company}",
            get(|Path(c): Path<String>| async move {
                if c != CO {
                    return StatusCode::NOT_FOUND.into_response();
                }
                body(json!({"id": CO, "name": "Acme AI", "status": "active", "budgetMonthlyCents": 100000}))
            }),
        )
        .route(
            "/api/companies/{company}/agents",
            get(|State(s): State<Shared>, h: HeaderMap| async move {
                record(&s, &h, "/agents", None);
                body(json!([
                    {"id": "a-ceo", "name": "CEO", "role": "ceo", "title": "Chief", "status": "idle",
                     "adapterType": "claude_local", "adapterConfig": {"model": "claude-sonnet"},
                     "budgetMonthlyCents": 5000, "spentMonthlyCents": 1200},
                    {"id": "a-eng", "name": "Engineer", "role": "engineer", "reportsTo": "a-ceo",
                     "status": "running", "adapterType": "opencode_local",
                     "adapterConfig": {"model": "harness/qwen"}, "budgetMonthlyCents": 2000}
                ]))
            }),
        )
        .route(
            "/api/companies/{company}/issues",
            get(|State(s): State<Shared>, h: HeaderMap| async move {
                record(&s, &h, "/issues", None);
                body(json!([
                    {"id": "t1", "identifier": "PAP-1", "title": "Ship it", "status": "in_progress",
                     "priority": "high", "assigneeAgentId": "a-eng", "projectId": "p1"},
                    {"id": "t2", "title": "Write docs", "status": "todo"},
                    {"title": "no id is skipped"}
                ]))
            }),
        )
        .route(
            "/api/companies/{company}/heartbeat-runs",
            get(
                |State(s): State<Shared>, h: HeaderMap, RawQuery(q): RawQuery| async move {
                    record(&s, &h, "/runs", q);
                    body(json!([
                        {"id": "r1", "agentId": "a-eng", "issueId": "t1", "status": "succeeded",
                         "invocationSource": "timer", "startedAt": "2026-10-01T10:00:00Z",
                         "finishedAt": "2026-10-01T10:05:00Z"},
                        {"id": "r2", "agentId": "a-ceo", "status": "failed", "error": "adapter exited 1"}
                    ]))
                },
            ),
        )
        .route(
            "/api/companies/{company}/dashboard",
            get(|| async {
                body(json!({
                    "costs": {"monthSpendCents": 1200, "monthBudgetCents": 100000,
                              "monthUtilizationPercent": 1.2},
                    "pendingApprovals": 2,
                    "budgets": {"activeIncidents": 1, "pausedAgents": 1, "pausedProjects": 0}
                }))
            }),
        )
        .route(
            "/api/companies/{company}/costs/by-agent",
            get(|| async {
                body(json!([{"agentId": "a-ceo", "costCents": 1200, "inputTokens": 5000, "outputTokens": 800}]))
            }),
        )
        .with_state(seen.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (url, seen)
}

#[test]
fn urls_are_normalized_and_validated() {
    assert_eq!(
        normalize_url(" http://localhost:3100/api/ ").unwrap(),
        "http://localhost:3100"
    );
    assert_eq!(
        normalize_url("https://paperclip.example.com/").unwrap(),
        "https://paperclip.example.com"
    );
    assert_eq!(
        normalize_url("https://example.com/team/api").unwrap(),
        "https://example.com/team"
    );
    for bad in [
        "",
        "not a url",
        "ftp://example.com",
        // Plain http is only for this machine: the key would cross the network in clear.
        "http://paperclip.example.com",
        "https://user:pw@example.com",
        "https://example.com/?x=1",
        "https://example.com/#frag",
    ] {
        assert_eq!(normalize_url(bad).unwrap_err(), ERR_URL_INVALID, "{bad}");
    }
}

#[test]
fn ids_are_path_safe() {
    assert!(segment("c0ffee00-0000").is_ok());
    for bad in ["", "../x", "a/b", "a?b", "a b", &"x".repeat(65)] {
        assert_eq!(segment(bad).unwrap_err(), "paperclip.id_invalid", "{bad}");
    }
}

#[test]
fn http_statuses_map_to_keys() {
    assert_eq!(status_error(401), "paperclip.unauthorized");
    assert_eq!(status_error(403), "paperclip.forbidden");
    assert_eq!(status_error(404), "paperclip.api_missing");
    assert_eq!(status_error(307), "paperclip.redirect");
    assert!(status_error(503).contains("paperclip.server_error"));
    assert!(status_error(418).contains("paperclip.http_error"));
}

#[tokio::test]
async fn connection_test_reports_health_and_companies() {
    let (url, seen) = start().await;
    let ok = PaperclipClient::new(&format!("{url}/"), Some(KEY))
        .unwrap()
        .test()
        .await
        .unwrap();
    assert!(ok.ok);
    assert_eq!(ok.url, url);
    assert_eq!(ok.health.status, "ok");
    assert_eq!(ok.health.deployment_mode.as_deref(), Some("authenticated"));
    assert_eq!(ok.companies.len(), 1);
    assert_eq!(ok.companies[0].name, "Acme AI");
    assert_eq!(
        seen.lock().unwrap().calls[0].2.as_deref(),
        Some("Bearer pcp-board-key")
    );

    let bad = PaperclipClient::new(&url, Some("wrong"))
        .unwrap()
        .test()
        .await;
    assert_eq!(bad.unwrap_err(), "paperclip.unauthorized");
    let none = PaperclipClient::new(&url, None).unwrap().test().await;
    assert_eq!(none.unwrap_err(), "paperclip.unauthorized");
}

#[tokio::test]
async fn overview_gathers_agents_teams_tasks_runs_and_budget() {
    let (url, seen) = start().await;
    let client = PaperclipClient::new(&url, Some(KEY)).unwrap();
    let links = BTreeMap::from([("t1".to_string(), "harness-p".to_string())]);
    let o = client.overview(CO, &links).await.unwrap();

    assert_eq!(o.company.budget_monthly_cents, 100_000);
    assert_eq!(o.health.version.as_deref(), Some("2026.1.0"));

    assert_eq!(o.agents.len(), 2);
    let eng = &o.agents[1];
    assert_eq!(eng.adapter_type, "opencode_local");
    assert_eq!(eng.model.as_deref(), Some("harness/qwen"));
    assert!(eng.uses_harness && !o.agents[0].uses_harness);
    assert_eq!(o.agents[0].spent_monthly_cents, 1200);
    assert_eq!(
        o.teams,
        vec![Team {
            lead_id: "a-ceo".into(),
            lead_name: "CEO".into(),
            member_ids: vec!["a-eng".into()],
        }]
    );

    // The entry without an id is dropped; the link shows up on its task only.
    assert_eq!(o.tasks.len(), 2);
    assert_eq!(o.tasks[0].identifier.as_deref(), Some("PAP-1"));
    assert_eq!(o.tasks[0].harness_project_id.as_deref(), Some("harness-p"));
    assert_eq!(o.tasks[1].harness_project_id, None);

    assert_eq!(o.runs.len(), 2);
    assert_eq!(o.runs[1].error.as_deref(), Some("adapter exited 1"));

    let b = o.budget.expect("budget");
    assert_eq!(b.month_spend_cents, 1200.0);
    assert_eq!(b.month_budget_cents, 100_000.0);
    assert_eq!(
        (b.pending_approvals, b.paused_agents, b.active_incidents),
        (2, 1, 1)
    );
    assert_eq!(o.costs[0].output_tokens, 800.0);

    let sent = seen.lock().unwrap();
    let runs = sent.calls.iter().find(|c| c.0 == "/runs").unwrap();
    assert!(runs.1.contains("limit=25") && runs.1.contains("summary=true"));
    assert!(sent
        .calls
        .iter()
        .all(|c| c.0 == "/companies" || c.2.is_some()));
}

#[tokio::test]
async fn unknown_company_and_bad_ids_are_refused() {
    let (url, _) = start().await;
    let client = PaperclipClient::new(&url, Some(KEY)).unwrap();
    assert_eq!(
        client.overview("nope", &BTreeMap::new()).await.unwrap_err(),
        "paperclip.api_missing"
    );
    assert_eq!(
        client
            .overview("../etc", &BTreeMap::new())
            .await
            .unwrap_err(),
        "paperclip.id_invalid"
    );
}

#[tokio::test]
async fn redirects_are_not_followed() {
    let hit = Arc::new(Mutex::new(false));
    let h = hit.clone();
    let target = Router::new().fallback(move || {
        let h = h.clone();
        async move {
            *h.lock().unwrap() = true;
            "secret"
        }
    });
    let l2 = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let other = format!("http://{}", l2.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(l2, target).await.unwrap();
    });
    let app = Router::new().fallback(move || {
        let other = other.clone();
        async move { (StatusCode::TEMPORARY_REDIRECT, [("location", other)]) }
    });
    let l1 = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l1.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(l1, app).await.unwrap();
    });
    let r = PaperclipClient::new(&url, Some(KEY))
        .unwrap()
        .companies()
        .await;
    assert_eq!(r.unwrap_err(), "paperclip.redirect");
    assert!(!*hit.lock().unwrap(), "the key must not follow a redirect");
}

#[test]
fn key_goes_to_the_keychain_and_links_follow_the_instance() {
    save("https://pc.example.com/", Some("  s3cret-pc  ")).unwrap();
    let v = view().unwrap();
    assert_eq!(v.url, "https://pc.example.com");
    assert!(v.key_set);
    assert_eq!(
        crate::secrets::paperclip_key_get().unwrap().as_deref(),
        Some("s3cret-pc")
    );
    let file = std::fs::read_to_string(crate::paths::data_root().join("config/settings.json"))
        .or_else(|_| std::fs::read_to_string(crate::paths::data_root().join("settings.json")))
        .unwrap_or_default();
    assert!(file.contains("pc.example.com"));
    assert!(!file.contains("s3cret-pc"));

    // Company and task links.
    assert_eq!(set_company("../x").unwrap_err(), "paperclip.id_invalid");
    set_company(CO).unwrap();
    let v = set_task_link("t1", Some("harness-p")).unwrap();
    assert_eq!(
        v.task_links.get("t1").map(String::as_str),
        Some("harness-p")
    );
    assert_eq!(
        set_task_link("a/b", Some("p")).unwrap_err(),
        "paperclip.id_invalid"
    );
    set_task_link("t2", Some("gone")).unwrap();
    forget_project("gone").unwrap();
    assert!(!view().unwrap().task_links.contains_key("t2"));
    assert!(view().unwrap().task_links.contains_key("t1"));
    // Saving the same address again keeps key, company and links.
    let v = save("https://pc.example.com", None).unwrap();
    assert!(v.key_set && v.company_id == CO && v.task_links.len() == 1);
    assert_eq!(save("not a url", None).unwrap_err(), ERR_URL_INVALID);

    // A key is never carried over to another address, neither by save nor by a test call.
    let evil = PaperclipClient::from_form(Some("https://evil.example.com"), None).unwrap();
    assert!(evil.key.is_none());
    assert_eq!(
        PaperclipClient::from_form(Some("https://pc.example.com/"), None)
            .unwrap()
            .key
            .as_deref(),
        Some("s3cret-pc")
    );
    let v = save("https://pc2.example.com", None).unwrap();
    assert!(!v.key_set);
    assert!(v.company_id.is_empty() && v.task_links.is_empty());

    // An empty address disconnects and removes everything.
    save("https://pc2.example.com", Some("k")).unwrap();
    let off = save("", None).unwrap();
    assert_eq!(off, PaperclipView::default());
    assert!(crate::secrets::paperclip_key_get().unwrap().is_none());
}

#[tokio::test]
async fn a_paperclip_that_is_not_running_is_named_as_such() {
    // A port nothing listens on: the shared local text sent the user to the Servers page.
    let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", free.local_addr().unwrap());
    drop(free);
    let e = PaperclipClient::new(&url, None)
        .unwrap()
        .test()
        .await
        .unwrap_err();
    assert_eq!(
        crate::error_key::parse(&e).map(|(k, _)| k),
        Some("paperclip.unreachable")
    );
}
