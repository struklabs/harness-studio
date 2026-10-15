use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use harness_core::{
    config::AppSettings,
    control_api::{Scope, TokenStore},
    gateway::GatewaySettings,
    link,
    token_policy::TokenPolicy,
};
use harness_gateway::{GatewayContext, GatewayServer};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicU16, Ordering},
    Arc, Mutex,
};

const TOKEN: &str = "hs_remote_inference_secret";
const MODEL: &str = "remote:office/org/model";

#[derive(Clone, Default)]
struct Remote {
    status: Arc<AtomicU16>,
    identity: Arc<AtomicU16>,
    seen: Arc<Mutex<Vec<Value>>>,
    redirect_hits: Arc<AtomicU16>,
}

async fn models(State(remote): State<Remote>, headers: HeaderMap) -> Response {
    assert_eq!(headers["authorization"], format!("Bearer {TOKEN}"));
    match remote.status.load(Ordering::SeqCst) {
        401 => (StatusCode::UNAUTHORIZED, TOKEN).into_response(),
        302 => (StatusCode::FOUND, [("location", "/redirect-target")]).into_response(),
        _ => Json(json!({"data": [
            {"id": "org/model", "harness": {"definition_id": "upstream-definition", "state": "unloaded"}},
            {"id": "remote:other/loop", "owned_by": "harness-link"}
        ]})).into_response(),
    }
}

async fn inference(
    State(remote): State<Remote>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    assert_eq!(headers["authorization"], format!("Bearer {TOKEN}"));
    assert_eq!(headers["x-harness-link-hop"], "1");
    assert_eq!(body["model"], "org/model");
    assert!(headers.get("x-harness-client").is_none());
    remote.seen.lock().unwrap().push(body.clone());
    match remote.status.load(Ordering::SeqCst) {
        401 => (StatusCode::UNAUTHORIZED, TOKEN).into_response(),
        302 => (StatusCode::FOUND, [("location", "/redirect-target")]).into_response(),
        _ if body["stream"] == true => (
            [("content-type", "text/event-stream")],
            "data: {\"id\":\"remote-stream\",\"model\":\"org/model\",\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\ndata: [DONE]\n\n",
        ).into_response(),
        _ => Json(json!({"id": "remote-answer", "model": "org/model", "choices": [{"message": {"role": "assistant", "content": "hello"}}], "usage": {"prompt_tokens": 2, "completion_tokens": 1}})).into_response(),
    }
}

