//! Knowledge bases through the real gateway with the real recommended embedding model
//! (multilingual-e5-small, CPU): a folder with PDF, Markdown and text files in two languages is
//! scanned, chunked, embedded and indexed; a question in one language finds the document in
//! the other; the base goes stale when another embedding model is in use.
//!
//! ```text
//! HS_LLAMA_SERVER=<llama-server>
//! HS_TEST_EMBED_MODEL=<multilingual-e5-small-q8_0.gguf>
//! cargo test -p harness-gateway --test kb_e2e -- --ignored --nocapture --test-threads=1
//! ```
//!
//! Temporary data root (never the user's), in-memory secrets, gateway on 19870 and its model on
//! the port right after it. The test stops what it started.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use harness_core::domain::{ModelDefinition, ModelRef};
use harness_core::gateway::GatewaySettings;
use harness_core::rag::kb::{self, KbIndexRequest, KbState};
use harness_core::rag::{self, EmbedModel, Endpoint, RetrieveOptions};
use harness_core::servers::ServerRegistry;
use harness_core::state::{NewConversation, StateDb};
use harness_gateway::{GatewayContext, GatewayServer};

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
            .prefix(&harness_core::paths::test_dir_prefix("kb-e2e"))
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

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../harness-core/tests/data/rag")
        .join(rel)
}

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn assert_best_similarity(p: &harness_core::rag::Prepared, doc: &str) {
    let best = p
        .sources
        .iter()
        .filter(|s| s.similarity.is_some())
        .max_by(|a, b| a.similarity.partial_cmp(&b.similarity).unwrap())
        .unwrap();
    assert_eq!(best.document_name, doc, "{:?}", p.sources);
}

