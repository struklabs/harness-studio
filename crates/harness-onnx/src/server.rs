//! The HTTP server of the sidecar: `/health`, `/v1/models`, `/v1/embeddings`, `/v1/rerank` for
//! encoder models and `/v1/chat/completions` for onnxruntime-genai chat models.
//!
//! It binds first and loads the model afterwards, like llama-server: `/health` answers 503
//! while the model loads and 200 once it serves, so Harness's readiness poll works unchanged.

use crate::api::{self, ApiError, EmbeddingsRequest, RerankRequest};
use crate::chat::{self, ChatJob, ChatRequest};
use crate::chat_engine::ChatEngine;
use crate::engine::Engine;
use crate::vision_engine::VisionEngine;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

/// Shared state: the served name and the model once loaded.
pub struct Shared {
    pub alias: String,
    engine: RwLock<Option<Arc<Engine>>>,
    chat: RwLock<Option<Arc<ChatEngine>>>,
    vision: RwLock<Option<Arc<VisionEngine>>>,
    requests: AtomicU64,
}

impl Shared {
    pub fn new(alias: String) -> Arc<Self> {
        Arc::new(Self {
            alias,
            engine: RwLock::new(None),
            chat: RwLock::new(None),
            vision: RwLock::new(None),
            requests: AtomicU64::new(0),
        })
    }

    pub fn set_engine(&self, engine: Engine) {
        *self.engine.write().unwrap() = Some(Arc::new(engine));
    }

    pub fn set_chat(&self, engine: ChatEngine) {
        *self.chat.write().unwrap() = Some(Arc::new(engine));
    }

    pub fn set_vision(&self, engine: VisionEngine) {
        *self.vision.write().unwrap() = Some(Arc::new(engine));
    }

    fn vision(&self) -> Option<Arc<VisionEngine>> {
        self.vision.read().unwrap().clone()
    }

    fn engine(&self) -> Option<Arc<Engine>> {
        self.engine.read().unwrap().clone()
    }

    fn chat(&self) -> Option<Arc<ChatEngine>> {
        self.chat.read().unwrap().clone()
    }

    fn ready(&self) -> bool {
        self.engine().is_some() || self.chat().is_some() || self.vision().is_some()
    }
}

struct Failure(ApiError);

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        let status =
            StatusCode::from_u16(self.0.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        (status, Json(self.0.body())).into_response()
    }
}

impl From<ApiError> for Failure {
    fn from(e: ApiError) -> Self {
        Failure(e)
    }
}

fn loading() -> Failure {
    Failure(ApiError {
        status: 503,
        kind: "unavailable_error",
        message: "Loading model".into(),
    })
}

async fn health(State(s): State<Arc<Shared>>) -> Response {
    if s.ready() {
        Json(json!({"status": "ok"})).into_response()
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"status": "loading", "error": {"message": "Loading model", "code": 503}})),
        )
            .into_response()
    }
}

async fn models(State(s): State<Arc<Shared>>) -> Json<Value> {
    Json(api::models_response(&s.alias))
}

/// Why `/v1/embeddings` and `/v1/rerank` have no model: still loading, or a chat model.
fn not_an_encoder(s: &Shared) -> Failure {
    if s.chat().is_some() {
        Failure(ApiError::invalid(
            "this model is a chat model: use /v1/chat/completions",
        ))
    } else {
        loading()
    }
}

/// Runs `work` on the blocking pool (ONNX Runtime calls block).
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, ApiError> + Send + 'static,
) -> Result<T, Failure> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| Failure(ApiError::server(format!("worker failed: {}", e))))?
        .map_err(Failure)
}

async fn embeddings(
    State(s): State<Arc<Shared>>,
    body: axum::body::Bytes,
) -> Result<Json<Value>, Failure> {
    let engine = s.engine().ok_or_else(|| not_an_encoder(&s))?;
    let request: EmbeddingsRequest = serde_json::from_slice(&body)
        .map_err(|e| Failure(ApiError::invalid(format!("invalid request body: {}", e))))?;
    let job = request.into_job()?;
    let encoding = job.encoding;
    let (vectors, tokens) = blocking(move || engine.embed(&job.texts, job.dimensions)).await?;
    Ok(Json(api::embeddings_response(
        &s.alias, &vectors, encoding, tokens,
    )))
}

/// `POST /hf`: the task heads in the Hugging Face Inference API shape ([`crate::hf_api`]).
async fn hf(State(s): State<Arc<Shared>>, body: axum::body::Bytes) -> Result<Json<Value>, Failure> {
    use crate::hf_api::{self, HfJob};
    let request: Value = serde_json::from_slice(&body)
        .map_err(|e| Failure(ApiError::invalid(format!("invalid request body: {}", e))))?;
    if let Some(vision) = s.vision() {
        let job = crate::vision::parse(&request).map_err(Failure)?;
        let answer = blocking(move || vision.handle(&job)).await?;
        return Ok(Json(answer));
    }
    let engine = s.engine().ok_or_else(|| not_an_encoder(&s))?;
    let job = hf_api::parse(engine.package.task, &request).map_err(Failure)?;
    let answer = blocking(move || {
        Ok(match job {
            HfJob::Classify {
                texts,
                pairs,
                top_k,
                single: _,
            } => {
                let (rows, _) = engine.classify(&texts, pairs.as_deref(), top_k)?;
                hf_api::classify_response(&rows)
            }
            HfJob::ZeroShot {
                text,
                candidates,
                template,
                multi_label,
            } => {
                let (scores, _) = engine.zero_shot(&text, &candidates, &template, multi_label)?;
                hf_api::zero_shot_response(&text, &scores)
            }
            HfJob::Tokens {
                texts,
                group,
                single,
            } => {
                let (entities, _) = engine.token_classify(&texts, group)?;
                hf_api::tokens_response(&entities, &texts, group, single)
            }
            HfJob::FillMask { text, top_k } => {
                let (fills, _) = engine.fill_mask(&text, top_k)?;
                hf_api::fill_mask_response(&fills)
            }
            HfJob::Answer {
                question,
                context,
                top_k,
            } => {
                let (answers, _) = engine.answer(&question, &context, top_k)?;
                hf_api::answer_response(&answers, &context, top_k)
            }
        })
    })
    .await?;
    Ok(Json(answer))
}

