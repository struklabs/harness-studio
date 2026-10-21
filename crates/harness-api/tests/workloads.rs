//! Workload consent and checkpoints over real HTTP: a named token is asked before a heavy
//! job, answers are remembered and revocable, the owner credential is never asked, and the
//! routes are admin-only.

mod common;

use common::{json, start};
use reqwest::Method;
use serde_json::json;

static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn a_named_token_is_asked_before_a_heavy_conversion_and_the_answer_is_remembered() {
    let _g = ONE_AT_A_TIME.lock().await;
    let api = start().await;
    let (status, created) = json(
        api.post(
            "/tokens",
            json!({"name": "convtok", "scopes": ["downloads:write"]}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 201, "{created}");
    let token = created["token"].as_str().unwrap().to_string();

    // Not asked yet: 428 with the key the client turns into a prompt.
    let convert = json!({"repo": "owner/name"});
    let (status, body) = json(
        api.send(Method::POST, "/convert", &token, Some(convert.clone()))
            .await,
    )
    .await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (428, Some("workload.consent_required")),
        "{body}"
    );
    assert_eq!(body["error"]["params"]["client"], "api:convtok");

    // The token cannot answer for itself: the routes need admin.
    let (status, _) = json(
        api.send(
            Method::POST,
            "/workloads/consents",
            &token,
            Some(json!({"client": "api:convtok", "kind": "heavy_cpu", "choice": "allow"})),
        )
        .await,
    )
    .await;
    assert_eq!(status, 403);

    // The owner denies; the token is refused (403) and the answer is listed.
    let (status, consent) = json(
        api.post(
            "/workloads/consents",
            json!({"client": "api:convtok", "kind": "heavy_cpu", "choice": "deny"}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200, "{consent}");
    let (status, body) = json(
        api.send(Method::POST, "/convert", &token, Some(convert))
            .await,
    )
    .await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (403, Some("workload.denied")),
        "{body}"
    );
    let (_, list) = json(api.get("/workloads").await).await;
    let cpu = list
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["kind"] == "heavy_cpu")
        .unwrap();
    assert_eq!(cpu["consents"][0]["client"], "api:convtok");
    assert!(
        cpu["os"].is_null(),
        "heavy work has no operating-system switch"
    );

    // Check says what would happen; revoking asks again.
    let check = |client: &'static str| {
        let api = &api;
        async move {
            let (_, v) = json(
                api.post(
                    "/workloads/check",
                    json!({"client": client, "kind": "heavy_cpu"}),
                )
                .await,
            )
            .await;
            v["verdict"].as_str().unwrap().to_string()
        }
    };
    assert_eq!(check("api:convtok").await, "denied");
    let id = consent["id"].as_str().unwrap();
    let resp = api
        .send(
            Method::DELETE,
            &format!("/workloads/consents/{id}"),
            &api.token.clone(),
            None,
        )
        .await;
    assert_eq!(resp.status(), 204);
    assert_eq!(check("api:convtok").await, "ask");
    let (status, body) = json(
        api.send(
            Method::DELETE,
            &format!("/workloads/consents/{id}"),
            &api.token.clone(),
            None,
        )
        .await,
    )
    .await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (404, Some("workload.consent_not_found"))
    );
}

#[tokio::test]
async fn checkpoint_routes_are_admin_only_and_report_missing_ids() {
    let _g = ONE_AT_A_TIME.lock().await;
    let api = start().await;
    let (_, created) = json(
        api.post("/tokens", json!({"name": "reader", "scopes": ["read"]}))
            .await,
    )
    .await;
    let reader = created["token"].as_str().unwrap().to_string();
    let resp = api.send(Method::GET, "/checkpoints", &reader, None).await;
    assert_eq!(resp.status(), 403);

    let (status, list) = json(api.get("/checkpoints").await).await;
    assert_eq!(status, 200);
    assert!(list.is_array());
    let (status, body) = json(api.post("/checkpoints/nope/undo", json!({})).await).await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (404, Some("checkpoint.not_found")),
        "{body}"
    );
}
