//! Request and response shapes of the HTTP API (pure: no model, no runtime).
//!
//! * `POST /v1/embeddings`: OpenAI's shape. `input` is a string or an array of strings;
//!   `encoding_format` `float` (default) or `base64` (little-endian f32, what the official
//!   SDKs ask for); `dimensions` truncates and renormalizes (Matryoshka models).
//! * `POST /v1/rerank`: the llama-server / Jina / Cohere shape Harness's gateway already
//!   forwards: `{query, documents, top_n}` in, `{results: [{index, relevance_score}]}` out,
//!   best first. The score is the model's raw logit, like llama-server's.

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// A client error: HTTP status, OpenAI-style error `type` and message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    pub status: u16,
    pub kind: &'static str,
    pub message: String,
}

impl ApiError {
    pub fn invalid(message: impl Into<String>) -> Self {
        Self {
            status: 400,
            kind: "invalid_request_error",
            message: message.into(),
        }
    }

    pub fn unavailable(message: impl Into<String>) -> Self {
        Self {
            status: 501,
            kind: "not_supported_error",
            message: message.into(),
        }
    }

    pub fn server(message: impl Into<String>) -> Self {
        Self {
            status: 500,
            kind: "server_error",
            message: message.into(),
        }
    }

    pub fn body(&self) -> Value {
        json!({"error": {"message": self.message, "type": self.kind, "code": self.status}})
    }
}

/// `input` of an embeddings request.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum EmbeddingInput {
    One(String),
    Many(Vec<String>),
    /// Token ids (`[1, 2, 3]` or `[[1, 2], [3]]`): OpenAI allows them, an encoder with its own
    /// tokenizer does not take them.
    TokenIds(Vec<Value>),
}

#[derive(Debug, Deserialize)]
pub struct EmbeddingsRequest {
    #[serde(default)]
    pub model: Option<String>,
    pub input: EmbeddingInput,
    #[serde(default)]
    pub encoding_format: Option<String>,
    #[serde(default)]
    pub dimensions: Option<usize>,
}

/// How the vectors are written into the response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Float,
    Base64,
}

/// An embeddings request after validation.
#[derive(Debug, PartialEq)]
pub struct EmbeddingJob {
    pub texts: Vec<String>,
    pub encoding: Encoding,
    pub dimensions: Option<usize>,
}

impl EmbeddingsRequest {
    pub fn into_job(self) -> Result<EmbeddingJob, ApiError> {
        let texts = match self.input {
            EmbeddingInput::One(s) => vec![s],
            EmbeddingInput::Many(v) => v,
            EmbeddingInput::TokenIds(v) => {
                // An array of non-strings. (An array mixing strings and numbers is also here.)
                return Err(if v.is_empty() {
                    ApiError::invalid("input must not be empty")
                } else {
                    ApiError::invalid(
                        "input as token ids is not supported: send text (the model has its own tokenizer)",
                    )
                });
            }
        };
        if texts.is_empty() {
            return Err(ApiError::invalid("input must not be empty"));
        }
        let encoding = match self.encoding_format.as_deref() {
            None | Some("float") => Encoding::Float,
            Some("base64") => Encoding::Base64,
            Some(other) => {
                return Err(ApiError::invalid(format!(
                    "encoding_format must be \"float\" or \"base64\", not \"{}\"",
                    other
                )))
            }
        };
        if self.dimensions == Some(0) {
            return Err(ApiError::invalid("dimensions must be at least 1"));
        }
        Ok(EmbeddingJob {
            texts,
            encoding,
            dimensions: self.dimensions,
        })
    }
}

/// Little-endian f32 bytes as base64 (the SDKs decode this with `numpy.frombuffer`).
pub fn embedding_base64(v: &[f32]) -> String {
    let mut bytes = Vec::with_capacity(v.len() * 4);
    for x in v {
        bytes.extend_from_slice(&x.to_le_bytes());
    }
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// The `/v1/embeddings` response body.
pub fn embeddings_response(
    model: &str,
    vectors: &[Vec<f32>],
    encoding: Encoding,
    prompt_tokens: usize,
) -> Value {
    let data: Vec<Value> = vectors
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let embedding = match encoding {
                Encoding::Float => json!(v),
                Encoding::Base64 => json!(embedding_base64(v)),
            };
            json!({"object": "embedding", "index": i, "embedding": embedding})
        })
        .collect();
    json!({
        "object": "list",
        "data": data,
        "model": model,
        "usage": {"prompt_tokens": prompt_tokens, "total_tokens": prompt_tokens},
    })
}

