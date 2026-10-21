//! Embedding and rerank requests through the gateway (`/v1/embeddings`, `/v1/rerank`) by
//! model alias, so the gateway loads the model on demand (JIT) and unloads it by its TTL.

use crate::error_key::err;
use serde_json::{json, Value};
use std::time::Duration;

/// Where requests go: the managed endpoint's `.../v1` and Harness's client token.
#[derive(Debug, Clone)]
pub struct Endpoint {
    pub base_url: String,
    pub token: Option<String>,
}

/// Loading a model on the first request can take a while; the gateway has its own limit.
const TIMEOUT: Duration = Duration::from_secs(600);
const CLIENT_LABEL: &str = "Harness Playground";

fn http() -> reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .timeout(TIMEOUT)
                .no_proxy()
                .build()
                .unwrap_or_default()
        })
        .clone()
}

async fn post(ep: &Endpoint, path: &str, body: Value) -> Result<Value, String> {
    let url = format!("{}/{}", ep.base_url.trim_end_matches('/'), path);
    let mut req = http()
        .post(&url)
        .header("X-Harness-Client", CLIENT_LABEL)
        // Indexing is background work: it queues behind chats and never takes the GPU first.
        .header("X-Harness-Priority", "background")
        .json(&body);
    if let Some(t) = &ep.token {
        req = req.bearer_auth(t);
    }
    send(req, &url, path).await
}

/// A multipart form with one file and text fields (the body of `/v1/audio/transcriptions`).
fn multipart(filename: &str, file: &[u8], fields: &[(&str, &str)]) -> (String, Vec<u8>) {
    let boundary = format!("harness-kb-{:016x}", rand_u64());
    let mut body = Vec::with_capacity(file.len() + 512);
    let name = filename.replace(['"', '\r', '\n'], "_");
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(file);
    body.extend_from_slice(b"\r\n");
    for (key, value) in fields {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{key}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

fn rand_u64() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    std::collections::hash_map::RandomState::new()
        .build_hasher()
        .finish()
}

/// Timed transcript of an audio file through the gateway's `/v1/audio/transcriptions`
/// (`verbose_json`: the text, the language and the segments with their times).
pub async fn transcribe(
    ep: &Endpoint,
    model: &str,
    filename: &str,
    audio: &[u8],
) -> Result<Value, String> {
    let (content_type, body) = multipart(
        filename,
        audio,
        &[
            ("model", model),
            ("response_format", "verbose_json"),
            ("timestamp_granularities[]", "segment"),
        ],
    );
    let url = format!("{}/audio/transcriptions", ep.base_url.trim_end_matches('/'));
    let mut req = http()
        .post(&url)
        .header("X-Harness-Client", CLIENT_LABEL)
        .header("X-Harness-Priority", "background")
        .header("Content-Type", content_type)
        .body(body);
    if let Some(t) = &ep.token {
        req = req.bearer_auth(t);
    }
    send(req, &url, "audio/transcriptions").await
}

async fn send(req: reqwest::RequestBuilder, url: &str, path: &str) -> Result<Value, String> {
    let res = req
        .send()
        .await
        .map_err(|e| crate::http_error::transport(&e, url))?;
    let status = res.status();
    let text = res
        .text()
        .await
        .map_err(|e| crate::http_error::transport(&e, url))?;
    if !status.is_success() {
        let detail = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| {
                v["error"]["message"]
                    .as_str()
                    .or(v["error"].as_str())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| text.chars().take(300).collect());
        return Err(err("rag.request_failed")
            .with("endpoint", path)
            .with("error", format!("HTTP {}: {}", status.as_u16(), detail))
            .into());
    }
    serde_json::from_str(&text).map_err(|e| {
        String::from(
            err("rag.request_failed")
                .with("endpoint", path)
                .with("error", e),
        )
    })
}

/// Embeddings of `input` (in order), each L2-normalized.
pub async fn embed(ep: &Endpoint, model: &str, input: &[String]) -> Result<Vec<Vec<f32>>, String> {
    let v = post(
        ep,
        "embeddings",
        json!({ "model": model, "input": input, "encoding_format": "float" }),
    )
    .await?;
    parse_embeddings(&v, input.len())
        .ok_or_else(|| err("rag.invalid_embeddings").with("model", model).into())
}

pub(crate) fn parse_embeddings(v: &Value, expected: usize) -> Option<Vec<Vec<f32>>> {
    let data = v["data"].as_array()?;
    let mut rows: Vec<(u64, Vec<f32>)> = Vec::with_capacity(data.len());
    for (i, d) in data.iter().enumerate() {
        let emb = d["embedding"].as_array()?;
        let mut vec: Vec<f32> = emb
            .iter()
            .map(|x| x.as_f64().map(|f| f as f32))
            .collect::<Option<_>>()?;
        if vec.is_empty() || vec.iter().any(|x| !x.is_finite()) {
            return None;
        }
        super::search::normalize(&mut vec);
        rows.push((d["index"].as_u64().unwrap_or(i as u64), vec));
    }
    rows.sort_by_key(|(i, _)| *i);
    let dims = rows.first()?.1.len();
    (rows.len() == expected && rows.iter().all(|(_, v)| v.len() == dims))
        .then(|| rows.into_iter().map(|(_, v)| v).collect())
}

/// Text of one page image (PNG) through the gateway's `/v1/ocr` by model alias: the same
/// route and the same transcription prompt as the Read Document panel.
pub async fn ocr(ep: &Endpoint, model: &str, png: &[u8]) -> Result<String, String> {
    ocr_image(ep, model, png, "image/png").await
}

/// Like [`ocr`] for an image of another type (`image/jpeg`, `image/webp`, ...).
pub async fn ocr_image(
    ep: &Endpoint,
    model: &str,
    bytes: &[u8],
    mime: &str,
) -> Result<String, String> {
    use base64::Engine;
    let page = format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    );
    let v = post(ep, "ocr", json!({ "model": model, "pages": [page] })).await?;
    v["markdown"].as_str().map(str::to_string).ok_or_else(|| {
        err("rag.request_failed")
            .with("endpoint", "ocr")
            .with("error", "no text in the answer")
            .into()
    })
}

