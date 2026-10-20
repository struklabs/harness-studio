//! Trust levels and grants over real HTTP: fail-closed defaults, grant and revoke, the training
//! placeholder, the `admin` scope and the audit rows in the tool log.

mod common;

use common::{json, start};
use reqwest::Method;
use serde_json::{json, Value};

static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn subject<'a>(list: &'a Value, kind: &str, id: &str) -> &'a Value {
    list.as_array()
        .unwrap()
        .iter()
        .find(|s| s["kind"] == kind && s["id"] == id)
        .unwrap_or_else(|| panic!("{kind}/{id} in {list}"))
}

#[tokio::test]
async fn grants_are_denied_by_default_revocable_and_audited() {
    let _g = ONE_AT_A_TIME.lock().await;
    let api = start().await;
    let (status, list) = json(api.get("/permissions").await).await;
    assert_eq!(status, 200, "{list}");
    let hermes = subject(&list, "agent", "hermes");
    assert!(hermes["grants"].as_array().unwrap().is_empty() && hermes["trust"].is_null());
    let trainer = subject(&list, "trainer", "training");
    assert_eq!(trainer["available"], false);
    assert!(trainer["grantable"].as_array().unwrap().is_empty());

    // The training placeholder takes no run capability and no level.
    let (status, body) = json(
        api.post(
            "/permissions/grants",
            json!({"kind": "trainer", "id": "training", "capability": "network"}),
        )
        .await,
    )
    .await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (409, Some("permissions.not_available")),
        "{body}"
    );
    let (status, _) = json(
        api.send(
            Method::PUT,
            "/permissions/trainer/training/trust",
            &api.token,
            Some(json!({"trust": "safe"})),
        )
        .await,
    )
    .await;
    assert_eq!(status, 409);

    // Agents take file grants only; folders must exist.
    let (status, body) = json(
        api.post(
            "/permissions/grants",
            json!({"kind": "agent", "id": "hermes", "capability": "shell"}),
        )
        .await,
    )
    .await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (400, Some("permissions.capability_unsupported")),
        "{body}"
    );
    let work = tempfile::tempdir().unwrap();
    let (status, grant) = json(
        api.post(
            "/permissions/grants",
            json!({"kind": "agent", "id": "hermes", "capability": "file",
                   "paths": [work.path().to_string_lossy()]}),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200, "{grant}");
    let (_, list) = json(api.get("/permissions").await).await;
    assert_eq!(
        subject(&list, "agent", "hermes")["grants"][0]["id"],
        grant["id"]
    );

    // An MCP level and grant, then the whole subject is revoked.
    let (status, _) = json(
        api.send(
            Method::PUT,
            "/permissions/mcp_server/files/trust",
            &api.token,
            Some(json!({"trust": "restricted"})),
        )
        .await,
    )
    .await;
    assert_eq!(status, 204);
    let (_, list) = json(api.get("/permissions").await).await;
    assert_eq!(subject(&list, "mcp_server", "files")["trust"], "restricted");
    let path = format!(
        "/permissions/agent/hermes/grants/{}",
        grant["id"].as_str().unwrap()
    );
    assert_eq!(
        api.send(Method::DELETE, &path, &api.token, None)
            .await
            .status(),
        204
    );
    let (status, body) = json(api.send(Method::DELETE, &path, &api.token, None).await).await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (404, Some("permissions.grant_not_found")),
        "{body}"
    );
    assert_eq!(
        api.send(
            Method::DELETE,
            "/permissions/mcp_server/files",
            &api.token,
            None
        )
        .await
        .status(),
        204
    );
    let (_, list) = json(api.get("/permissions").await).await;
    assert!(list
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s["grants"].as_array().unwrap().is_empty() && s["trust"].is_null()));

    // Every change is in the existing tool log, with the API as the client.
    let (status, log) = json(api.get("/tools/log?limit=200").await).await;
    assert_eq!(status, 200, "{log}");
    let tools: Vec<&str> = log
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["source"] == "permissions")
        .map(|r| r["tool"].as_str().unwrap())
        .collect();
    for t in ["grant", "revoke_grant", "revoke_subject", "set_trust"] {
        assert!(
            tools.contains(&format!("permissions.{t}").as_str()),
            "{t}: {tools:?}"
        );
    }
    assert!(log
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["source"] == "permissions")
        .all(|r| r["client"] == "api"));
}

#[tokio::test]
async fn permissions_need_admin() {
    let _g = ONE_AT_A_TIME.lock().await;
    let api = start().await;
    let (status, body) = json(
        api.post("/tokens", json!({"name": "reader", "scopes": ["read"]}))
            .await,
    )
    .await;
    assert_eq!(status, 201, "{body}");
    let reader = body["token"].as_str().unwrap().to_string();
    assert_eq!(
        api.send(Method::GET, "/permissions", &reader, None)
            .await
            .status(),
        403
    );
    assert_eq!(
        api.send(
            Method::POST,
            "/permissions/grants",
            &reader,
            Some(json!({"kind": "mcp_server", "id": "x", "capability": "network"}))
        )
        .await
        .status(),
        403
    );
}
