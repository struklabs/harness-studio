//! Lemonade's HTTP API as Harness uses it.
//!
//! Checked against `lemond` 2026.41.1 (endpoints on both `/v1/...` and `/api/v1/...`):
//! `GET /live` (no key), `GET /v1/health`, `GET /v1/models[?show_all=true]`,
//! `GET /v1/system-info`, `POST /v1/pull` (SSE progress), `POST /v1/delete`, `POST /v1/load`,
//! `POST /v1/unload`, `POST /v1/install` (a backend, SSE progress) and `POST /internal/shutdown`.
//! The chat, embeddings, rerank and transcription routes are OpenAI-compatible.
//! With `LEMONADE_API_KEY` set every route except `/live` wants `Authorization: Bearer <key>`.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error_key::err;

/// Where a running `lemond` listens and the key it wants.
#[derive(Debug, Clone)]
pub struct Endpoint {
    /// `http://127.0.0.1:<port>`.
    pub base: String,
    pub key: String,
}

/// What a Lemonade model does, from its labels (and recipe).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum ModelKind {
    Chat,
    Embedding,
    Rerank,
    Transcription,
    Speech,
    Image,
    /// Audio / 3D generation, classifiers...: listed, not routed by the gateway.
    Other,
}

impl ModelKind {
    /// The gateway serves this kind (chat, embeddings, rerank, transcriptions).
    pub fn routable(self) -> bool {
        matches!(
            self,
            Self::Chat | Self::Embedding | Self::Rerank | Self::Transcription
        )
    }
}

/// One model of Lemonade's registry (`GET /v1/models?show_all=true`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct LemonadeModel {
    /// The name clients use (`Qwen3-0.6B-GGUF`, `user.my-model`).
    pub id: String,
    /// `llamacpp`, `whispercpp`, `flm`, `ryzenai-llm`, `sd-cpp`, `kokoro`...
    pub recipe: String,
    /// Hugging Face reference (`unsloth/Qwen3-0.6B-GGUF:Q4_0`).
    pub checkpoint: String,
    pub labels: Vec<String>,
    pub kind: ModelKind,
    /// Download size in GB, as Lemonade lists it.
    pub size_gb: f64,
    pub downloaded: bool,
    pub suggested: bool,
    pub update_available: bool,
}

/// Kind of a model from its labels; the recipe settles models without a clear label.
pub fn kind_of(recipe: &str, labels: &[String]) -> ModelKind {
    let has = |l: &str| labels.iter().any(|x| x == l);
    if has("embeddings") {
        ModelKind::Embedding
    } else if has("reranking") {
        ModelKind::Rerank
    } else if has("transcription") {
        ModelKind::Transcription
    } else if has("tts") {
        ModelKind::Speech
    } else if has("image") {
        ModelKind::Image
    } else if has("chat") || recipe == "llamacpp" || recipe == "flm" || recipe == "ryzenai-llm" {
        ModelKind::Chat
    } else {
        ModelKind::Other
    }
}

/// The models of a `/v1/models` answer.
pub fn parse_models(v: &Value) -> Vec<LemonadeModel> {
    let Some(items) = v.get("data").and_then(Value::as_array) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|m| {
            let id = m.get("id")?.as_str()?.to_string();
            let recipe = m
                .get("recipe")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let labels: Vec<String> = m
                .get("labels")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|l| l.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            Some(LemonadeModel {
                kind: kind_of(&recipe, &labels),
                checkpoint: m
                    .get("checkpoint")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                size_gb: m.get("size").and_then(Value::as_f64).unwrap_or(0.0),
                downloaded: m
                    .get("downloaded")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                suggested: m.get("suggested").and_then(Value::as_bool).unwrap_or(false),
                update_available: m
                    .get("update_available")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                id,
                recipe,
                labels,
            })
        })
        .collect()
}

/// A backend Lemonade can run models on, from `GET /v1/system-info` `recipes`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BackendInfo {
    /// Recipe: `llamacpp`, `flm`, `whispercpp`...
    pub recipe: String,
    /// `vulkan`, `rocm`, `cpu`, `npu`...
    pub backend: String,
    /// `installed`, `installable`, `update_required`, `unsupported`...
    pub state: String,
    /// Release tag Lemonade pins for this backend (`b10825`).
    pub version: Option<String>,
    /// Why it is not usable on this machine (`Unsupported GPU`).
    pub message: Option<String>,
}

/// Recipes whose backends the Harness card shows: the llama.cpp builds, whisper.cpp and the
/// FastFlowLM / Ryzen AI NPU backends.
const SHOWN_RECIPES: &[&str] = &["llamacpp", "whispercpp", "flm", "ryzenai-llm"];

