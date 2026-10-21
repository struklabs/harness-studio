//! Opt-in real scanned-PDF OCR: `/v1/ocr` on a scanned and a mixed PDF, then a knowledge base
//! with an OCR model, queried for the recognized text. No downloads happen in the test except
//! the Python engine, which it installs into the isolated data folder when it is missing.
//! Generate the PDFs locally and set HS_OCR_SCAN_PDF (images only), HS_OCR_MIXED_PDF (page 1
//! text layer, page 2 a scan), HS_OCR_WORD (a word on the scanned page), HS_OCR_TEXT_WORD (a word
//! on the text page), HS_TEST_MODEL, HS_TEST_MMPROJ, HS_LLAMA_SERVER and isolated
//! HARNESS_DATA_DIR/HOME/HF_HOME/UV_CACHE_DIR/PIP_CACHE_DIR before running with --ignored.
use base64::{engine::general_purpose::STANDARD, Engine};
use harness_core::{
    domain::{ModelDefinition, ModelRef},
    gateway::GatewaySettings,
    params::ParamValue,
    rag::{self, kb, ocr::OcrModel, RetrieveOptions},
    servers::ServerRegistry,
    state::{NewConversation, StateDb},
};
use harness_gateway::{GatewayContext, GatewayServer};
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Arc, time::Duration};

