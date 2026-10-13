//! Harness ONNX engine: Hugging Face ONNX encoder models (sentence embedders and
//! cross-encoder rerankers) behind an OpenAI-compatible HTTP API, without Python.
//!
//! * [`package`]: the repo layout on disk and what its sentence-transformers files say
//!   (pooling, normalization, maximum length, embedder or reranker). Pure.
//! * [`pooling`]: pooling, normalization and score maths. Pure.
//! * [`api`]: request and response shapes of `/v1/embeddings` and `/v1/rerank`. Pure.
//! * [`genai_pkg`]: the folder of an onnxruntime-genai chat model (`genai_config.json`). Pure.
//! * `chat`, `genai`, `chat_engine` (feature `engine`): chat models on onnxruntime-genai: the
//!   OpenAI chat request and chat templates, the C API binding, the generation loop.
//! * `engine` (feature `engine`): ONNX Runtime session, tokenizer, batching.
//! * `server` (feature `engine`): the HTTP server of the `harness-onnx` sidecar binary.
//!
//! `harness-core` uses only the pure parts (library recognition: is this `.onnx` file a
//! servable encoder, an embedder or a reranker?); the sidecar binary loads ONNX Runtime
//! dynamically from the managed runtime folder.

pub mod api;
pub mod cli;
pub mod genai_pkg;
pub mod hf_api;
pub mod package;
pub mod pooling;
pub mod tasks;

#[cfg(feature = "engine")]
pub mod chat;
#[cfg(feature = "engine")]
pub mod chat_engine;
#[cfg(feature = "engine")]
pub mod engine;
#[cfg(feature = "engine")]
pub mod genai;
#[cfg(feature = "engine")]
pub mod server;
