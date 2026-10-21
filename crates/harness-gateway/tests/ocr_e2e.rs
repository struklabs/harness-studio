//! Opt-in real document inference, no downloads in tests. Generate the PNG/PDF locally and set
//! HS_OCR_IMAGE, HS_OCR_PDF, HS_TEST_MODEL, HS_TEST_MMPROJ, HS_LLAMA_SERVER and isolated
//! HARNESS_DATA_DIR/HOME/HF_HOME/UV_CACHE_DIR/PIP_CACHE_DIR before running with --ignored.
use base64::{engine::general_purpose::STANDARD, Engine};
use harness_core::{
    domain::{ModelDefinition, ModelRef},
    gateway::GatewaySettings,
    params::ParamValue,
    servers::ServerRegistry,
};
use harness_gateway::{GatewayContext, GatewayServer};
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Arc, time::Duration};

#[tokio::test]
#[ignore = "requires a local VLM GGUF, projector and generated document fixtures"]
async fn real_document_ocr_qa_and_pdf() {
    let path = |name| PathBuf::from(std::env::var_os(name).expect(name));
    let root = path("HARNESS_DATA_DIR");
    std::fs::create_dir_all(&root).unwrap();
    let sandbox = std::fs::canonicalize(std::env::temp_dir().join("ocr-e2e")).unwrap();
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
    let mut settings = harness_core::config::AppSettings::load();
    settings.runtime_mode = harness_core::config::RuntimeMode::Custom;
    settings.custom_binary.binary_path = Some(path("HS_LLAMA_SERVER"));
    settings.custom_binary.is_valid = true;
    settings.server.port = 11701;
    settings.control_api.port = 11709;
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
    // A general VLM answers questions; OCR-specialised models only transcribe. Optional.
    let qa_model = match (
        std::env::var_os("HS_QA_MODEL"),
        std::env::var_os("HS_QA_MMPROJ"),
    ) {
        (Some(m), Some(p)) => {
            let mut d = ModelDefinition::new("qa-e2e", ModelRef::Local { path: m.into() });
            d.mmproj = Some(p.into());
            d.load.insert("context_size".into(), ParamValue::Int(8192));
            d.load.insert("gpu_layers".into(), ParamValue::Int(0));
            d.load.insert("threads".into(), ParamValue::Int(4));
            harness_core::domain::store::Domain::global()
                .save_definition(d)
                .unwrap();
            "qa-e2e"
        }
        _ => "ocr-e2e",
    };
    let registry = Arc::new(ServerRegistry::default());
    let server = GatewayServer::new();
    server.set_strict_port(true);
    let status = server
        .start(
            GatewayContext {
                registry: registry.clone(),
                runtime: Arc::default(),
                gate: Arc::default(),
                data_dir: root,
                downloads: None,
            },
            &GatewaySettings {
                enabled: true,
                port: 11700,
                request_timeout_secs: 180,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let base = status.endpoint.unwrap().host_url;
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(240))
        .no_proxy()
        .build()
        .unwrap();
    let image = format!(
        "data:image/png;base64,{}",
        STANDARD.encode(std::fs::read(path("HS_OCR_IMAGE")).unwrap())
    );
    let pdf = format!(
        "data:application/pdf;base64,{}",
        STANDARD.encode(std::fs::read(path("HS_OCR_PDF")).unwrap())
    );
    let mut results = Vec::new();
    for (route, model, document, question) in [
        ("/v1/ocr", "ocr-e2e", image.clone(), None),
        (
            "/v1/document/qa",
            qa_model,
            image,
            Some("What is the total?"),
        ),
        ("/v1/ocr", "ocr-e2e", pdf, None),
    ] {
        let mut body = json!({"model":model, "document":document});
        if let Some(q) = question {
            body["question"] = json!(q);
        }
        let result = http.post(format!("{base}{route}")).json(&body).send().await;
        match result {
            Ok(response) => {
                let code = response.status();
                let text = response.text().await.unwrap_or_default();
                eprintln!("POST {route}: {code} {text}");
                results.push((
                    code.as_u16(),
                    serde_json::from_str::<Value>(&text).unwrap_or(Value::Null),
                ));
            }
            Err(e) => {
                eprintln!("POST {route}: {e}");
                results.push((0, Value::Null));
            }
        }
    }
    server.stop().await;
    registry.stop_all().await;
    for (code, body) in &results {
        assert_eq!(*code, 200, "{body}");
    }
    assert!(results[0].1["markdown"].as_str().unwrap().contains("42"));
    assert!(results[1].1["answer"].as_str().unwrap().contains("42"));
    assert_eq!(results[2].1["pages"][0]["source"], "pdf_text");
    assert!(results[2].1["markdown"].as_str().unwrap().contains("42"));
}