#[tokio::test]
#[ignore = "requires a local OCR VLM GGUF, projector, llama-server and generated PDF fixtures"]
async fn scanned_and_mixed_pdfs_through_ocr_and_a_knowledge_base() {
    let path = |name| PathBuf::from(std::env::var_os(name).expect(name));
    let text = |name| std::env::var(name).expect(name).to_lowercase();
    let root = path("HARNESS_DATA_DIR");
    std::fs::create_dir_all(&root).unwrap();
    let sandbox = std::fs::canonicalize(std::env::temp_dir().join("ocrpdf-e2e")).unwrap();
    for name in [
        "HARNESS_DATA_DIR",
        "HOME",
        "HF_HOME",
        "UV_CACHE_DIR",
        "PIP_CACHE_DIR",
    ] {
        std::fs::create_dir_all(path(name)).unwrap();
        let real = std::fs::canonicalize(path(name)).unwrap();
        assert!(
            real.starts_with(&sandbox),
            "{name}: {real:?} not under {sandbox:?}"
        );
    }
    harness_core::secrets::use_memory_store();
    harness_core::paths::init(root.clone());

    // The rasterizer lives in the Python engine's environment.
    let env = harness_core::engine::python_hf::env();
    if !harness_core::pyenv::envs::status_in(&harness_core::pyenv::envs::root(), env).ready {
        eprintln!("installing the Python engine environment {} ...", env.name);
        harness_core::pyenv::envs::install(env.name).await.unwrap();
    }

    let mut settings = harness_core::config::AppSettings::load();
    settings.runtime_mode = harness_core::config::RuntimeMode::Custom;
    settings.custom_binary.binary_path = Some(path("HS_LLAMA_SERVER"));
    settings.custom_binary.is_valid = true;
    settings.server.port = 11741;
    settings.control_api.port = 11749;
    settings.save().unwrap();
    let mut def = ModelDefinition::new(
        "ocr-e2e",
        ModelRef::Local {
            path: path("HS_TEST_MODEL"),
        },
    );
    def.mmproj = Some(path("HS_TEST_MMPROJ"));
    def.load
        .insert("context_size".into(), ParamValue::Int(8192));
    def.load.insert("gpu_layers".into(), ParamValue::Int(0));
    def.load.insert("threads".into(), ParamValue::Int(4));
    harness_core::domain::store::Domain::global()
        .save_definition(def)
        .unwrap();

    let registry = Arc::new(ServerRegistry::default());
    let server = GatewayServer::new();
    server.set_strict_port(true);
    let status = server
        .start(
            GatewayContext {
                registry: registry.clone(),
                runtime: Arc::default(),
                gate: Arc::default(),
                data_dir: root.clone(),
                downloads: None,
            },
            &GatewaySettings {
                enabled: true,
                port: 11740,
                request_timeout_secs: 600,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let endpoint = status.endpoint.clone().unwrap();
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(900))
        .no_proxy()
        .build()
        .unwrap();
    let post_pdf = |file: &'static str| {
        let http = http.clone();
        let url = format!("{}/ocr", endpoint.base_url.trim_end_matches('/'));
        let data = format!(
            "data:application/pdf;base64,{}",
            STANDARD.encode(std::fs::read(path(file)).unwrap())
        );
        async move {
            let response = http
                .post(url)
                .json(&json!({"model": "ocr-e2e", "document": data}))
                .send()
                .await
                .unwrap();
            let code = response.status().as_u16();
            let body: Value = response.json().await.unwrap_or(Value::Null);
            eprintln!("POST /v1/ocr {file}: {code} {body}");
            (code, body)
        }
    };

    // 1. A scanned PDF: every page is rendered and recognized by the model.
    let (code, scan) = post_pdf("HS_OCR_SCAN_PDF").await;
    assert_eq!(code, 200, "{scan}");
    assert_eq!(scan["pages"][0]["source"], "vlm");
    assert!(scan["markdown"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains(&text("HS_OCR_WORD")));

    // 2. A mixed PDF: the text layer of page 1, the model for page 2.
    let (code, mixed) = post_pdf("HS_OCR_MIXED_PDF").await;
    assert_eq!(code, 200, "{mixed}");
    assert_eq!(mixed["pages"][0]["page"], 1);
    assert_eq!(mixed["pages"][0]["source"], "pdf_text");
    assert_eq!(mixed["pages"][1]["page"], 2);
    assert_eq!(mixed["pages"][1]["source"], "vlm");
    let markdown = mixed["markdown"].as_str().unwrap().to_lowercase();
    assert!(
        markdown.contains(&text("HS_OCR_TEXT_WORD")) && markdown.contains(&text("HS_OCR_WORD"))
    );

    // 3. A knowledge base with the OCR model: the scanned PDF is indexed and its text found.
    let db = Arc::new(StateDb::open(&root.join("state.db")).unwrap());
    let folder = root.join("kb-docs");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::copy(path("HS_OCR_SCAN_PDF"), folder.join("scan.pdf")).unwrap();
    let base = db
        .kb_create("Scans", Some(&folder.to_string_lossy()))
        .unwrap();
    db.kb_set_ocr_model(&base.id, Some("ocr-e2e")).unwrap();
    let model = OcrModel::gateway(
        rag::Endpoint {
            base_url: endpoint.base_url.clone(),
            token: server.client_token().await,
        },
        "ocr-e2e".into(),
    );
    let flag = kb::kb_jobs().register(&base.id);
    let run = kb::index_kb(
        db.clone(),
        kb::KbIndexRequest {
            kb_id: base.id.clone(),
            files: kb::scan_folder(&folder).unwrap().files,
            root: Some(folder.clone()),
            embed: None,
            ocr: Some(model),
            rebuild: false,
        },
        flag,
        |_| {},
    )
    .await
    .unwrap();
    let docs = db.rag_documents(&base.id).unwrap();
    eprintln!("KB RUN: {run:?}\nKB DOCS: {docs:?}");
    assert_eq!(run.failed, 0);
    assert_eq!(docs[0].status, "ready");
    assert!(!docs[0].ocr_pages.is_empty());
    let chat = db.create_conversation(&NewConversation::default()).unwrap();
    db.kb_attach(&chat.id, &base.id).unwrap();
    let found = rag::prepare(
        &db,
        &chat.id,
        &RetrieveOptions {
            query: text("HS_OCR_WORD"),
            top_k: 3,
            context_tokens: Some(8192),
            embed: None,
            rerank: None,
        },
    )
    .await
    .unwrap();
    eprintln!("KB SOURCES: {:?}", found.sources);
    assert!(found.sources[0]
        .text
        .to_lowercase()
        .contains(&text("HS_OCR_WORD")));

    server.stop().await;
    registry.stop_all().await;
}