#[tokio::test]
#[ignore]
async fn e2e_knowledge_base_multilingual_cpu_index_and_retrieval() {
    let (Some(bin), Some(model)) = (env_path("HS_LLAMA_SERVER"), env_path("HS_TEST_EMBED_MODEL"))
    else {
        return;
    };
    let root = data_root(&bin).to_path_buf();

    // The definition the app makes for the installed model: on the CPU.
    let mut def = ModelDefinition::new("e5-small", ModelRef::Local { path: model });
    assert!(rag::embedder::apply_cpu_default(&mut def));
    let def = harness_core::domain::store::Domain::global()
        .save_definition(def)
        .unwrap();
    let defs = harness_core::domain::store::Domain::global()
        .definitions()
        .unwrap();
    let models = rag::models::retrieval_models(&defs);
    eprintln!("retrieval models: {models:?}");
    let m = models.iter().find(|m| m.alias == "e5-small").unwrap();
    assert_eq!(m.role, rag::models::RetrievalRole::Embedding);
    assert_eq!(m.dims, Some(rag::embedder::DIMS));

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
    let embed = EmbedModel::for_definition(
        Endpoint {
            base_url: base,
            token: None,
        },
        &def,
    );
    // e5 takes "query: " / "passage: " and has a 511-token context.
    assert_eq!(embed.query_prefix, "query: ");
    assert_eq!(embed.doc_prefix, "passage: ");
    assert_eq!(embed.max_chars, Some(1022));

    // A folder: the real PDF and two repository documents, a Turkish and an English note, and
    // files the scan must leave out.
    let folder = tempfile::tempdir().unwrap();
    let f = folder.path();
    std::fs::copy(fixture("state-db-schema.pdf"), {
        std::fs::create_dir_all(f.join("docs")).unwrap();
        f.join("docs/state-db-schema.pdf")
    })
    .unwrap();
    std::fs::copy(
        fixture("distractors/SECURITY.md"),
        f.join("docs/SECURITY.md"),
    )
    .unwrap();
    std::fs::copy(
        fixture("distractors/PERFORMANCE.md"),
        f.join("docs/PERFORMANCE.md"),
    )
    .unwrap();
    write(
        f,
        "notes/photosynthesis.md",
        "# Photosynthesis\n\nPlants capture sunlight with chlorophyll in their leaves. They use the light energy to turn carbon dioxide and water into glucose and release oxygen. This process is the base of almost every food chain on Earth.",
    );
    write(
        f,
        "notes/yemek.txt",
        "Mercimek çorbası için önce soğanı ve havucu yağda kavurun, sonra kırmızı mercimeği ekleyip su ile pişirin. Çorbayı blenderdan geçirip limon ve pul biberle servis edin.",
    );
    write(f, "node_modules/x/readme.md", "# ignored");
    write(f, "logo.png", "png");

    let db = Arc::new(StateDb::open(&root.join("state.db")).unwrap());
    let scan = kb::scan_folder(f).unwrap();
    eprintln!("scan: {} files, {} skipped", scan.files.len(), scan.skipped);
    assert_eq!(scan.files.len(), 5);
    let row = db
        .kb_create("E2E Folder", Some(&f.to_string_lossy()))
        .unwrap();
    let flag = kb::kb_jobs().register(&row.id);
    let events = Arc::new(Mutex::new(Vec::new()));
    let ev = events.clone();
    let run = kb::index_kb(
        db.clone(),
        KbIndexRequest {
            kb_id: row.id.clone(),
            files: scan.files,
            root: Some(f.to_path_buf()),
            embed: Some(embed.clone()),
            ocr: None,
            rebuild: false,
        },
        flag,
        move |p| ev.lock().unwrap().push(p),
    )
    .await
    .unwrap();
    eprintln!(
        "INDEX: {} docs, {} chunks, {} embedded in {} ms ({:.1} chunks/s) with {:?} {:?} dims",
        run.documents,
        run.chunks,
        run.embedded,
        run.elapsed_ms,
        run.chunks as f64 / (run.elapsed_ms.max(1) as f64 / 1000.0),
        run.model,
        run.dims
    );
    for d in db.rag_documents(&row.id).unwrap() {
        eprintln!(
            "doc {}: {} chunks {} embedded status {} {:?}",
            d.name, d.chunks, d.embedded, d.status, d.error
        );
    }
    let progress = events.lock().unwrap().clone();
    eprintln!("progress events: {}", progress.len());
    assert_eq!(run.failed, 0);
    assert_eq!(run.documents, 5);
    assert_eq!(run.chunks, run.embedded);
    assert_eq!(run.model.as_deref(), Some("e5-small"));
    assert_eq!(run.dims, Some(384));
    assert_eq!(progress.last().unwrap().status, "done");
    let summary = kb::get(&db, &row.id, Some("e5-small")).unwrap();
    assert_eq!(summary.state, KbState::Ready);
    assert_eq!(summary.row.embed_dims, Some(384));
    assert!(summary.row.index_ms > 0);

    // Retrieval through an attached base; questions in Turkish find English text and the
    // other way round.
    let chat = db
        .create_conversation(&NewConversation::default())
        .unwrap()
        .id;
    db.kb_attach(&chat, &row.id).unwrap();
    let ask = |q: &'static str| {
        let db = db.clone();
        let chat = chat.clone();
        let embed = embed.clone();
        async move {
            let t = std::time::Instant::now();
            let p = rag::prepare(
                &db,
                &chat,
                &RetrieveOptions {
                    query: q.into(),
                    top_k: 3,
                    context_tokens: Some(4096),
                    embed: Some(embed),
                    rerank: None,
                },
            )
            .await
            .unwrap();
            eprintln!(
                "Q ({} ms, {} tokens): {q}\n  -> {:?}",
                t.elapsed().as_millis(),
                p.tokens,
                p.sources
                    .iter()
                    .map(|s| (s.document_name.clone(), s.similarity))
                    .collect::<Vec<_>>()
            );
            assert_eq!(p.retrieval.as_deref(), Some("hybrid"));
            assert!(p.notices.is_empty(), "{:?}", p.notices);
            p
        }
    };
    let tr_to_en = ask("Bitkiler güneş ışığını nasıl enerjiye dönüştürür?").await;
    // The right document has the best dense similarity of all excerpts and is among the cited
    // ones; keyword noise of a cross-language question may still rank another excerpt first.
    assert_best_similarity(&tr_to_en, "notes/photosynthesis.md");
    let en_to_tr = ask("How do I cook red lentil soup?").await;
    assert_best_similarity(&en_to_tr, "notes/yemek.txt");
    let pdf =
        ask("state.db şemasında v3 migration hangi tabloyu ekler, birincil anahtarı nedir?").await;
    assert_eq!(pdf.sources[0].document_name, "docs/state-db-schema.pdf");
    assert!(
        pdf.tokens <= rag::kb::token_budget(4096) + 400,
        "{}",
        pdf.tokens
    );

    // Another embedding model in use: the base is stale and a notice says so.
    let stale = kb::get(&db, &row.id, Some("other-embedder")).unwrap();
    assert_eq!(stale.state, KbState::Stale);
    assert_eq!(stale.stale_reason.as_deref(), Some("model"));

    server.stop().await;
    registry.stop_all().await;
}
