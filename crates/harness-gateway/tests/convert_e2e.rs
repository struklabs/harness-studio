//! Safetensors to GGUF end to end, real everything: the Hugging Face download, the
//! managed conversion environment (uv, Python, torch), the official converter, the managed
//! runtime's `llama-quantize`, and a chat answer from the converted model through the gateway.
//!
//! ```text
//! HS_MANAGED_RUNTIME=<a managed llama.cpp runtime folder, e.g. <data>/bin/managed>
//! HS_CONVERT_E2E_ROOT=<optional: a folder to keep between runs (the environment is ~700 MB)>
//! cargo test -p harness-gateway --test convert_e2e -- --ignored --nocapture --test-threads=1
//! ```
//!
//! Temporary data root (never the user's), in-memory secrets. The test model is
//! `HuggingFaceTB/SmolLM2-135M-Instruct` (270 MB of safetensors).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use harness_core::convert::{self, ConvertStage, ConvertStatus};
use harness_core::domain::{ModelDefinition, ModelRef};
use harness_core::gateway::GatewaySettings;
use harness_core::params::ParamValue;
use harness_core::servers::ServerRegistry;
use harness_gateway::{GatewayContext, GatewayServer};
use serde_json::{json, Value};

const REPO: &str = "HuggingFaceTB/SmolLM2-135M-Instruct";

fn managed_runtime() -> Option<PathBuf> {
    let p = std::env::var_os("HS_MANAGED_RUNTIME")
        .map(PathBuf::from)
        .filter(|p| p.join(".harness-runtime.json").is_file());
    if p.is_none() {
        eprintln!("HS_MANAGED_RUNTIME not set (a managed runtime folder), skipping");
    }
    p
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let dest = to.join(e.file_name());
        if e.path().is_dir() {
            copy_dir(&e.path(), &dest);
        } else {
            std::fs::copy(e.path(), dest).unwrap();
        }
    }
}

/// A data root with the managed runtime copied in.
fn data_root(runtime: &Path) -> PathBuf {
    harness_core::secrets::use_memory_store();
    let dir = match std::env::var_os("HS_CONVERT_E2E_ROOT") {
        Some(d) => {
            let d = PathBuf::from(d);
            std::fs::create_dir_all(&d).unwrap();
            d
        }
        None => tempfile::Builder::new()
            .prefix(&harness_core::paths::test_dir_prefix("convert-e2e"))
            .tempdir()
            .unwrap()
            .keep(),
    };
    harness_core::paths::init(dir.clone());
    let managed = harness_core::paths::bin_dir().join("managed");
    if !managed.join(".harness-runtime.json").is_file() {
        copy_dir(runtime, &managed);
    }
    let mut settings = harness_core::config::AppSettings::load();
    settings.control_api.port = 19989;
    settings.save().unwrap();
    dir
}

