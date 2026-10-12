//! Router mode against the real managed llama-server and two small GGUF files.
//!
//! `cargo test -p harness-core --test router_e2e -- --ignored --nocapture`
//!
//! Environment (defaults are this project's Windows layout):
//! * `HARNESS_E2E_LLAMA_SERVER`: the llama-server binary
//! * `HARNESS_E2E_GEMMA`: `gemma-3-1b-it-q4_0.gguf` (the QAT build whose metadata needs the
//!   `sliding_window` override)
//! * `HARNESS_E2E_QWEN`: any small chat GGUF
//! * `HARNESS_E2E_PORT`: a free port (default 11490)

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use harness_core::domain::launch::LaunchDefaults;
use harness_core::domain::{ModelDefinition, ModelRef};
use harness_core::params::ParamValue;
use harness_core::router::{self, RouterEndpoint, RouterModelState};

fn env_path(name: &str, default: PathBuf) -> PathBuf {
    std::env::var_os(name).map(PathBuf::from).unwrap_or(default)
}

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn link(from: &PathBuf, dir: &std::path::Path, name: &str) -> PathBuf {
    let sub = dir.join(name);
    std::fs::create_dir_all(&sub).unwrap();
    let to = sub.join(from.file_name().unwrap());
    if std::fs::hard_link(from, &to).is_err() {
        std::fs::copy(from, &to).unwrap();
    }
    to
}

#[tokio::test]
#[ignore = "needs the managed llama-server and two small GGUF files"]
async fn router_serves_loads_and_unloads_models_with_their_presets() {
    let appdata = PathBuf::from(std::env::var("APPDATA").unwrap_or_default());
    let binary = env_path(
        "HARNESS_E2E_LLAMA_SERVER",
        appdata.join("com.harnessstudio.app/bin/managed/llama-server.exe"),
    );
    let gemma = env_path(
        "HARNESS_E2E_GEMMA",
        PathBuf::from(r"D:\Models\google\gemma-3-1b-it-qat-q4_0-gguf\gemma-3-1b-it-q4_0.gguf"),
    );
    let qwen = env_path(
        "HARNESS_E2E_QWEN",
        PathBuf::from(
            r"D:\Models\Qwen\Qwen2.5-0.5B-Instruct-GGUF\qwen2.5-0.5b-instruct-q4_k_m.gguf",
        ),
    );
    let port: u16 = std::env::var("HARNESS_E2E_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(11490);
    for f in [&binary, &gemma, &qwen] {
        assert!(f.is_file(), "missing {}", f.display());
    }

    let dir = tempfile::tempdir().unwrap();
    let models = dir.path().join("models");
    let gemma_link = link(&gemma, &models, "gemma-dir");
    let qwen_link = link(&qwen, &models, "qwen-dir");

    // Harness definitions of both files: the preset carries their settings.
    let runtime = harness_core::engine::llama_cpp().detect_capabilities(&binary);
    assert!(
        runtime.has_flag("--models-preset"),
        "runtime has no router presets"
    );
    let mut gemma_def = ModelDefinition::new("gemma-1b", ModelRef::Local { path: gemma_link });
    gemma_def.id = "md-gemma".into();
    gemma_def
        .load
        .insert("context_size".into(), ParamValue::Int(2048));
    gemma_def
        .load
        .insert("gpu_layers".into(), ParamValue::Int(0));
    let mut qwen_def = ModelDefinition::new("qwen-half", ModelRef::Local { path: qwen_link });
    qwen_def.id = "md-qwen".into();
    qwen_def
        .load
        .insert("context_size".into(), ParamValue::Int(1024));
    qwen_def
        .load
        .insert("gpu_layers".into(), ParamValue::Int(0));
    let defaults = LaunchDefaults {
        gpu_layers: 0,
        context_size: 0,
        host: "127.0.0.1".into(),
        port,
        models_dir: models.clone(),
    };
    let sections = router::presets_for_dir(
        &[gemma_def, qwen_def],
        &models,
        &defaults,
        Some(runtime.as_ref()),
    );
    assert_eq!(sections.len(), 2);
    let text = router::render_preset(&sections);
    println!("{text}");
    // The Gemma 3 QAT metadata fix reaches the router, which single-model launches had alone.
    assert!(
        text.contains("override-kv = gemma3.attention.sliding_window=int:512"),
        "no sliding_window fix in:\n{text}"
    );
    assert!(text.contains("[gemma-1b]") && text.contains("[qwen-half]"));
    let preset = dir.path().join("preset.ini");
    std::fs::write(&preset, &text).unwrap();

    let child = Command::new(&binary)
        .current_dir(binary.parent().unwrap())
        .args(["--models-dir"])
        .arg(&models)
        .arg("--models-preset")
        .arg(&preset)
        .args(["--models-max", "1", "--host", "127.0.0.1", "--port"])
        .arg(port.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start llama-server");
    let _guard = Kill(child);
    let ep = RouterEndpoint {
        base: format!("http://127.0.0.1:{port}"),
        api_key: None,
        models_dir: None,
    };
    let mut listed = Vec::new();
    for _ in 0..60 {
        if let Ok(models) = router::list(&ep).await {
            listed = models;
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let state_of = |models: &[router::RouterModel], id: &str| {
        models
            .iter()
            .find(|m| m.id == id)
            .unwrap_or_else(|| panic!("{id} not listed"))
            .state
    };
    assert_eq!(state_of(&listed, "gemma-1b"), RouterModelState::Unloaded);
    assert_eq!(state_of(&listed, "qwen-half"), RouterModelState::Unloaded);

    // Load Gemma with its preset: the context comes from the definition.
    let gemma_loaded = router::load(&ep, "gemma-1b", Some(Duration::from_secs(180)))
        .await
        .unwrap();
    assert_eq!(gemma_loaded.state, RouterModelState::Loaded);
    assert!(
        gemma_loaded
            .path
            .as_deref()
            .is_some_and(|p| p.ends_with("gemma-3-1b-it-q4_0.gguf")),
        "path: {:?}",
        gemma_loaded.path
    );

    // A request to the router answers from the loaded model.
    let answer: serde_json::Value = reqwest::Client::new()
        .post(format!("{}/v1/chat/completions", ep.base))
        .json(&serde_json::json!({
            "model": "gemma-1b",
            "messages": [{"role": "user", "content": "Say hi."}],
            "max_tokens": 4
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(answer["choices"][0]["message"].is_object(), "{answer}");

    // `--models-max 1`: loading the second model evicts the first.
    let qwen_loaded = router::load(&ep, "qwen-half", Some(Duration::from_secs(180)))
        .await
        .unwrap();
    assert_eq!(qwen_loaded.state, RouterModelState::Loaded);
    let after = router::list(&ep).await.unwrap();
    assert_eq!(state_of(&after, "qwen-half"), RouterModelState::Loaded);
    assert_eq!(state_of(&after, "gemma-1b"), RouterModelState::Unloaded);

    let unloaded = router::unload(&ep, "qwen-half").await.unwrap();
    assert_eq!(unloaded.state, RouterModelState::Unloaded);

    // An unknown model is an error, not a hang.
    assert!(router::load(&ep, "no-such-model", None).await.is_err());
}