/// Rerank scores of `documents` for `query`, as `(index, score)` best first.
pub async fn rerank(
    ep: &Endpoint,
    model: &str,
    query: &str,
    documents: &[String],
) -> Result<Vec<(usize, f32)>, String> {
    let v = post(
        ep,
        "rerank",
        json!({ "model": model, "query": query, "documents": documents, "top_n": documents.len() }),
    )
    .await?;
    parse_rerank(&v, documents.len())
        .ok_or_else(|| err("rag.invalid_rerank").with("model", model).into())
}

pub(crate) fn parse_rerank(v: &Value, n: usize) -> Option<Vec<(usize, f32)>> {
    let mut out: Vec<(usize, f32)> = v["results"]
        .as_array()?
        .iter()
        .map(|r| {
            let i = r["index"].as_u64()? as usize;
            let s = r["relevance_score"].as_f64()? as f32;
            (i < n && s.is_finite()).then_some((i, s))
        })
        .collect::<Option<_>>()?;
    out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Answers recorded from llama-server b11236 (nomic-embed-text-v1.5, bge-reranker-v2-m3).
    fn recorded(name: &str) -> Value {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data")
            .join(name);
        serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
    }

    #[test]
    fn parses_recorded_embeddings() {
        let v = recorded("b11236-nomic-embed.v1-embeddings.json");
        let n = v["data"].as_array().unwrap().len();
        let rows = parse_embeddings(&v, n).unwrap();
        assert_eq!(rows.len(), n);
        for r in &rows {
            assert_eq!(r.len(), 768);
            let norm: f32 = r.iter().map(|x| x * x).sum::<f32>().sqrt();
            assert!((norm - 1.0).abs() < 1e-4);
        }
        assert!(parse_embeddings(&v, n + 1).is_none());
        assert!(parse_embeddings(&json!({"data": [{"embedding": null}]}), 1).is_none());
    }

    #[test]
    fn parses_recorded_rerank() {
        let v = recorded("b11236-bge-reranker.rerank.json");
        let n = v["results"].as_array().unwrap().len();
        let r = parse_rerank(&v, n).unwrap();
        assert_eq!(r.len(), n);
        assert!(r.windows(2).all(|w| w[0].1 >= w[1].1));
        assert!(parse_rerank(&v, 0).is_none());
    }
}
