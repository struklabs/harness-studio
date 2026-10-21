//! Which model definitions can embed or rerank, and the task prefixes embedding models
//! expect.
//!
//! A definition qualifies by its GGUF header (`domain::detect_kind`: pooling type 1–3 =
//! embedding; RANK pooling, classifier labels or a classification head = reranker), the
//! same classification the library and the gateway's serve mode use, or, for an ONNX file,
//! by its repo files ([`harness_onnx::package`]: sentence-transformers modules = embedding,
//! a sequence-classification head = reranker), or, for a safetensors folder of the Python
//! engine, by what the worker would run it as ([`crate::engine::python_hf::detect`]: a
//! sentence-transformers folder = embedding, a one-label classification head = reranker).
//! Definitions whose file is not downloaded
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

/// The BYOK cloud embedding models (`cloud:<provider>/<model>`): the knowledge bases may embed
/// with them instead of a local model. They need no file; the id marks the index.
pub fn cloud_embedding_models() -> Vec<RetrievalModel> {
    crate::cloud::embedding_models()
        .into_iter()
        .map(|m| RetrievalModel {
            definition_id: m.id.clone(),
            alias: m.id,
            role: RetrievalRole::Embedding,
            file: m.model,
            dims: None,
            context_length: m.info.context_length,
        })
        .collect()
}

/// Embedding and rerank definitions, in definition order.
pub fn retrieval_models(defs: &[ModelDefinition]) -> Vec<RetrievalModel> {
    defs.iter()
        .filter_map(|d| {
            let path = d.base.local_path()?;
            if let Some(model) = onnx_model(d, path).or_else(|| mlx_model(d, path)) {
                return Some(model);
            }
            if let Some(model) = python_hf_model(d, path) {
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

/// An MLX embedder or reranker folder (the `mlx-embed` engine).
fn mlx_model(def: &ModelDefinition, path: &std::path::Path) -> Option<RetrievalModel> {
    let (task, dims, context_length) = crate::engine::mlx::retrieval_info(path)?;
    let role = match task {
        crate::engine::mlx::RetrievalTask::Embedding => RetrievalRole::Embedding,
        crate::engine::mlx::RetrievalTask::Rerank => RetrievalRole::Rerank,
    };
    Some(RetrievalModel {
        definition_id: def.id.clone(),
        alias: def.alias.clone(),
        file: path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
        dims: dims.filter(|_| role == RetrievalRole::Embedding),
        context_length,
        role,
    })
}

/// A safetensors embedder or cross-encoder folder of the Python engine, read from its configs.
fn python_hf_model(def: &ModelDefinition, path: &std::path::Path) -> Option<RetrievalModel> {
    let role = match crate::engine::python_hf::detect(path)?.as_str() {
        "feature-extraction" | "sentence-similarity" => RetrievalRole::Embedding,
        "text-ranking" => RetrievalRole::Rerank,
        _ => return None,
    };
    let json = |name: &str| -> Option<serde_json::Value> {
        serde_json::from_str(&std::fs::read_to_string(path.join(name)).ok()?).ok()
    };
    let config = json("config.json");
    let number = |v: Option<&serde_json::Value>, key: &str| v?.get(key)?.as_u64();
    // The pooling module names the vector size (a `2_Dense` module can change it; the model
    // size is then not known here and the index takes it from the embeddings).
    let pooling = (0..4).find_map(|i| json(&format!("{i}_Pooling/config.json")));
    let has_dense = std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().ends_with("_Dense"));
    let dims = (role == RetrievalRole::Embedding && !has_dense)
        .then(|| {
            number(pooling.as_ref(), "word_embedding_dimension")
                .or_else(|| number(config.as_ref(), "hidden_size"))
        })
        .flatten();
    let context = number(json("sentence_bert_config.json").as_ref(), "max_seq_length")
        .or_else(|| number(config.as_ref(), "max_position_embeddings"));
    Some(RetrievalModel {
        definition_id: def.id.clone(),
        alias: def.alias.clone(),
        file: path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
        dims,
        context_length: context,
        role,
    })
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
        // Classifiers, NER, fill-mask and QA heads do not retrieve.
        _ => return None,
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

    fn folder(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (name, text) in files {
            let path = dir.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        dir
    }

    fn def(alias: &str, dir: &std::path::Path) -> ModelDefinition {
        ModelDefinition::new(
            alias,
            crate::domain::ModelRef::Local {
                path: dir.to_path_buf(),
            },
        )
    }

    #[test]
    fn safetensors_embedders_and_cross_encoders_of_the_python_engine_retrieve() {
        // A sentence-transformers folder without an `onnx/` export.
        let st = folder(&[
            (
                "config.json",
                r#"{"architectures":["BertModel"],"hidden_size":384,"max_position_embeddings":512}"#,
            ),
            ("modules.json", "[]"),
            (
                "1_Pooling/config.json",
                r#"{"word_embedding_dimension":384}"#,
            ),
            ("sentence_bert_config.json", r#"{"max_seq_length":256}"#),
            ("model.safetensors", "x"),
        ]);
        // A cross-encoder whose card says text-classification (as most of them do).
        let ce = folder(&[
            (
                "config.json",
                r#"{"architectures":["XLMRobertaForSequenceClassification"],"id2label":{"0":"LABEL_0"},"max_position_embeddings":514}"#,
            ),
            (
                "README.md",
                "---
library_name: sentence-transformers
pipeline_tag: text-classification
---
",
            ),
            ("model.safetensors", "x"),
        ]);
        let classifier = folder(&[
            (
                "config.json",
                r#"{"architectures":["BertForSequenceClassification"],"id2label":{"0":"NEG","1":"POS"}}"#,
            ),
            ("model.safetensors", "x"),
        ]);
        let ner = folder(&[
            (
                "config.json",
                r#"{"architectures":["BertForTokenClassification"]}"#,
            ),
            ("model.safetensors", "x"),
        ]);
        let defs = vec![
            def("st", st.path()),
            def("ce", ce.path()),
            def("sst", classifier.path()),
            def("ner", ner.path()),
        ];
        let found = retrieval_models(&defs);
        let by = |alias: &str| found.iter().find(|m| m.alias == alias);
        let embed = by("st").expect("the embedder is a retrieval model");
        assert_eq!(embed.role, RetrievalRole::Embedding);
        assert_eq!(embed.dims, Some(384));
        assert_eq!(embed.context_length, Some(256));
        let rerank = by("ce").expect("the cross-encoder is one too");
        assert_eq!(rerank.role, RetrievalRole::Rerank);
        assert_eq!(rerank.dims, None);
        assert_eq!(rerank.context_length, Some(514));
        // Classifiers and taggers do not retrieve.
        assert!(by("sst").is_none() && by("ner").is_none());
    }

    #[test]
    fn a_dense_module_hides_the_pooling_size() {
        let st = folder(&[
            (
                "config.json",
                r#"{"architectures":["BertModel"],"hidden_size":768}"#,
            ),
            ("modules.json", "[]"),
            (
                "1_Pooling/config.json",
                r#"{"word_embedding_dimension":768}"#,
            ),
            ("2_Dense/config.json", r#"{"out_features":512}"#),
            ("model.safetensors", "x"),
        ]);
        let found = retrieval_models(&[def("dense", st.path())]);
        assert_eq!(found[0].role, RetrievalRole::Embedding);
        assert_eq!(found[0].dims, None);
    }

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