/// A document of a rerank request: a string, or `{"text": ...}` (Jina/Cohere objects).
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum RerankDocument {
    Text(String),
    Object { text: String },
}

impl RerankDocument {
    pub fn text(&self) -> &str {
        match self {
            RerankDocument::Text(s) => s,
            RerankDocument::Object { text } => text,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct RerankRequest {
    #[serde(default)]
    pub model: Option<String>,
    pub query: String,
    pub documents: Vec<RerankDocument>,
    #[serde(default)]
    pub top_n: Option<usize>,
    #[serde(default)]
    pub return_documents: Option<bool>,
}

/// A rerank request after validation.
#[derive(Debug, PartialEq)]
pub struct RerankJob {
    pub query: String,
    pub documents: Vec<String>,
    pub top_n: usize,
    pub return_documents: bool,
}

impl RerankRequest {
    pub fn into_job(self) -> Result<RerankJob, ApiError> {
        if self.query.trim().is_empty() {
            return Err(ApiError::invalid("query must not be empty"));
        }
        if self.documents.is_empty() {
            return Err(ApiError::invalid("documents must not be empty"));
        }
        let documents: Vec<String> = self
            .documents
            .iter()
            .map(|d| d.text().to_string())
            .collect();
        let n = documents.len();
        Ok(RerankJob {
            query: self.query,
            top_n: self.top_n.filter(|t| *t > 0).map_or(n, |t| t.min(n)),
            return_documents: self.return_documents.unwrap_or(false),
            documents,
        })
    }
}

#[derive(Debug, Serialize, PartialEq)]
pub struct RerankResult {
    pub index: usize,
    pub relevance_score: f32,
}

/// Scores to results: best first, cut to `top_n`. NaN scores sort last.
pub fn rank(scores: &[f32], top_n: usize) -> Vec<RerankResult> {
    let mut results: Vec<RerankResult> = scores
        .iter()
        .enumerate()
        .map(|(index, &relevance_score)| RerankResult {
            index,
            relevance_score,
        })
        .collect();
    results.sort_by(|a, b| {
        b.relevance_score
            .partial_cmp(&a.relevance_score)
            .unwrap_or_else(|| a.relevance_score.is_nan().cmp(&b.relevance_score.is_nan()))
    });
    results.truncate(top_n);
    results
}

/// The `/v1/rerank` response body.
pub fn rerank_response(
    model: &str,
    job: &RerankJob,
    results: &[RerankResult],
    prompt_tokens: usize,
) -> Value {
    let results: Vec<Value> = results
        .iter()
        .map(|r| {
            let mut o = json!({"index": r.index, "relevance_score": r.relevance_score});
            if job.return_documents {
                o["document"] = json!({"text": job.documents[r.index]});
            }
            o
        })
        .collect();
    json!({
        "model": model,
        "object": "list",
        "usage": {"prompt_tokens": prompt_tokens, "total_tokens": prompt_tokens},
        "results": results,
    })
}

/// `/v1/models` body: the one model this server holds.
pub fn models_response(alias: &str) -> Value {
    json!({
        "object": "list",
        "data": [{"id": alias, "object": "model", "created": 0, "owned_by": "harness-onnx"}],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn emb(v: Value) -> Result<EmbeddingJob, ApiError> {
        serde_json::from_value::<EmbeddingsRequest>(v)
            .map_err(|e| ApiError::invalid(e.to_string()))?
            .into_job()
    }

    #[test]
    fn embeddings_input_forms() {
        let j = emb(json!({"input": "hello"})).unwrap();
        assert_eq!(j.texts, vec!["hello"]);
        assert_eq!(j.encoding, Encoding::Float);
        let j = emb(json!({"model": "m", "input": ["a", "b"], "encoding_format": "base64", "dimensions": 8})).unwrap();
        assert_eq!(j.texts.len(), 2);
        assert_eq!((j.encoding, j.dimensions), (Encoding::Base64, Some(8)));
    }

    #[test]
    fn embeddings_validation() {
        assert!(emb(json!({"input": []})).is_err());
        assert!(emb(json!({"input": [1, 2, 3]}))
            .unwrap_err()
            .message
            .contains("token ids"));
        assert!(emb(json!({"input": [[1, 2], [3]]})).is_err());
        assert!(emb(json!({"input": "a", "encoding_format": "int8"})).is_err());
        assert!(emb(json!({"input": "a", "dimensions": 0})).is_err());
        assert!(emb(json!({"model": "m"})).is_err());
    }

    #[test]
    fn embeddings_response_shapes() {
        let v = vec![vec![1.0f32, 0.5], vec![0.0, 1.0]];
        let r = embeddings_response("minilm", &v, Encoding::Float, 7);
        assert_eq!(r["object"], "list");
        assert_eq!(r["data"][1]["index"], 1);
        assert_eq!(r["data"][0]["embedding"][1], 0.5);
        assert_eq!(r["model"], "minilm");
        assert_eq!(r["usage"]["total_tokens"], 7);

        let r = embeddings_response("minilm", &v, Encoding::Base64, 7);
        let b64 = r["data"][0]["embedding"].as_str().unwrap();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .unwrap();
        assert_eq!(bytes.len(), 8);
        assert_eq!(f32::from_le_bytes(bytes[4..8].try_into().unwrap()), 0.5);
    }

    #[test]
    fn rerank_request_forms() {
        let req: RerankRequest = serde_json::from_value(json!({
            "model": "m", "query": "q",
            "documents": ["a", {"text": "b"}], "top_n": 5, "return_documents": true
        }))
        .unwrap();
        let j = req.into_job().unwrap();
        assert_eq!(j.documents, vec!["a", "b"]);
        assert_eq!(j.top_n, 2, "top_n is cut to the number of documents");
        assert!(j.return_documents);

        let j: RerankJob =
            serde_json::from_value::<RerankRequest>(json!({"query": "q", "documents": ["x"]}))
                .unwrap()
                .into_job()
                .unwrap();
        assert_eq!((j.top_n, j.return_documents), (1, false));

        let bad =
            serde_json::from_value::<RerankRequest>(json!({"query": " ", "documents": ["x"]}))
                .unwrap();
        assert!(bad.into_job().is_err());
        let bad = serde_json::from_value::<RerankRequest>(json!({"query": "q", "documents": []}))
            .unwrap();
        assert!(bad.into_job().is_err());
    }

    #[test]
    fn ranking_is_best_first_and_cut() {
        let r = rank(&[-8.4, 0.08, -11.0, f32::NAN], 3);
        assert_eq!(r.iter().map(|x| x.index).collect::<Vec<_>>(), vec![1, 0, 2]);
        assert_eq!(rank(&[1.0, 2.0], 1)[0].index, 1);
    }

    /// The shape llama-server answers with (recorded in harness-core's test data) and the
    /// optional documents.
    #[test]
    fn rerank_response_matches_llama_server_shape() {
        let job = RerankJob {
            query: "q".into(),
            documents: vec!["a".into(), "b".into()],
            top_n: 2,
            return_documents: false,
        };
        let results = rank(&[0.5, 2.0], 2);
        let r = rerank_response("bge", &job, &results, 11);
        assert_eq!(r["object"], "list");
        assert_eq!(r["results"][0]["index"], 1);
        assert_eq!(r["results"][0]["relevance_score"], 2.0);
        assert!(r["results"][0].get("document").is_none());
        let job = RerankJob {
            return_documents: true,
            ..job
        };
        let r = rerank_response("bge", &job, &results, 11);
        assert_eq!(r["results"][0]["document"]["text"], "b");
        assert_eq!(r["usage"]["prompt_tokens"], 11);
    }

    #[test]
    fn error_body_shape() {
        let b = ApiError::invalid("nope").body();
        assert_eq!(b["error"]["type"], "invalid_request_error");
        assert_eq!(b["error"]["message"], "nope");
    }
}
