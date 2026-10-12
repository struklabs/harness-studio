//! Local RAG (Playground documents) through the real gateway with real models (no mocks):
//! a real PDF is extracted, chunked and embedded by an embedding definition the gateway
//! loads on demand; a question whose answer is only in that PDF retrieves the right chunk
//! (hybrid search + reranker) and the chat model answers from it, citing the excerpt.
//!
//! ```text
//! HS_LLAMA_SERVER=<llama-server>
//! HS_TEST_EMBED_MODEL=<nomic-embed-text-v1.5 gguf>
//! HS_TEST_RERANK_MODEL=<bge-reranker-v2-m3 gguf>
//! HS_TEST_MODEL=<chat gguf, e.g. Qwen3-4B-Instruct-2507 Q4_K_M>
//! cargo test -p harness-gateway --test rag_e2e -- --ignored --nocapture --test-threads=1
//! ```
//!
//! Temporary data root (never the user's), in-memory secrets, gateway on 19870 and its models
//! on the ports right after it. The test stops what it started.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use harness_core::domain::{ModelDefinition, ModelRef};
use harness_core::gateway::GatewaySettings;
use harness_core::params::ParamValue;
use harness_core::rag::{self, EmbedModel, Endpoint, IndexJob, RetrieveOptions};
use harness_core::servers::ServerRegistry;
use harness_core::state::{NewConversation, StateDb};
use harness_gateway::{GatewayContext, GatewayServer};
use serde_json::{json, Value};

fn env_path(name: &str) -> Option<PathBuf> {
    let p = std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|p| p.is_file());
    if p.is_none() {
        eprintln!("{name} not set, skipping");
    }
    p
}

fn data_root(bin: &Path) -> &'static Path {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        harness_core::secrets::use_memory_store();
        let dir = tempfile::Builder::new()
            .prefix(&harness_core::paths::test_dir_prefix("rag-e2e"))
            .tempdir()
            .unwrap()
            .keep();
        harness_core::paths::init(dir.clone());
        let mut s = harness_core::config::AppSettings::load();
        s.runtime_mode = harness_core::config::RuntimeMode::Custom;
        s.custom_binary.binary_path = Some(bin.to_path_buf());
        s.custom_binary.is_valid = true;
        s.server.port = 19890;
        s.control_api.port = 19898;
        s.save().unwrap();
        dir
    })
}

fn define(alias: &str, model: &Path, ctx: i64) -> ModelDefinition {
    let mut d = ModelDefinition::new(alias, ModelRef::Local { path: model.into() });
    d.load.insert("gpu_layers".into(), ParamValue::Int(99));
    d.load.insert("context_size".into(), ParamValue::Int(ctx));
    harness_core::domain::store::Domain::global()
        .save_definition(d)
        .unwrap()
}

/// RAG fixtures of harness-core. The distractors are frozen copies of two repository documents:
/// the live `docs/*.md` keep growing, which would change the ranking (the right chunk could fall
/// out of the candidate pool).
fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../harness-core/tests/data/rag")
        .join(rel)
}

