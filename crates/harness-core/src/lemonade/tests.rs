use super::*;
use client::{kind_of, ModelKind};

fn model(id: &str, labels: &[&str], downloaded: bool) -> LemonadeModel {
    let labels: Vec<String> = labels.iter().map(|s| s.to_string()).collect();
    LemonadeModel {
        id: id.into(),
        recipe: "llamacpp".into(),
        checkpoint: format!("org/{id}"),
        kind: kind_of("llamacpp", &labels),
        labels,
        size_gb: 0.4,
        downloaded,
        suggested: false,
        update_available: false,
    }
}

#[test]
fn config_json_keeps_lemonades_keys_and_sets_ours() {
    // What lemond 2026.41.1 wrote after `lemonade config set llamacpp.backend=vulkan`.
    let existing = json!({
        "config_version": 2,
        "llamacpp": { "backend": "vulkan", "args": "--no-mmap" },
        "ctx_size": 8192
    });
    let settings = LemonadeSettings {
        backend: "rocm".into(),
        runtime: None,
    };
    let cfg = merged_config(
        existing,
        &settings,
        Path::new("C:\\data\\lemonade\\models"),
        11451,
    );
    assert_eq!(cfg["models_dir"], "C:\\data\\lemonade\\models");
    assert_eq!(cfg["websocket_port"], 11451);
    assert_eq!(cfg["llamacpp"]["backend"], "rocm");
    // Lemonade's own keys stay.
    assert_eq!(cfg["llamacpp"]["args"], "--no-mmap");
    assert_eq!(cfg["ctx_size"], 8192);
    assert_eq!(cfg["config_version"], 2);
    // A missing or broken file becomes a fresh object.
    let fresh = merged_config(
        Value::Null,
        &LemonadeSettings::default(),
        Path::new("/m"),
        1,
    );
    assert_eq!(fresh["llamacpp"]["backend"], "auto");
    let broken = merged_config(
        json!({"llamacpp": 3}),
        &LemonadeSettings::default(),
        Path::new("/m"),
        1,
    );
    assert_eq!(broken["llamacpp"]["backend"], "auto");
}

#[test]
fn the_command_line_is_loopback_with_the_dirs_last() {
    let args = launch_args(
        "127.0.0.1",
        11452,
        Path::new("/d/cache"),
        Path::new("/d/config"),
    );
    assert_eq!(
        args,
        vec![
            "--host",
            "127.0.0.1",
            "--port",
            "11452",
            "--no-broadcast",
            "--log-file",
            "disabled",
            "/d/cache",
            "/d/config"
        ]
    );
}

#[test]
fn settings_accept_only_known_backends() {
    assert!(LemonadeSettings::default().validate().is_ok());
    for b in ["auto", "vulkan", "rocm", "cpu"] {
        let s = LemonadeSettings {
            backend: b.into(),
            runtime: None,
        };
        assert!(s.validate().is_ok(), "{b}");
    }
    let bad = LemonadeSettings {
        backend: "opencl; calc".into(),
        runtime: None,
    };
    assert!(bad.validate().unwrap_err().contains("lemonade.bad_backend"));
    // An old file without a field reads with defaults.
    let s: LemonadeSettings = serde_json::from_str("{}").unwrap();
    assert_eq!(s, LemonadeSettings::default());
}

#[test]
fn model_references_are_registry_names_or_checkpoints() {
    for ok in [
        "Qwen3-0.6B-GGUF",
        "user.my-model",
        "unsloth/Qwen3-0.6B-GGUF:Q4_0",
        "Gemma-3-4b-it-GGUF",
    ] {
        assert!(validate_model_ref(ok).is_ok(), "{ok}");
    }
    for bad in ["", "  ", "a b", "../x", "a;b", "x\ny", "$(calc)"] {
        assert!(validate_model_ref(bad).is_err(), "{bad:?}");
    }
    assert!(validate_model_ref(&"a".repeat(300)).is_err());
}

#[test]
fn only_downloaded_routable_models_reach_the_gateway() {
    let models = vec![
        model("Qwen3-0.6B-GGUF", &["chat"], true),
        model("Qwen3-4B-GGUF", &["chat"], false),
        model("nomic-embed-text-v1-GGUF", &["embeddings"], true),
        model("kokoro-v1", &["tts"], true),
        model("has space", &["chat"], true),
        model("Whisper-Tiny", &["transcription"], true),
    ];
    let ids: Vec<String> = servable(models).into_iter().map(|m| m.id).collect();
    assert_eq!(
        ids,
        vec![
            "Qwen3-0.6B-GGUF",
            "nomic-embed-text-v1-GGUF",
            "Whisper-Tiny"
        ]
    );
}

#[test]
fn lemonade_models_become_definitions_of_the_lemonade_runtime() {
    let models = servable(vec![
        model("Qwen3-0.6B-GGUF", &["chat"], true),
        model("Taken", &["chat"], true),
    ]);
    // A stored definition of the same alias wins.
    let stored = ModelDefinition::new("taken", ModelRef::parse("C:/models/x.gguf"));
    let defs = definitions_of(models, &[stored]);
    assert_eq!(defs.len(), 1);
    let d = &defs[0];
    assert_eq!(d.alias, "Qwen3-0.6B-GGUF");
    assert_eq!(d.id, "lemonade:Qwen3-0.6B-GGUF");
    assert_eq!(model_of_def(&d.id), Some("Qwen3-0.6B-GGUF"));
    assert_eq!(model_of_def("srv-gw-1/x"), None);
    assert_eq!(model_of_def("lemonade:"), None);
    assert_eq!(
        d.load.get("runtime"),
        Some(&crate::params::ParamValue::Text("lemonade".into()))
    );
    // Not a file: no code path mistakes it for a servable GGUF.
    assert!(d.base.local_path().is_some_and(|p| !p.is_file()));
}

#[test]
fn kinds_route_to_the_gateway_endpoints() {
    assert!(ModelKind::Chat.routable() && ModelKind::Embedding.routable());
    assert!(ModelKind::Rerank.routable() && ModelKind::Transcription.routable());
    assert!(!ModelKind::Speech.routable() && !ModelKind::Image.routable());
    assert!(!ModelKind::Other.routable());
}

#[test]
fn status_on_a_machine_without_lemonade_or_an_npu() {
    // The test data root is empty: nothing installed, nothing running.
    let s = status();
    assert!(s.installs.is_empty());
    assert!(!s.running && s.port.is_none() && s.base_url.is_none());
    assert_eq!(s.settings, LemonadeSettings::default());
    assert!(s.models_dir.ends_with("models"));
    assert_eq!(s.supported, platform_supported());
    assert!(gateway_models().is_empty());
    assert!(endpoint().is_none());
}

#[test]
fn keys_are_random_and_long() {
    let (a, b) = (new_key(), new_key());
    assert_eq!(a.len(), 64);
    assert_ne!(a, b);
    assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
}

#[tokio::test]
async fn starting_without_an_installed_build_says_so() {
    let e = ensure_running().await.unwrap_err();
    assert!(e.contains("lemonade.not_installed"), "{e}");
}