/// Waits for a job to leave the active states, printing every stage change.
async fn wait_for(id: u64, limit: Duration) -> convert::ConvertJob {
    let started = Instant::now();
    let mut last = (ConvertStage::Queued, String::new());
    loop {
        let job = convert::job(id).expect("the job exists");
        let now = (job.stage, format!("{:?}", job.status));
        if now != (last.0, last.1.clone()) {
            eprintln!(
                "[{:>4}s] {:?} / {:?} {:?} {}",
                started.elapsed().as_secs(),
                job.status,
                job.stage,
                job.progress,
                job.detail
            );
            last = now;
        }
        if !job.status.is_active() {
            return job;
        }
        assert!(
            started.elapsed() < limit,
            "the conversion took too long: {job:?}"
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

#[tokio::test]
#[ignore = "downloads ~1 GB (uv, Python, torch, the model) and needs HS_MANAGED_RUNTIME"]
async fn converts_a_safetensors_repo_and_chats_through_the_gateway() {
    let Some(runtime) = managed_runtime() else {
        return;
    };
    let dir = data_root(&runtime);
    let models = harness_core::config::AppSettings::load().primary_models_dir();

    // The plan names the files, sizes and the conversion's disk need before anything runs.
    let plan = convert::plan(REPO, None).await.unwrap();
    eprintln!(
        "plan: {:?} {} params, {} bytes to download, convertible {}",
        plan.architecture, plan.parameters, plan.download_bytes, plan.convertible
    );
    assert!(plan.convertible, "{:?}", plan.reason);
    assert_eq!(plan.architecture.as_deref(), Some("LlamaForCausalLM"));
    assert!(plan.architectures_checked);
    assert!(plan.files.iter().any(|f| f.name == "model.safetensors"));
    assert!(plan.estimate("Q4_K_M").size_bytes < plan.estimate("F16").size_bytes);

    // An unsupported architecture is refused before any download.
    let none = convert::plan("openai-community/gpt2-xl-does-not-exist-here", None).await;
    assert!(none.is_err(), "{none:?}");

    // The whole pipeline.
    let job = convert::start(REPO, Some("Q4_K_M"), false).await.unwrap();
    let job = wait_for(job.id, Duration::from_secs(40 * 60)).await;
    assert_eq!(job.status, ConvertStatus::Completed, "{job:?}");
    assert_eq!(job.stage, ConvertStage::Done);
    let output = PathBuf::from(job.output.clone().expect("an output path"));
    eprintln!("converted: {}", output.display());
    assert_eq!(
        output,
        models
            .join("HuggingFaceTB")
            .join("SmolLM2-135M-Instruct-GGUF")
            .join("SmolLM2-135M-Instruct-Q4_K_M.gguf")
    );
    let size = std::fs::metadata(&output).unwrap().len();
    assert!(size > 50_000_000 && size < 150_000_000, "{size}");
    let meta = harness_core::gguf::read_gguf_metadata(&output).unwrap();
    assert_eq!(meta.architecture.as_deref(), Some("llama"));
    // llama-quantize files Q4_K_M as ftype 15.
    assert_eq!(meta.file_type, Some(15), "{}", meta.quantization);

    // The work folder (downloaded safetensors, the 16-bit intermediate) is gone.
    assert!(!models
        .join(".cache")
        .join("convert")
        .join("HuggingFaceTB__SmolLM2-135M-Instruct")
        .exists());
    assert!(!models
        .join("HuggingFaceTB")
        .join("SmolLM2-135M-Instruct")
        .exists());

    // The library finds it like any downloaded model.
    let library =
        harness_core::models::ModelManager.scan_directories(std::slice::from_ref(&models));
    assert!(
        library.iter().any(|m| m.path == output),
        "{:?}",
        library.iter().map(|m| &m.path).collect::<Vec<_>>()
    );

    // Run it with the managed llama-server and ask something through the gateway.
    let mut def = ModelDefinition::new(
        "smollm2-converted",
        ModelRef::Local {
            path: output.clone(),
        },
    );
    def.load
        .insert("context_size".into(), ParamValue::Int(2048));
    // CPU: the test must not depend on how much VRAM the machine has free.
    def.load.insert("gpu_layers".into(), ParamValue::Int(0));
    let store = harness_core::domain::store::Domain::global();
    // A kept data root (HS_CONVERT_E2E_ROOT) may hold the definition of an earlier run.
    for old in store.definitions().unwrap() {
        if old.alias == "smollm2-converted" {
            store.delete_definition(&old.id).unwrap();
        }
    }
    store.save_definition(def).unwrap();

    let registry = Arc::new(ServerRegistry::default());
    let server = GatewayServer::new();
    let status = server
        .start(
            GatewayContext {
                registry: registry.clone(),
                runtime: Arc::new(harness_core::runtime::RuntimeManager::default()),
                gate: Arc::new(harness_core::operation_gate::OperationGate::default()),
                data_dir: dir.clone(),
                downloads: None,
            },
            &GatewaySettings {
                enabled: true,
                port: 19979,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let base = status.endpoint.unwrap().host_url;
    let res = reqwest::Client::new()
        .post(format!("{base}/v1/chat/completions"))
        .json(&json!({
            "model": "smollm2-converted",
            "messages": [{"role": "user", "content": "What is the capital of France? Answer in one short sentence."}],
            "max_tokens": 40,
            "temperature": 0
        }))
        .send()
        .await
        .unwrap();
    let status = res.status();
    let body: Value = res.json().await.unwrap();
    assert_eq!(status, 200, "{body}");
    let answer = body["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string();
    eprintln!("answer: {answer:?}");
    assert!(
        answer.chars().filter(|c| c.is_alphabetic()).count() > 5,
        "{answer:?}"
    );
    assert!(
        answer.to_lowercase().contains("paris"),
        "a 135M model should still know this: {answer:?}"
    );

    server.stop().await;
    registry.stop_all().await;

    // The environment stays for the next conversion and can be removed.
    let env = convert::env::status();
    assert!(env.installed && env.ready, "{env:?}");
    eprintln!("environment: {} bytes", env.size_bytes);
}

#[tokio::test]
#[ignore = "downloads the model (270 MB) and needs HS_MANAGED_RUNTIME and a built environment"]
async fn a_cancelled_conversion_cleans_up_and_a_bad_quant_is_refused() {
    let Some(runtime) = managed_runtime() else {
        return;
    };
    let _dir = data_root(&runtime);
    let models = harness_core::config::AppSettings::load().primary_models_dir();

    // Not a quantization at all.
    let err = convert::start(REPO, Some("not a quant"), false)
        .await
        .unwrap_err();
    assert!(err.starts_with("convert.invalid_quant"), "{err}");

    let job = convert::start(REPO, Some("Q8_0"), false).await.unwrap();
    // The same repo cannot be queued twice.
    let again = convert::start(REPO, Some("Q8_0"), false).await.unwrap_err();
    assert!(again.starts_with("convert.already_running"), "{again}");
    // Let it start working, then stop it.
    tokio::time::sleep(Duration::from_secs(8)).await;
    convert::cancel(job.id).unwrap();
    let job = wait_for(job.id, Duration::from_secs(120)).await;
    assert_eq!(job.status, ConvertStatus::Cancelled, "{job:?}");
    assert!(job.output.is_none() && job.error.is_none());
    assert!(!models
        .join("HuggingFaceTB")
        .join("SmolLM2-135M-Instruct-GGUF")
        .join("SmolLM2-135M-Instruct-Q8_0.gguf")
        .exists());
    // The 16-bit intermediates are gone; removing the entry deletes what was kept.
    let work = models
        .join(".cache")
        .join("convert")
        .join("HuggingFaceTB__SmolLM2-135M-Instruct");
    assert!(!work.join("out").exists());
    convert::remove(job.id).unwrap();
    assert!(!work.exists());
    assert!(convert::jobs().iter().all(|j| j.id != job.id));
}