async fn chat(http: &reqwest::Client, base: &str, model: &str, messages: Value) -> String {
    let res = http
        .post(format!("{base}/chat/completions"))
        .header("x-harness-client", "Harness Playground")
        .json(&json!({
            "model": model,
            "messages": messages,
            "temperature": 0,
            "max_tokens": 200,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let v: Value = res.json().await.unwrap();
    v["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

#[tokio::test]
#[ignore]
async fn e2e_rag_pdf_retrieval_rerank_and_cited_answer() {
    let (Some(bin), Some(embed_gguf), Some(rerank_gguf), Some(chat_gguf)) = (
        env_path("HS_LLAMA_SERVER"),
        env_path("HS_TEST_EMBED_MODEL"),
        env_path("HS_TEST_RERANK_MODEL"),
        env_path("HS_TEST_MODEL"),
    ) else {
        return;
    };
    let root = data_root(&bin).to_path_buf();
    let embed_def = define("rag-embed", &embed_gguf, 2048);
    define("rag-rerank", &rerank_gguf, 4096);
    define("rag-chat", &chat_gguf, 8192);

    // The definitions are recognized by their GGUF headers.
    let defs = harness_core::domain::store::Domain::global()
        .definitions()
        .unwrap();
    let models = harness_core::rag::models::retrieval_models(&defs);
    eprintln!("retrieval models: {models:?}");
    assert!(models.iter().any(|m| m.alias == "rag-embed"
        && m.role == harness_core::rag::models::RetrievalRole::Embedding
        && m.dims == Some(768)));
    assert!(models
        .iter()
        .any(|m| m.alias == "rag-rerank"
            && m.role == harness_core::rag::models::RetrievalRole::Rerank));
    assert!(!models.iter().any(|m| m.alias == "rag-chat"));

    let registry = Arc::new(ServerRegistry::default());
    let ctx = GatewayContext {
        registry: registry.clone(),
        runtime: Arc::new(harness_core::runtime::RuntimeManager::default()),
        gate: Arc::new(harness_core::operation_gate::OperationGate::default()),
        data_dir: root.clone(),
        downloads: None,
    };
    let server = GatewayServer::new();
    let status = server
        .start(
            ctx,
            &GatewaySettings {
                enabled: true,
                port: 19870,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let base = status.endpoint.unwrap().base_url;
    let endpoint = Endpoint {
        base_url: base.clone(),
        token: None,
    };
    let embed = EmbedModel::for_definition(endpoint.clone(), &embed_def);
    assert_eq!(embed.query_prefix, "search_query: ");
    assert_eq!(embed.doc_prefix, "search_document: ");

    let db = Arc::new(StateDb::open(&root.join("state.db")).unwrap());
    let conv = db
        .create_conversation(&NewConversation {
            title: Some("rag e2e".into()),
            ..Default::default()
        })
        .unwrap()
        .id;

    // The PDF (answer on page 2) and two real repository documents that do not contain it.
    let files = [
        fixture("state-db-schema.pdf"),
        fixture("distractors/SECURITY.md"),
        fixture("distractors/PERFORMANCE.md"),
    ];
    let t_index = Instant::now();
    let mut pdf_id = String::new();
    for f in &files {
        let doc = db
            .rag_create_document(
                &conv,
                &rag::extract::file_name(f),
                Some(&f.to_string_lossy()),
            )
            .unwrap();
        if pdf_id.is_empty() {
            pdf_id = doc.id.clone();
        }
        let flag = rag::jobs().register(&doc.id);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s = seen.clone();
        let done = rag::run_job(
            db.clone(),
            IndexJob {
                document_id: doc.id.clone(),
                source: Some(f.clone()),
                embed: Some(embed.clone()),
            },
            flag,
            move |p| s.lock().unwrap().push((p.status, p.done, p.total)),
        )
        .await
        .unwrap();
        eprintln!(
            "indexed {}: {} chunks, {} embedded, dims {:?}, progress {:?}",
            done.name,
            done.chunks,
            done.embedded,
            done.embed_dims,
            seen.lock().unwrap()
        );
        assert_eq!(done.status, "ready");
        assert_eq!(done.embedded, done.chunks);
        assert_eq!(done.embed_dims, Some(768));
        assert_eq!(done.embed_model.as_deref(), Some("rag-embed"));
        let last = seen.lock().unwrap().last().cloned().unwrap();
        assert_eq!(
            last,
            (
                "ready".to_string(),
                done.chunks as usize,
                done.chunks as usize
            )
        );
    }
    let index_ms = t_index.elapsed().as_millis();
    let total_chunks: i64 = db
        .rag_documents(&conv)
        .unwrap()
        .iter()
        .map(|d| d.chunks)
        .sum();
    eprintln!(
        "indexing (incl. JIT load of the embedding model): {index_ms} ms, {total_chunks} chunks"
    );

    // A small context forces retrieval; the reranker orders the fused candidates.
    let question = "Harness Studio'nun state.db şemasında v3 migration'ı hangi tabloyu ekler ve bu tablonun birincil anahtarı nedir?";
    let t_ret = Instant::now();
    let prepared = rag::prepare(
        &db,
        &conv,
        &RetrieveOptions {
            query: question.into(),
            top_k: 4,
            context_tokens: Some(2048),
            embed: Some(embed.clone()),
            rerank: Some((endpoint.clone(), "rag-rerank".into())),
        },
    )
    .await
    .unwrap();
    let retrieve_ms = t_ret.elapsed().as_millis();
    for s in &prepared.sources {
        eprintln!(
            "[{}] {} p{:?} sim {:?} rerank {:?}: {}",
            s.n,
            s.document_name,
            s.page_start,
            s.similarity,
            s.rerank_score,
            s.text
                .chars()
                .take(90)
                .collect::<String>()
                .replace('\n', " ")
        );
    }
    eprintln!(
        "retrieval (incl. JIT load of the reranker): {retrieve_ms} ms, notices {:?}",
        prepared.notices
    );
    assert_eq!(prepared.retrieval.as_deref(), Some("hybrid"));
    assert_eq!(prepared.reranked.as_deref(), Some("rag-rerank"));
    assert!(prepared.notices.is_empty(), "{:?}", prepared.notices);
    assert_eq!(prepared.system, None);
    let top = &prepared.sources[0];
    assert_eq!(top.document_id, pdf_id);
    assert_eq!(top.page_start, Some(2));
    assert!(
        top.text
            .contains("vram_measurements(model_key, settings_key"),
        "{}",
        top.text
    );
    // The reranker's score always; the dense similarity only when the chunk was among the
    // nearest vectors (it can come from the keyword list alone).
    assert!(top.rerank_score.is_some());
    let cite = top.n;

    // The chat model answers from the excerpts and cites the right one.
    let http = reqwest::Client::new();
    let t_chat = Instant::now();
    let answer = chat(
        &http,
        &base,
        "rag-chat",
        json!([{ "role": "user", "content": prepared.user.clone().unwrap() }]),
    )
    .await;
    eprintln!("answer ({} ms): {answer}", t_chat.elapsed().as_millis());
    assert!(answer.contains("vram_measurements"), "{answer}");
    assert!(
        answer.contains("model_key") && answer.contains("settings_key"),
        "{answer}"
    );
    assert!(
        answer.contains(&format!("[{cite}]")),
        "cites [{cite}]: {answer}"
    );

    // Without the document the model cannot know the table (the answer is only in the PDF).
    let blind = chat(
        &http,
        &base,
        "rag-chat",
        json!([{ "role": "user", "content": question }]),
    )
    .await;
    eprintln!("answer without documents: {blind}");
    assert!(!blind.contains("vram_measurements"), "{blind}");

    // Full text: with only the PDF in "full" mode and a large context, it goes in whole as a
    // system message and the answer still comes from it.
    for d in db.rag_documents(&conv).unwrap() {
        if d.id != pdf_id {
            db.rag_delete_document(&d.id).unwrap();
        }
    }
    db.rag_set_mode(&pdf_id, "full").unwrap();
    let full = rag::prepare(
        &db,
        &conv,
        &RetrieveOptions {
            query: question.into(),
            top_k: 4,
            context_tokens: Some(8192),
            embed: Some(embed.clone()),
            rerank: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(full.user, None);
    assert!(full.sources.len() == 1 && full.sources[0].full);
    let answer = chat(
        &http,
        &base,
        "rag-chat",
        json!([
            { "role": "system", "content": full.system.unwrap() },
            { "role": "user", "content": question }
        ]),
    )
    .await;
    eprintln!("full-text answer: {answer}");
    assert!(answer.contains("vram_measurements"), "{answer}");

    server.stop().await;
    registry.stop_all().await;
}
