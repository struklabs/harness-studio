//! Live LoRA scales of a running llama-server: `GET /lora-adapters` lists the adapters the
//! server loaded (`--lora` / `--lora-scaled`, scale 0 with `--lora-init-without-apply`) and
//! `POST /lora-adapters` sets their scales without a restart.
//!
//! llama-server's POST replaces the whole state: every adapter the body does not name goes back
//! to scale 0. [`set_scales`] therefore reads the current list first and posts all adapters, so
//! changing one id leaves the others as they are.
//!
//! Errors are keyed strings ([`crate::error_key`]): `lora.server_not_running`,
//! `lora.adapter_not_found {id}`, `lora.bad_scale {id}`, `lora.bad_response`,
//! `lora.unsupported {status}`, `lora.request_failed {error}`.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error_key::err;
use crate::process::{ActiveServerInfo, ServerStatus};
use crate::servers::ServerRegistry;

/// One adapter of a running server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct LoraAdapter {
    /// Index llama-server gave the adapter (order of the `--lora` flags).
    pub id: u32,
    pub path: String,
    /// Current scale (0 = loaded but not applied).
    pub scale: f64,
}

/// One scale change.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct LoraScale {
    pub id: u32,
    pub scale: f64,
}

/// Reads the body of `GET /lora-adapters` (an array of `{id, path, scale, ...}`; newer builds
/// add more fields, which are ignored).
pub fn parse_adapters(body: &Value) -> Result<Vec<LoraAdapter>, String> {
    let items = body
        .as_array()
        .ok_or_else(|| String::from(err("lora.bad_response")))?;
    items
        .iter()
        .map(|item| {
            let id = item["id"]
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| String::from(err("lora.bad_response")))?;
            let scale = item["scale"]
                .as_f64()
                .ok_or_else(|| String::from(err("lora.bad_response")))?;
            Ok(LoraAdapter {
                id,
                path: item["path"].as_str().unwrap_or_default().to_string(),
                scale,
            })
        })
        .collect()
}

/// `current` with the given scales applied; unknown ids and non-finite scales are refused.
pub fn apply_scales(
    current: &[LoraAdapter],
    changes: &[LoraScale],
) -> Result<Vec<LoraAdapter>, String> {
    let mut out = current.to_vec();
    for change in changes {
        if !change.scale.is_finite() {
            return Err(err("lora.bad_scale").num("id", change.id).into());
        }
        let adapter = out
            .iter_mut()
            .find(|a| a.id == change.id)
            .ok_or_else(|| String::from(err("lora.adapter_not_found").num("id", change.id)))?;
        adapter.scale = change.scale;
    }
    Ok(out)
}

/// Body of `POST /lora-adapters`: every adapter with its scale.
pub fn request_body(adapters: &[LoraAdapter]) -> Value {
    Value::Array(
        adapters
            .iter()
            .map(|a| json!({"id": a.id, "scale": a.scale}))
            .collect(),
    )
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| err("lora.request_failed").with("error", e).into())
}

fn authed(req: reqwest::RequestBuilder, server: &ActiveServerInfo) -> reqwest::RequestBuilder {
    match &server.api_key {
        Some(k) => req.bearer_auth(k),
        None => req,
    }
}

fn url(server: &ActiveServerInfo) -> String {
    format!(
        "http://{}:{}/lora-adapters",
        server.connect_host, server.port
    )
}

fn check(resp: &reqwest::Response) -> Result<(), String> {
    let status = resp.status();
    if status.is_success() {
        Ok(())
    } else if matches!(status.as_u16(), 404 | 405 | 501) {
        Err(err("lora.unsupported")
            .num("status", status.as_u16())
            .into())
    } else {
        Err(err("lora.request_failed")
            .with("error", format!("HTTP {}", status.as_u16()))
            .into())
    }
}

/// The adapters of a server.
pub async fn list(server: &ActiveServerInfo) -> Result<Vec<LoraAdapter>, String> {
    let resp = authed(client()?.get(url(server)), server)
        .send()
        .await
        .map_err(|e| String::from(err("lora.request_failed").with("error", e)))?;
    check(&resp)?;
    let body: Value = resp
        .json()
        .await
        .map_err(|_| String::from(err("lora.bad_response")))?;
    parse_adapters(&body)
}

/// Applies the scale changes and returns the adapters as the server reports them afterwards.
pub async fn set_scales(
    server: &ActiveServerInfo,
    changes: &[LoraScale],
) -> Result<Vec<LoraAdapter>, String> {
    let merged = apply_scales(&list(server).await?, changes)?;
    let resp = authed(client()?.post(url(server)), server)
        .json(&request_body(&merged))
        .send()
        .await
        .map_err(|e| String::from(err("lora.request_failed").with("error", e)))?;
    check(&resp)?;
    erase_slot_caches(server).await;
    list(server).await
}