/// The backends of a `/v1/system-info` answer (the recipes Harness shows).
pub fn parse_backends(v: &Value) -> Vec<BackendInfo> {
    let Some(recipes) = v.get("recipes").and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for recipe in SHOWN_RECIPES {
        let Some(backends) = recipes
            .get(*recipe)
            .and_then(|r| r.get("backends"))
            .and_then(Value::as_object)
        else {
            continue;
        };
        for (name, b) in backends {
            let text = |k: &str| {
                b.get(k)
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
            };
            out.push(BackendInfo {
                recipe: recipe.to_string(),
                backend: name.clone(),
                state: text("state").unwrap_or_else(|| "unsupported".into()),
                version: text("version"),
                message: text("message"),
            });
        }
    }
    out
}

/// Progress of a model pull or a backend install (the SSE `progress` events of lemond).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PullProgress {
    /// What is being fetched (model name or `recipe:backend`).
    pub target: String,
    /// File being downloaded.
    pub file: String,
    pub file_index: u32,
    pub total_files: u32,
    pub bytes_downloaded: u64,
    pub bytes_total: u64,
    /// Size of the whole pull when lemond knows it (0 = unknown).
    pub total_download_size: u64,
    pub percent: f64,
}

/// A progress event's JSON.
pub fn parse_progress(target: &str, data: &Value) -> PullProgress {
    let n = |k: &str| data.get(k).and_then(Value::as_u64).unwrap_or(0);
    PullProgress {
        target: target.to_string(),
        file: data
            .get("file")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        file_index: n("file_index") as u32,
        total_files: n("total_files") as u32,
        bytes_downloaded: n("bytes_downloaded"),
        bytes_total: n("bytes_total"),
        total_download_size: n("total_download_size"),
        percent: data.get("percent").and_then(Value::as_f64).unwrap_or(0.0),
    }
}

/// Server-sent events of a response body: `(event, data)` pairs, complete ones only. The rest
/// of `buf` (an unfinished event) stays for the next chunk.
pub fn take_events(buf: &mut String) -> Vec<(String, String)> {
    let mut out = Vec::new();
    loop {
        let normalized = buf.replace("\r\n", "\n");
        let Some(end) = normalized.find("\n\n") else {
            *buf = normalized;
            return out;
        };
        let block = &normalized[..end];
        let mut event = String::from("message");
        let mut data = String::new();
        for line in block.lines() {
            if let Some(v) = line.strip_prefix("event:") {
                event = v.trim().to_string();
            } else if let Some(v) = line.strip_prefix("data:") {
                if !data.is_empty() {
                    data.push('\n');
                }
                data.push_str(v.trim_start());
            }
        }
        out.push((event, data));
        *buf = normalized[end + 2..].to_string();
    }
}

fn http() -> reqwest::Client {
    // Loopback only: never through a system proxy; no overall timeout (a pull takes minutes).
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .no_proxy()
        .build()
        .unwrap_or_default()
}

fn url(ep: &Endpoint, path: &str) -> String {
    format!("{}{}", ep.base.trim_end_matches('/'), path)
}

fn transport(e: reqwest::Error, ep: &Endpoint) -> String {
    crate::http_error::transport(&e, &ep.base)
}

/// The message of an error body (`{"error":"..."}` or `{"error":{"message":"..."}}`).
pub fn error_message(status: u16, body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            let e = v.get("error")?;
            e.as_str()
                .map(str::to_string)
                .or_else(|| e.get("message")?.as_str().map(str::to_string))
        })
        .unwrap_or_else(|| {
            format!(
                "HTTP {status} {}",
                body.trim().chars().take(200).collect::<String>()
            )
        })
}

async fn get_json(ep: &Endpoint, path: &str, timeout: Duration) -> Result<Value, String> {
    let res = http()
        .get(url(ep, path))
        .bearer_auth(&ep.key)
        .timeout(timeout)
        .send()
        .await
        .map_err(|e| transport(e, ep))?;
    let status = res.status().as_u16();
    let body = res.text().await.unwrap_or_default();
    if !(200..300).contains(&status) {
        return Err(err("lemonade.api_error")
            .with("detail", error_message(status, &body))
            .into());
    }
    serde_json::from_str(&body).map_err(|e| {
        err("lemonade.api_error")
            .with("detail", format!("{path}: {e}"))
            .into()
    })
}

async fn post_json(
    ep: &Endpoint,
    path: &str,
    body: &Value,
    timeout: Duration,
) -> Result<Value, String> {
    let res = http()
        .post(url(ep, path))
        .bearer_auth(&ep.key)
        .timeout(timeout)
        .json(body)
        .send()
        .await
        .map_err(|e| transport(e, ep))?;
    let status = res.status().as_u16();
    let text = res.text().await.unwrap_or_default();
    if !(200..300).contains(&status) {
        return Err(err("lemonade.api_error")
            .with("detail", error_message(status, &text))
            .into());
    }
    Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
}

