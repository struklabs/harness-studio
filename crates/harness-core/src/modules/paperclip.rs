//! Inference handoff to an existing Paperclip OpenCode adapter. No lifecycle operations.
use crate::{
    control_api::{Scope, TokenStore},
    token_policy::{GatewayEndpoint, TokenPolicy},
};
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Serialize)]
pub struct Handoff {
    pub token_id: String,
    pub endpoint: String,
    pub config: Value,
}
pub fn generate(base: &str, model: &str) -> Result<Handoff, String> {
    if !crate::integrations::agent_models::list_with_link()?
        .iter()
        .any(|m| m.alias == model && m.chat != Some(false))
    {
        return Err("agents.model_missing".into());
    }
    generate_in(&TokenStore::global(), base, model)
}
fn generate_in(tokens: &TokenStore, base: &str, model: &str) -> Result<Handoff, String> {
    if model.is_empty() || model.contains(['*', '?']) {
        return Err("agents.model_missing".into());
    }
    let token = tokens.create_with_policy(
        "Paperclip",
        &[Scope::Inference],
        &TokenPolicy {
            models: vec![model.into()],
            endpoints: vec![GatewayEndpoint::Chat, GatewayEndpoint::Models],
            ..Default::default()
        },
    )?;
    Ok(Handoff {
        token_id: token.info.id,
        endpoint: base.into(),
        config: json!({
            "model": format!("harness/{model}"),
            "env": {"OPENCODE_CONFIG_CONTENT": json!({
                "provider": {"harness": {
                    "npm": "@ai-sdk/openai-compatible", "name": "Harness",
                    "options": {"baseURL": base, "apiKey": token.token},
                    "models": {(model): {"name": model}}
                }}
            }).to_string()}
        }),
    })
}
pub fn revoke(id: &str) -> Result<(), String> {
    TokenStore::global().revoke(id)
}
fn agent_url(instance: &str, agent: &str) -> Result<reqwest::Url, String> {
    let mut url = reqwest::Url::parse(instance).map_err(|_| "agents.paperclip_invalid")?;
    if !(url.scheme() == "https"
        || url.scheme() == "http"
            && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]")))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "/" | "" | "/api" | "/api/")
        || agent.is_empty()
        || !agent
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return Err("agents.paperclip_invalid".into());
    }
    url.set_path(&format!("/api/agents/{agent}"));
    Ok(url)
}
/// Documented GET/PATCH agent configuration API. Preserve all unrelated fields.
pub async fn apply(
    instance: String,
    agent: String,
    credential: String,
    config: Value,
) -> Result<(), String> {
    let url = agent_url(&instance, &agent)?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|_| "agents.paperclip_failed")?;
    // A local trusted Paperclip needs no credential; an empty one sends no header.
    let request = |method| {
        let builder = client.request(method, url.clone());
        if credential.is_empty() {
            builder
        } else {
            builder.bearer_auth(&credential)
        }
    };
    let response = request(reqwest::Method::GET)
        .send()
        .await
        .map_err(|_| "agents.paperclip_failed")?
        .error_for_status()
        .map_err(|_| "agents.paperclip_failed")?;
    if !response.status().is_success() {
        return Err("agents.paperclip_failed".into());
    }
    let current: Value = response
        .json()
        .await
        .map_err(|_| "agents.paperclip_failed")?;
    let adapter = merge_config(&current, &config)?;
    let response = request(reqwest::Method::PATCH)
        .json(&json!({"adapterConfig": adapter}))
        .send()
        .await
        .map_err(|_| "agents.paperclip_failed")?
        .error_for_status()
        .map_err(|_| "agents.paperclip_failed")?;
    if !response.status().is_success() {
        return Err("agents.paperclip_failed".into());
    }
    Ok(())
}
fn merge_config(current: &Value, config: &Value) -> Result<Value, String> {
    if current["adapterType"] != "opencode_local" {
        return Err("agents.paperclip_adapter".into());
    }
    let mut adapter = current["adapterConfig"].clone();
    if !adapter.is_object() {
        return Err("agents.paperclip_failed".into());
    }
    if adapter.get("env").is_some_and(|env| !env.is_object()) {
        return Err("agents.paperclip_failed".into());
    }
    if adapter.get("env").is_none() {
        adapter["env"] = json!({});
    }
    adapter["model"] = config["model"].clone();
    let key = "OPENCODE_CONFIG_CONTENT";
    let mut inline = match adapter["env"].get(key) {
        None => json!({}),
        Some(Value::String(s)) => {
            serde_json::from_str::<Value>(s).map_err(|_| "agents.paperclip_failed")?
        }
        Some(_) => return Err("agents.paperclip_failed".into()),
    };
    if !inline.is_object() {
        return Err("agents.paperclip_failed".into());
    }
    if inline.get("provider").is_some_and(|p| !p.is_object()) {
        return Err("agents.paperclip_failed".into());
    }
    if inline.get("provider").is_none() {
        inline["provider"] = json!({});
    }
    let incoming: Value = serde_json::from_str(
        config["env"][key]
            .as_str()
            .ok_or("agents.paperclip_failed")?,
    )
    .map_err(|_| "agents.paperclip_failed")?;
    inline["provider"]["harness"] = incoming["provider"]["harness"].clone();
    adapter["env"][key] = Value::String(inline.to_string());
    Ok(adapter)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn documented_api_round_trip_and_redirect_refusal() {
        use axum::{body::to_bytes, extract::Request, routing::get, Router};
        use std::sync::{Arc, Mutex};
        let patched = Arc::new(Mutex::new(Value::Null));
        let capture = patched.clone();
        let router = Router::new().route("/api/agents/existing", get(|request: Request| async move {
            assert_eq!(request.headers()["authorization"], "Bearer board-test");
            json!({"adapterType": "opencode_local", "adapterConfig": {"cwd": "/keep", "env": {"KEEP": "yes"}}}).to_string()
        }).patch(move |request: Request| {
            let capture = capture.clone();
            async move {
                assert_eq!(request.headers()["authorization"], "Bearer board-test");
                let bytes = to_bytes(request.into_body(), 65536).await.unwrap();
                *capture.lock().unwrap() = serde_json::from_slice(&bytes).unwrap();
                "{}"
            }
        })).route("/api/agents/redirect", get(|| async {
            (axum::http::StatusCode::TEMPORARY_REDIRECT, [("location", "http://127.0.0.1:1/secret")])
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let config = json!({"model": "harness/chat", "env": {"OPENCODE_CONFIG_CONTENT": "{\"provider\":{\"harness\":{}}}"}});
        let result = apply(
            base.clone(),
            "existing".into(),
            "board-test".into(),
            config.clone(),
        )
        .await;
        let redirect = apply(base, "redirect".into(), "board-test".into(), config).await;
        server.abort();
        let _ = server.await;
        result.unwrap();
        assert_eq!(redirect.unwrap_err(), "agents.paperclip_failed");
        let patched = patched.lock().unwrap();
        assert_eq!(patched["adapterConfig"]["cwd"], "/keep");
        assert_eq!(patched["adapterConfig"]["env"]["KEEP"], "yes");
        assert_eq!(patched["adapterConfig"]["model"], "harness/chat");
    }
    #[test]
    fn apply_preserves_unrelated_config_and_refuses_other_adapters() {
        let current = json!({"adapterType": "opencode_local", "adapterConfig": {
            "cwd": "/project", "toolsets": "file", "env": {"KEEP": "value", "OPENCODE_CONFIG_CONTENT": "{\"permission\":\"ask\",\"provider\":{\"other\":{}}}"}
        }});
        let config = json!({"model": "harness/chat", "env": {
            "OPENCODE_CONFIG_CONTENT": "{\"provider\":{\"harness\":{\"options\":{\"apiKey\":\"new\"}}}}"
        }});
        let merged = merge_config(&current, &config).unwrap();
        assert_eq!(merged["cwd"], "/project");
        assert_eq!(merged["toolsets"], "file");
        assert_eq!(merged["env"]["KEEP"], "value");
        let inline: Value =
            serde_json::from_str(merged["env"]["OPENCODE_CONFIG_CONTENT"].as_str().unwrap())
                .unwrap();
        assert_eq!(inline["permission"], "ask");
        assert!(inline["provider"].get("other").is_some());
        assert_eq!(inline["provider"]["harness"]["options"]["apiKey"], "new");
        assert_eq!(merged["model"], "harness/chat");
        assert!(merge_config(&json!({"adapterType": "http"}), &config).is_err());
    }
    #[test]
    fn link_model_ids_are_matched_exactly_by_the_handoff_token() {
        let dir = tempfile::tempdir().unwrap();
        let tokens = TokenStore::at(&dir.path().join("tokens.json"));
        let model = "remote:lab-box/qwen3:8b";
        generate_in(&tokens, "http://127.0.0.1:11620/v1", model).unwrap();
        let policy = &tokens.list().unwrap()[0].policy;
        assert_eq!(policy.models, vec![model]);
        assert!(policy.allows_model(model) && !policy.allows_model("remote:lab-box/other"));
        assert!(generate_in(&tokens, "http://127.0.0.1:11620/v1", "remote:lab/*").is_err());
    }
    #[test]
    fn token_is_exact_and_revocable_and_urls_cannot_exfiltrate_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let tokens = TokenStore::at(&dir.path().join("tokens.json"));
        let handoff = generate_in(&tokens, "http://127.0.0.1:11435/v1", "chat").unwrap();
        let info = &tokens.list().unwrap()[0];
        assert_eq!(info.scopes, vec![Scope::Inference]);
        assert_eq!(info.policy.models, vec!["chat"]);
        assert_eq!(
            info.policy.endpoints,
            vec![GatewayEndpoint::Chat, GatewayEndpoint::Models]
        );
        let inline: Value = serde_json::from_str(
            handoff.config["env"]["OPENCODE_CONFIG_CONTENT"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            inline["provider"]["harness"]["options"]["baseURL"],
            "http://127.0.0.1:11435/v1"
        );
        tokens.revoke(&handoff.token_id).unwrap();
        assert!(tokens.list().unwrap().is_empty());
        assert!(generate_in(&tokens, "http://localhost/v1", "*").is_err());
        for url in [
            "http://remote.test",
            "https://user:pass@host",
            "https://host/other",
            "https://host/?secret",
        ] {
            assert!(agent_url(url, "agent-1").is_err());
        }
        assert!(agent_url("http://localhost:3100", "../other").is_err());
        assert_eq!(
            agent_url("http://localhost:3100/api", "agent-1")
                .unwrap()
                .path(),
            "/api/agents/agent-1"
        );
    }
}