/// Drops the prompt caches of the server's slots. llama-server keeps the KV of earlier prompts
/// across a scale change and reuses it for a request with the same prefix: the answer then mixes
/// the old scale (the cached prompt) with the new one (the generated tokens). Best effort: a
/// server without the slots endpoint keeps its cache.
async fn erase_slot_caches(server: &ActiveServerInfo) {
    let Ok(client) = client() else { return };
    let base = format!("http://{}:{}", server.connect_host, server.port);
    let slots: Vec<Value> = match authed(client.get(format!("{base}/slots")), server)
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => r.json().await.unwrap_or_default(),
        _ => return,
    };
    for id in slot_ids(&slots) {
        let _ = authed(
            client.post(format!("{base}/slots/{id}?action=erase")),
            server,
        )
        .send()
        .await;
    }
}

/// Slot ids of a `GET /slots` body.
fn slot_ids(slots: &[Value]) -> Vec<u64> {
    slots
        .iter()
        .filter_map(|s| s.get("id").and_then(Value::as_u64))
        .collect()
}

fn running(registry: &ServerRegistry, id: &str) -> Result<ActiveServerInfo, String> {
    let pm = registry
        .get(id)
        .ok_or_else(|| String::from(err("lora.server_not_running").with("id", id)))?;
    match (pm.get_status(), pm.active_server()) {
        (ServerStatus::Running, Some(server)) => Ok(server),
        _ => Err(err("lora.server_not_running").with("id", id).into()),
    }
}

/// [`list`] for a server instance of the registry (it has to be running).
pub async fn list_for(registry: &ServerRegistry, id: &str) -> Result<Vec<LoraAdapter>, String> {
    list(&running(registry, id)?).await
}

/// [`set_scales`] for a server instance of the registry (it has to be running).
pub async fn set_scales_for(
    registry: &ServerRegistry,
    id: &str,
    changes: &[LoraScale],
) -> Result<Vec<LoraAdapter>, String> {
    set_scales(&running(registry, id)?, changes).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error_key::parse;

    #[test]
    fn slot_ids_of_a_slots_body() {
        let body = [
            json!({"id": 0, "n_ctx": 4096}),
            json!({"id": 3}),
            json!({"x": 1}),
        ];
        assert_eq!(slot_ids(&body), vec![0, 3]);
    }

    fn adapters() -> Vec<LoraAdapter> {
        parse_adapters(&json!([
            {"id": 0, "path": "C:\\lora\\a.gguf", "scale": 0.0},
            {"id": 1, "path": "C:\\lora\\b.gguf", "scale": 1.0, "task_name": "x", "prompt_prefix": ""}
        ]))
        .unwrap()
    }

    #[test]
    fn parses_the_server_list() {
        let a = adapters();
        assert_eq!(a.len(), 2);
        assert_eq!(a[1].id, 1);
        assert_eq!(a[1].path, "C:\\lora\\b.gguf");
        assert_eq!(a[1].scale, 1.0);
        assert!(parse_adapters(&json!([])).unwrap().is_empty());
    }

    #[test]
    fn refuses_a_malformed_list() {
        for bad in [
            json!({"error": "x"}),
            json!([{"path": "a", "scale": 1.0}]),
            json!([{"id": 0, "path": "a"}]),
            json!([{"id": -1, "path": "a", "scale": 1.0}]),
        ] {
            let e = parse_adapters(&bad).unwrap_err();
            assert_eq!(parse(&e).unwrap().0, "lora.bad_response");
        }
    }

    #[test]
    fn changing_one_adapter_keeps_the_others() {
        let merged = apply_scales(&adapters(), &[LoraScale { id: 0, scale: 0.55 }]).unwrap();
        assert_eq!(merged[0].scale, 0.55);
        assert_eq!(merged[1].scale, 1.0);
        assert_eq!(
            request_body(&merged),
            json!([{"id": 0, "scale": 0.55}, {"id": 1, "scale": 1.0}])
        );
    }

    #[test]
    fn unknown_ids_and_bad_scales_are_refused() {
        let e = apply_scales(&adapters(), &[LoraScale { id: 7, scale: 1.0 }]).unwrap_err();
        let (key, params) = parse(&e).unwrap();
        assert_eq!(key, "lora.adapter_not_found");
        assert_eq!(params["id"], 7);
        let e = apply_scales(
            &adapters(),
            &[LoraScale {
                id: 0,
                scale: f64::NAN,
            }],
        )
        .unwrap_err();
        assert_eq!(parse(&e).unwrap().0, "lora.bad_scale");
    }
}