/// `GET /live`: the process answers (no key needed).
pub async fn live(base: &str) -> bool {
    http()
        .get(format!("{}/live", base.trim_end_matches('/')))
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .is_ok_and(|r| r.status().is_success())
}

/// `GET /v1/health`.
pub async fn health(ep: &Endpoint) -> Result<Value, String> {
    get_json(ep, "/v1/health", Duration::from_secs(10)).await
}

/// `GET /v1/models?show_all=true`: the registry with the downloaded flag.
pub async fn models(ep: &Endpoint) -> Result<Vec<LemonadeModel>, String> {
    let v = get_json(ep, "/v1/models?show_all=true", Duration::from_secs(30)).await?;
    Ok(parse_models(&v))
}

/// `GET /v1/system-info`: the backends and their state.
pub async fn backends(ep: &Endpoint) -> Result<Vec<BackendInfo>, String> {
    let v = get_json(ep, "/v1/system-info", Duration::from_secs(30)).await?;
    Ok(parse_backends(&v))
}

/// `POST /v1/delete`.
pub async fn delete(ep: &Endpoint, model: &str) -> Result<(), String> {
    post_json(
        ep,
        "/v1/delete",
        &serde_json::json!({ "model_name": model }),
        Duration::from_secs(60),
    )
    .await
    .map(|_| ())
}

/// `POST /v1/load`.
pub async fn load(ep: &Endpoint, model: &str) -> Result<(), String> {
    post_json(
        ep,
        "/v1/load",
        &serde_json::json!({ "model_name": model }),
        Duration::from_secs(600),
    )
    .await
    .map(|_| ())
}

/// `POST /v1/unload` (every model when `model` is `None`).
pub async fn unload(ep: &Endpoint, model: Option<&str>) -> Result<(), String> {
    let body = match model {
        Some(m) => serde_json::json!({ "model_name": m }),
        None => serde_json::json!({}),
    };
    post_json(ep, "/v1/unload", &body, Duration::from_secs(60))
        .await
        .map(|_| ())
}

/// `POST /internal/shutdown`.
pub async fn shutdown(ep: &Endpoint) -> Result<(), String> {
    post_json(
        ep,
        "/internal/shutdown",
        &serde_json::json!({}),
        Duration::from_secs(10),
    )
    .await
    .map(|_| ())
}

/// A streaming POST (`/v1/pull`, `/v1/install`): calls `on_progress` for every progress event
/// and fails with lemond's message on an `error` event.
async fn stream_post(
    ep: &Endpoint,
    path: &str,
    body: &Value,
    target: &str,
    mut on_progress: impl FnMut(PullProgress),
) -> Result<(), String> {
    use futures_util::StreamExt;
    let res = http()
        .post(url(ep, path))
        .bearer_auth(&ep.key)
        .json(body)
        .send()
        .await
        .map_err(|e| transport(e, ep))?;
    let status = res.status().as_u16();
    if !(200..300).contains(&status) {
        let text = res.text().await.unwrap_or_default();
        return Err(err("lemonade.api_error")
            .with("detail", error_message(status, &text))
            .into());
    }
    let mut stream = res.bytes_stream();
    let mut buf = String::new();
    let mut complete = false;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| transport(e, ep))?;
        buf.push_str(&String::from_utf8_lossy(&chunk));
        for (event, data) in take_events(&mut buf) {
            let json: Value = serde_json::from_str(&data).unwrap_or(Value::Null);
            match event.as_str() {
                "progress" => on_progress(parse_progress(target, &json)),
                "complete" => {
                    on_progress(parse_progress(target, &json));
                    complete = true;
                }
                "error" => {
                    return Err(err("lemonade.api_error")
                        .with("detail", error_message(500, &data))
                        .into())
                }
                _ => {}
            }
        }
    }
    if complete {
        Ok(())
    } else {
        Err(err("lemonade.pull_incomplete")
            .with("target", target)
            .into())
    }
}

/// `POST /v1/pull`: downloads a registry model (or a `owner/repo[:variant]` checkpoint).
pub async fn pull(
    ep: &Endpoint,
    model: &str,
    on_progress: impl FnMut(PullProgress),
) -> Result<(), String> {
    stream_post(
        ep,
        "/v1/pull",
        &serde_json::json!({ "model_name": model, "stream": true }),
        model,
        on_progress,
    )
    .await
}