async fn rerank(
    State(s): State<Arc<Shared>>,
    body: axum::body::Bytes,
) -> Result<Json<Value>, Failure> {
    let engine = s.engine().ok_or_else(|| not_an_encoder(&s))?;
    let request: RerankRequest = serde_json::from_slice(&body)
        .map_err(|e| Failure(ApiError::invalid(format!("invalid request body: {}", e))))?;
    let job = request.into_job()?;
    let (scores, tokens) = {
        let (query, docs) = (job.query.clone(), job.documents.clone());
        blocking(move || engine.rerank(&query, &docs)).await?
    };
    let results = api::rank(&scores, job.top_n);
    Ok(Json(api::rerank_response(&s.alias, &job, &results, tokens)))
}

fn sse(value: &Value) -> bytes::Bytes {
    bytes::Bytes::from(format!(
        "data: {}

",
        value
    ))
}

fn log_usage(usage: &chat::Usage) {
    println!(
        "harness-onnx: chat: {} prompt tokens in {:.2} s, {} generated tokens at {:.1} tokens/s",
        usage.prompt_tokens,
        usage.prompt_secs,
        usage.completion_tokens,
        usage.tokens_per_second()
    );
}

async fn chat_completions(
    State(s): State<Arc<Shared>>,
    body: axum::body::Bytes,
) -> Result<Response, Failure> {
    let engine = s.chat().ok_or_else(|| {
        if s.engine().is_some() {
            Failure(ApiError::invalid(
                "this model is an embedding or reranking model, not a chat model",
            ))
        } else {
            loading()
        }
    })?;
    let request: ChatRequest = serde_json::from_slice(&body)
        .map_err(|e| Failure(ApiError::invalid(format!("invalid request body: {}", e))))?;
    let job: ChatJob = request.into_job()?;
    let id = format!(
        "chatcmpl-{}",
        s.requests.fetch_add(1, Ordering::Relaxed) + 1
    );
    let alias = s.alias.clone();
    let prepared = {
        let (engine, job) = (engine.clone(), job.clone());
        blocking(move || engine.prepare(&job)).await?
    };

    if !job.stream {
        let (text, finish, usage) = blocking(move || {
            let mut text = String::new();
            let (finish, usage) = engine.run(&prepared, |t| {
                text.push_str(t);
                true
            })?;
            Ok((text, finish, usage))
        })
        .await?;
        log_usage(&usage);
        return Ok(Json(chat::completion_response(
            &id, &alias, &text, finish, &usage,
        ))
        .into_response());
    }

    let (tx, rx) = tokio::sync::mpsc::channel::<bytes::Bytes>(256);
    let include_usage = job.include_usage;
    tokio::task::spawn_blocking(move || {
        let role = chat::chunk(
            &id,
            &alias,
            json!({"role": "assistant", "content": ""}),
            None,
        );
        if tx.blocking_send(sse(&role)).is_err() {
            return;
        }
        let result = engine.run(&prepared, |text| {
            let delta = chat::chunk(&id, &alias, json!({"content": text}), None);
            tx.blocking_send(sse(&delta)).is_ok()
        });
        match result {
            Ok((finish, usage)) => {
                log_usage(&usage);
                let last = chat::chunk(&id, &alias, json!({}), Some(finish));
                let _ = tx.blocking_send(sse(&last));
                if include_usage {
                    let _ = tx.blocking_send(sse(&chat::usage_chunk(&id, &alias, &usage)));
                }
            }
            Err(e) => {
                let _ = tx.blocking_send(sse(&e.body()));
            }
        }
        let _ = tx.blocking_send(bytes::Bytes::from(
            "data: [DONE]

",
        ));
    });
    let stream = futures_util::stream::unfold(rx, |mut rx| async move {
        rx.recv()
            .await
            .map(|b| (Ok::<_, std::convert::Infallible>(b), rx))
    });
    Response::builder()
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .body(axum::body::Body::from_stream(stream))
        .map_err(|e| Failure(ApiError::server(e.to_string())))
}

pub fn router(shared: Arc<Shared>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/models", get(models))
        .route("/models", get(models))
        .route("/v1/embeddings", post(embeddings))
        .route("/embeddings", post(embeddings))
        .route("/v1/chat/completions", post(chat_completions))
        .route("/chat/completions", post(chat_completions))
        .route("/v1/rerank", post(rerank))
        .route("/rerank", post(rerank))
        .route("/v1/reranking", post(rerank))
        .route("/hf", post(hf))
        .route("/reranking", post(rerank))
        .layer(DefaultBodyLimit::max(64 << 20))
        .with_state(shared)
}