#[tokio::test]
async fn remote_models_proxy_policy_and_failures() {
    harness_core::secrets::use_memory_store();
    let dir = tempfile::tempdir().unwrap();
    harness_core::paths::init(dir.path().to_path_buf());
    let remote = Remote::default();
    let app = Router::new()
        .route(
            "/health",
            get(|headers: HeaderMap| async move {
                assert!(headers.get("authorization").is_none());
                "ok"
            }),
        )
        .route("/v1/models", get(models))
        .route(
            "/v1/harness/whoami",
            get(
                |State(remote): State<Remote>, headers: HeaderMap| async move {
                    assert_eq!(headers["authorization"], format!("Bearer {TOKEN}"));
                    match remote.identity.load(Ordering::SeqCst) {
                        1 => Json(json!({"scopes": ["admin"]})).into_response(),
                        2 => Json(json!({"scopes": ["inference"]})).into_response(),
                        _ => StatusCode::NOT_FOUND.into_response(),
                    }
                },
            ),
        )
        .route("/v1/chat/completions", post(inference))
        .route("/v1/embeddings", post(inference))
        .route("/v1/completions", post(inference))
        .route("/v1/responses", post(inference))
        .route("/v1/rerank", post(inference))
        .route("/v1/messages", post(inference))
        .route(
            "/redirect-target",
            get(|State(remote): State<Remote>| async move {
                remote.redirect_hits.fetch_add(1, Ordering::SeqCst);
                "must not follow"
            }),
        )
        .with_state(remote.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let added = link::add(
        "office".into(),
        format!("http://{address}/v1/"),
        TOKEN.into(),
        true,
    )
    .await
    .unwrap();
    assert_eq!(added.status.state, link::State::Online);
    assert_eq!(added.status.model_count, 1);
    assert!(added.status.warning.is_none());
    remote.identity.store(1, Ordering::SeqCst);
    let tested = link::test(&added.link.id).await.unwrap();
    assert_eq!(tested.state, link::State::Online);
    assert_eq!(
        harness_core::error_key::parse(tested.warning.as_deref().unwrap())
            .unwrap()
            .0,
        "link.inference_scope_warning"
    );
    remote.identity.store(2, Ordering::SeqCst);
    assert!(link::test(&added.link.id).await.unwrap().warning.is_none());
    assert!(!serde_json::to_string(&link::list())
        .unwrap()
        .contains(TOKEN));
    let settings_file =
        std::fs::read_to_string(harness_core::paths::config_dir().join("settings.json")).unwrap();
    assert!(!settings_file.contains(TOKEN));
    let mut incoming = AppSettings::default();
    harness_core::network::sanitize_settings(&mut incoming, &AppSettings::load()).unwrap();
    incoming.save().unwrap();
    assert_eq!(AppSettings::load().links.len(), 1);

    let local_file = dir.path().join("local.gguf");
    std::fs::write(&local_file, b"GGUF").unwrap();
    let local = harness_core::domain::ModelDefinition::new(
        "local-model",
        harness_core::domain::ModelRef::Local { path: local_file },
    );
    harness_core::domain::store::Domain::global()
        .save_definition(local)
        .unwrap();
    let gateway = GatewayServer::new();
    let status = gateway
        .start_ephemeral(
            GatewayContext {
                registry: Arc::new(harness_core::servers::ServerRegistry::default()),
                runtime: Arc::new(harness_core::runtime::RuntimeManager::default()),
                gate: Arc::new(harness_core::operation_gate::OperationGate::default()),
                data_dir: dir.path().to_path_buf(),
                downloads: None,
            },
            &GatewaySettings {
                enabled: true,
                port: 0,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let base = status.endpoint.unwrap().host_url;
    let http = reqwest::Client::new();
    let session = gateway.client_token().await.unwrap();
    let whoami_url = format!("{base}/v1/harness/whoami");
    for token in [None, Some("invalid")] {
        let mut request = http.get(&whoami_url);
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        assert_eq!(request.send().await.unwrap().status(), 401);
    }
    let identity: Value = http
        .get(&whoami_url)
        .bearer_auth(&session)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(identity["token_name"], "harness");
    assert_eq!(identity["scopes"], json!(["inference"]));
    assert!(identity["harness_version"]
        .as_str()
        .is_some_and(|v| !v.is_empty()));
    assert!(!identity.to_string().contains(&session));

    for (name, scopes, policy, status) in [
        (
            "identity-admin",
            vec![Scope::Admin],
            TokenPolicy::default(),
            200,
        ),
        (
            "identity-inference",
            vec![Scope::Inference],
            TokenPolicy {
                endpoints: vec![harness_core::token_policy::GatewayEndpoint::Chat],
                ..Default::default()
            },
            200,
        ),
        (
            "identity-read",
            vec![Scope::Read],
            TokenPolicy::default(),
            403,
        ),
    ] {
        let token = TokenStore::global()
            .create_with_policy(name, &scopes, &policy)
            .unwrap();
        let response = http
            .get(&whoami_url)
            .bearer_auth(&token.token)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), status);
        if status == 200 {
            let identity: Value = response.json().await.unwrap();
            assert_eq!(identity["token_name"], name);
            assert_eq!(identity["scopes"], serde_json::to_value(scopes).unwrap());
            assert!(!identity.to_string().contains(&token.token));
        }
    }
    let listing: Value = http
        .get(format!("{base}/v1/models"))
        .bearer_auth(&session)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(listing["data"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["id"] == "local-model"));
    let found = listing["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == MODEL)
        .unwrap();
    assert_eq!(found["owned_by"], "harness-link");
    assert_eq!(found["harness"]["remote"]["link"], "office");
    assert_eq!(found["harness"]["remote"]["state"], "online");
    assert_eq!(
        found["harness"]["remote"]["definition_id"],
        "upstream-definition"
    );
    assert_eq!(found["harness"]["remote"]["model_state"], "unloaded");

    for path in [
        "chat/completions",
        "completions",
        "responses",
        "embeddings",
        "rerank",
        "messages",
    ] {
        for hop in ["1", "", "2"] {
            let response = http
                .post(format!("{base}/v1/{path}"))
                .bearer_auth(&session)
                .header("x-harness-link-hop", hop)
                .json(&json!({"model": MODEL, "messages": [], "input": "hi"}))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 400, "{path}");
            let error: Value = response.json().await.unwrap();
            assert_eq!(error["error"]["code"], "link.chained_not_allowed");
            assert_eq!(
                error["error"]["harness"]["error_key"],
                "link.chained_not_allowed"
            );
        }
    }
    assert!(remote.seen.lock().unwrap().is_empty());

    let mut events = harness_core::events::subscribe();
    for stream in [false, true] {
        let response = http.post(format!("{base}/v1/chat/completions")).bearer_auth(&session)
            .json(&json!({"model": MODEL, "messages": [{"role": "user", "content": "hi"}], "stream": stream}))
            .send().await.unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["x-harness-model"], MODEL);
        assert_eq!(response.headers()["x-harness-server"], "office");
        let text = response.text().await.unwrap();
        assert!(text.contains("hello"));
        assert_eq!(text.contains("[DONE]"), stream);
        assert!(!text.contains(TOKEN));
    }
    let embeddings = http
        .post(format!("{base}/v1/embeddings"))
        .bearer_auth(&session)
        .json(&json!({"model": MODEL, "input": "hi"}))
        .send()
        .await
        .unwrap();
    assert_eq!(embeddings.status(), 200);
    let _ = embeddings.bytes().await.unwrap();
    for path in [
        "/v1/completions",
        "/v1/responses",
        "/v1/rerank",
        "/v1/messages",
    ] {
        let response = http.post(format!("{base}{path}")).bearer_auth(&session)
            .json(&json!({"model": MODEL, "input": "hi", "previous_response_id": "remote-previous", "background": true, "tools": [{"type": "web_search"}]}))
            .send().await.unwrap();
        assert_eq!(response.status(), 200, "{path}");
        let _ = response.bytes().await.unwrap();
    }
    let seen = remote.seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 7);
    assert!(seen
        .iter()
        .any(|b| b["previous_response_id"] == "remote-previous"
            && b["background"] == true
            && b["tools"][0]["type"] == "web_search"));
    let record = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if let harness_core::events::CoreEvent::GatewayRequest { record } =
                events.recv().await.unwrap()
            {
                if record.definition_id.as_deref() == Some(MODEL) {
                    break record;
                }
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(record.model.as_deref(), Some(MODEL));
    assert_eq!(record.alias.as_deref(), Some(MODEL));
    assert_eq!(record.server_id.as_deref(), Some("office"));
    let serialized = serde_json::to_string(&record).unwrap();
    assert!(!serialized.contains(TOKEN));
    assert!(!serialized.contains(&address.to_string()));

    let store = TokenStore::global();
    let denied = store
        .create_with_policy(
            "local-only",
            &[Scope::Inference],
            &TokenPolicy {
                models: vec!["local-*".into()],
                ..Default::default()
            },
        )
        .unwrap();
    let response = http
        .post(format!("{base}/v1/chat/completions"))
        .bearer_auth(&denied.token)
        .json(&json!({"model": MODEL, "messages": []}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 403);
    let filtered: Value = http
        .get(format!("{base}/v1/models"))
        .bearer_auth(&denied.token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(!filtered["data"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["id"] == MODEL));
    let allowed = store
        .create_with_policy(
            "remote-only",
            &[Scope::Inference],
            &TokenPolicy {
                models: vec!["remote:office/*".into()],
                ..Default::default()
            },
        )
        .unwrap();
    let response = http
        .post(format!("{base}/v1/chat/completions"))
        .bearer_auth(&allowed.token)
        .json(&json!({"model": MODEL, "messages": []}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let _ = response.bytes().await.unwrap();

    for path in [
        "/v1/images/generations",
        "/v1/audio/speech",
        "/v1/decisions",
        "/v1/messages/count_tokens",
    ] {
        let response = http
            .post(format!("{base}{path}"))
            .bearer_auth(&session)
            .json(&json!({"model": MODEL, "prompt": "hi", "input": "hi"}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 400, "{path}");
        let body: Value = response.json().await.unwrap();
        assert_eq!(
            body["error"]["code"], "link.unsupported_endpoint",
            "{path}: {body}"
        );
    }

    remote.status.store(401, Ordering::SeqCst);
    let response = http
        .post(format!("{base}/v1/chat/completions"))
        .bearer_auth(&session)
        .json(&json!({"model": MODEL, "messages": []}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 502);
    let text = response.text().await.unwrap();
    assert!(text.contains("link.unauthorized"));
    assert!(!text.contains(TOKEN));
    assert_eq!(
        link::test(&added.link.id).await.unwrap().state,
        link::State::Unauthorized
    );

    remote.status.store(302, Ordering::SeqCst);
    assert_eq!(
        link::test(&added.link.id).await.unwrap().state,
        link::State::Offline
    );
    assert_eq!(remote.redirect_hits.load(Ordering::SeqCst), 0);
    remote.status.store(200, Ordering::SeqCst);
    link::test(&added.link.id).await.unwrap();
    remote.status.store(302, Ordering::SeqCst);
    let response = http
        .post(format!("{base}/v1/chat/completions"))
        .bearer_auth(&session)
        .json(&json!({"model": MODEL, "messages": []}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 502);
    assert_eq!(remote.redirect_hits.load(Ordering::SeqCst), 0);

    task.abort();
    let _ = task.await;
    let response = http
        .post(format!("{base}/v1/chat/completions"))
        .bearer_auth(&session)
        .json(&json!({"model": MODEL, "messages": []}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 502);
    let offline = link::test(&added.link.id).await.unwrap();
    assert_eq!(offline.state, link::State::Offline);
    assert_eq!(offline.model_count, 1);
    assert!(offline.last_ok.is_some());
    let listing: Value = http
        .get(format!("{base}/v1/models"))
        .bearer_auth(&session)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let found = listing["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == MODEL)
        .unwrap();
    assert_eq!(found["harness"]["state"], "offline");
    let response = http
        .post(format!("{base}/v1/chat/completions"))
        .bearer_auth(&session)
        .json(&json!({"model": MODEL, "messages": []}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 502);
    gateway.stop().await;
    let duplicate = link::add(
        "office".into(),
        format!("http://{address}"),
        TOKEN.into(),
        true,
    )
    .await
    .unwrap_err();
    assert_eq!(duplicate, "link.name_taken");
    let updated = link::update(
        added.link.id.clone(),
        "office-renamed".into(),
        added.link.base_url.clone(),
        None,
        true,
        false,
    )
    .await
    .unwrap();
    assert!(!updated.link.enabled);
    assert!(link::definitions().is_empty());
    assert_eq!(
        harness_core::secrets::link_get(&added.link.id)
            .unwrap()
            .as_deref(),
        Some(TOKEN)
    );
    link::remove(&added.link.id).await.unwrap();
    assert!(link::list().is_empty());
    assert_eq!(
        harness_core::secrets::link_get(&added.link.id).unwrap(),
        None
    );
}