/// `POST /v1/install`: installs a backend (`llamacpp` + `rocm`).
pub async fn install_backend(
    ep: &Endpoint,
    recipe: &str,
    backend: &str,
    on_progress: impl FnMut(PullProgress),
) -> Result<(), String> {
    stream_post(
        ep,
        "/v1/install",
        &serde_json::json!({ "recipe": recipe, "backend": backend, "stream": true }),
        &format!("{recipe}:{backend}"),
        on_progress,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data")
            .join(name);
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn models_of_the_real_registry_answer() {
        // `GET /v1/models?show_all=true` of lemond 2026.41.1 (a few of its 127 entries).
        let models = parse_models(&fixture("lemonade-models.json"));
        assert!(models.len() >= 5);
        let qwen = models.iter().find(|m| m.id == "Qwen3-0.6B-GGUF").unwrap();
        assert_eq!(qwen.recipe, "llamacpp");
        assert_eq!(qwen.kind, ModelKind::Chat);
        assert_eq!(qwen.checkpoint, "unsloth/Qwen3-0.6B-GGUF:Q4_0");
        assert!(qwen.suggested && !qwen.downloaded);
        assert!((qwen.size_gb - 0.38).abs() < 1e-9);
        let kind = |id: &str| models.iter().find(|m| m.id == id).unwrap().kind;
        assert_eq!(kind("nomic-embed-text-v1-GGUF"), ModelKind::Embedding);
        assert_eq!(kind("bge-reranker-v2-m3-GGUF"), ModelKind::Rerank);
        assert_eq!(kind("Whisper-Tiny"), ModelKind::Transcription);
        assert_eq!(kind("kokoro-v1"), ModelKind::Speech);
        assert!(!kind("kokoro-v1").routable());
        assert!(kind("Whisper-Tiny").routable());
        assert!(parse_models(&serde_json::json!({})).is_empty());
    }

    #[test]
    fn kinds_follow_labels_then_recipe() {
        let l = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(kind_of("flm", &l(&["chat"])), ModelKind::Chat);
        assert_eq!(kind_of("flm", &[]), ModelKind::Chat);
        assert_eq!(
            kind_of("onnxruntime", &l(&["classification"])),
            ModelKind::Other
        );
        assert_eq!(kind_of("sd-cpp", &l(&["image"])), ModelKind::Image);
        assert_eq!(
            kind_of("acestep", &l(&["audio-generation"])),
            ModelKind::Other
        );
    }

    #[test]
    fn backends_of_the_real_system_info() {
        // A desktop without an NPU: Vulkan and ROCm llama.cpp installable, FastFlowLM unsupported.
        let b = parse_backends(&fixture("lemonade-system-info.json"));
        let find = |r: &str, k: &str| b.iter().find(|x| x.recipe == r && x.backend == k).unwrap();
        assert_eq!(find("llamacpp", "vulkan").state, "installable");
        assert_eq!(
            find("llamacpp", "vulkan").version.as_deref(),
            Some("b10825")
        );
        assert_eq!(find("llamacpp", "rocm").state, "installable");
        assert_eq!(find("flm", "npu").state, "unsupported");
        assert_eq!(find("flm", "npu").version.as_deref(), Some("v1.0.6"));
        assert!(b.iter().all(|x| x.recipe != "sd-cpp"));
    }

    #[test]
    fn sse_events_are_split_across_chunks() {
        let mut buf = String::new();
        buf.push_str("event: progress\ndata: {\"percent\":10}\n\nevent: prog");
        let first = take_events(&mut buf);
        assert_eq!(first, vec![("progress".into(), "{\"percent\":10}".into())]);
        buf.push_str("ress\r\ndata: {\"percent\":50}\r\n\r\nevent: complete\ndata: {}\n\n");
        let rest = take_events(&mut buf);
        assert_eq!(rest.len(), 2);
        assert_eq!(rest[0].0, "progress");
        assert_eq!(rest[1].0, "complete");
        assert!(buf.is_empty());
    }

    #[test]
    fn progress_events_and_error_bodies() {
        let p = parse_progress(
            "Qwen3-0.6B-GGUF",
            &serde_json::json!({"bytes_downloaded":752,"bytes_total":752,"file":"config.json","file_index":2,"percent":100,"total_download_size":382157232,"total_files":2}),
        );
        assert_eq!(p.file, "config.json");
        assert_eq!((p.file_index, p.total_files), (2, 2));
        assert_eq!(p.total_download_size, 382_157_232);
        assert_eq!(
            error_message(500, "{\"error\":\"Failed to open file\"}"),
            "Failed to open file"
        );
        assert_eq!(
            error_message(
                404,
                "{\"error\":{\"message\":\"no such model\",\"type\":\"not_found\"}}"
            ),
            "no such model"
        );
        assert!(error_message(502, "oops").starts_with("HTTP 502"));
    }
}
