//! Which model definitions can embed or rerank, and the task prefixes embedding models
//! expect.
//!
//! A definition qualifies by its GGUF header (`domain::detect_kind`: pooling type 1–3 =
//! embedding; RANK pooling, classifier labels or a classification head = reranker), the
//! same classification the library and the gateway's serve mode use, or, for an ONNX file,
//! by its repo files ([`harness_onnx::package`]: sentence-transformers modules = embedding,
//! a sequence-classification head = reranker). Definitions whose file is not downloaded
//! (`hf:`) or unreadable are left out.

use crate::domain::{detect_kind, ModelDefinition, ModelKind};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalRole {
    Embedding,
    Rerank,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetrievalModel {
    pub definition_id: String,
    pub alias: String,
    pub role: RetrievalRole,
    /// File name of the GGUF.
    pub file: String,
    /// Embedding size from the header (embedding models).
    pub dims: Option<u64>,
    /// Training context from the header.
    pub context_length: Option<u64>,
}

/// Embedding and rerank definitions, in definition order.
pub fn retrieval_models(defs: &[ModelDefinition]) -> Vec<RetrievalModel> {
    defs.iter()
        .filter_map(|d| {
            let path = d.base.local_path()?;
            if let Some(model) = onnx_model(d, path) {
                return Some(model);
            }
            let meta = crate::gguf::read_gguf_metadata(path).ok()?;
            let role = match detect_kind(Some(&meta)) {
                ModelKind::Embedding => RetrievalRole::Embedding,
                ModelKind::Reranker => RetrievalRole::Rerank,
                _ => return None,
            };
            Some(RetrievalModel {
                definition_id: d.id.clone(),
                alias: d.alias.clone(),
                file: path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
                dims: (role == RetrievalRole::Embedding)
                    .then_some(meta.embedding_length)
                    .flatten(),
                context_length: meta.context_length,
                role,
            })
        })
        .collect()
}

/// An ONNX encoder definition (harness-onnx), read from the repo files around its graph.
fn onnx_model(def: &ModelDefinition, path: &std::path::Path) -> Option<RetrievalModel> {
    if crate::engine::ModelFormat::of(path) != crate::engine::ModelFormat::Onnx {
        return None;
    }
    let package = harness_onnx::package::ModelPackage::discover(path).ok()?;
    let role = match package.task {
        harness_onnx::package::Task::Embedding => RetrievalRole::Embedding,
        harness_onnx::package::Task::Rerank => RetrievalRole::Rerank,
    };
    Some(RetrievalModel {
        definition_id: def.id.clone(),
        alias: def.alias.clone(),
        file: path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
        dims: (role == RetrievalRole::Embedding)
            .then_some(package.dimension.map(|d| d as u64))
            .flatten(),
        context_length: Some(package.max_length as u64),
        role,
    })
}

/// Query and document prefixes an embedding model was trained with, from its file name,
/// alias or architecture (model cards): nomic-embed `search_query: ` / `search_document: `,
/// E5 `query: ` / `passage: `, mxbai / snowflake-arctic / bge English v1.5 query
/// instruction, EmbeddingGemma `task: search result | query: ` / `title: none | text: `,
/// Qwen3-Embedding instruct query. Others (bge-m3, gte, jina...) take plain text.
pub fn prefixes(name: &str, architecture: Option<&str>) -> (&'static str, &'static str) {
    let n = name.to_lowercase();
    if n.contains("nomic-embed") || architecture == Some("nomic-bert") {
        ("search_query: ", "search_document: ")
    } else if n.contains("e5-") || n.starts_with("e5") {
        ("query: ", "passage: ")
    } else if n.contains("embeddinggemma") || n.contains("embedding-gemma") {
        ("task: search result | query: ", "title: none | text: ")
    } else if n.contains("qwen3-embedding") {
        (
            "Instruct: Given a question, retrieve passages that answer the question\nQuery: ",
            "",
        )
    } else if n.contains("mxbai-embed")
        || n.contains("snowflake-arctic-embed")
        || (n.contains("bge-") && n.contains("-en") && n.contains("v1.5"))
    {
        (
            "Represent this sentence for searching relevant passages: ",
            "",
        )
    } else {
        ("", "")
    }
}

/// Prefixes of a definition (reads its GGUF header for the architecture).
pub fn prefixes_for(def: &ModelDefinition) -> (&'static str, &'static str) {
    let path = def.base.local_path();
    // An ONNX file is called `model.onnx` in every repo: the repo folder names the model.
    let repo = path
        .filter(|p| crate::engine::ModelFormat::of(p) == crate::engine::ModelFormat::Onnx)
        .and_then(harness_onnx::package::find_root)
        .and_then(|root| root.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_default();
    let file = path
        .and_then(|p| p.file_name())
        .map(|n| format!("{} {}", n.to_string_lossy(), repo))
        .unwrap_or_default();
    let meta = path.and_then(|p| crate::gguf::read_gguf_metadata(p).ok());
    let arch = meta.as_ref().and_then(|m| m.architecture.clone());
    let name = format!(
        "{} {} {}",
        file,
        def.alias,
        meta.and_then(|m| m.model_name).unwrap_or_default()
    );
    prefixes(&name, arch.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_prefixes() {
        assert_eq!(
            prefixes("nomic-embed-text-v1.5.Q8_0.gguf", None),
            ("search_query: ", "search_document: ")
        );
        assert_eq!(
            prefixes("my-alias", Some("nomic-bert")).1,
            "search_document: "
        );
        assert_eq!(
            prefixes("multilingual-e5-large-Q8_0.gguf", Some("bert")),
            ("query: ", "passage: ")
        );
        assert_eq!(prefixes("bge-m3-Q8_0.gguf", Some("bert")), ("", ""));
        assert!(prefixes("mxbai-embed-large-v1-f16.gguf", None)
            .0
            .starts_with("Represent"));
        assert!(prefixes("bge-small-en-v1.5-q8_0.gguf", None)
            .0
            .starts_with("Represent"));
        assert_eq!(
            prefixes("embeddinggemma-300M-Q8_0.gguf", Some("gemma-embedding")).1,
            "title: none | text: "
        );
        assert!(prefixes("Qwen3-Embedding-0.6B-Q8_0.gguf", Some("qwen3"))
            .0
            .starts_with("Instruct:"));
    }
}
