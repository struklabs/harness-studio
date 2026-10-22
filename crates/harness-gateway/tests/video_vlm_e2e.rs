//! Real llama.cpp video questions. Download Qwen2.5-VL-3B GGUF + matching projector
//! into an isolated temp folder; set HS_VIDEO_VLM_MODEL, HS_VIDEO_VLM_PROJECTOR and
//! HS_VIDEO_VLM_SERVER, then run this test with --ignored --nocapture. ffmpeg required.
use std::{path::PathBuf, process::Stdio, sync::Arc, time::Duration};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use futures_util::FutureExt;
use harness_core::{
    domain::{ModelDefinition, ModelRef},
    gateway::GatewaySettings,
    params::ParamValue,
    servers::ServerRegistry,
};
use harness_gateway::{GatewayContext, GatewayServer};
use serde_json::{json, Value};

fn required(name: &str) -> PathBuf {
    let path = PathBuf::from(std::env::var_os(name).expect(name));
    assert!(path.is_file(), "{name}: {}", path.display());
    assert!(
        path.starts_with(std::env::temp_dir()),
        "downloads must be isolated in temp"
    );
    path
}

#[tokio::test]
#[ignore = "requires a real Qwen2.5-VL GGUF, projector, llama-server and ffmpeg"]
async fn qwen_understands_video_through_gateway() {
    let model = required("HS_VIDEO_VLM_MODEL");
    let projector = required("HS_VIDEO_VLM_PROJECTOR");
    let binary = required("HS_VIDEO_VLM_SERVER");
    let data = tempfile::tempdir_in(model.parent().unwrap()).unwrap();
    harness_core::paths::init(data.path().to_path_buf());
    harness_core::secrets::use_memory_store();
    let mut settings = harness_core::config::AppSettings::load();
    settings.server.port = 12481;
    settings.control_api.port = 12482;
    settings.server.load_timeout_secs = 600;
    settings.save().unwrap();
    let runtime = harness_core::runtimes::RuntimeRegistry::global()
        .add_local(&harness_core::runtimes::AddLocal {
            path: binary.parent().unwrap().to_path_buf(),
            name: "Video VLM test".into(),
            id: None,
            copy: false,
            family: "llama.cpp".into(),
        })
        .unwrap();
    let mut def = ModelDefinition::new("video-qwen", ModelRef::Local { path: model });
    def.mmproj = Some(projector);
    def.load
        .insert("runtime".into(), ParamValue::Text(runtime.id));
    def.load
        .insert("context_size".into(), ParamValue::Int(8192));
    def.load.insert("gpu_layers".into(), ParamValue::Int(99));
    def.load.insert("parallel_slots".into(), ParamValue::Int(1));
    def.load.insert("threads".into(), ParamValue::Int(2));
    harness_core::domain::store::Domain::global()
        .save_definition(def)
        .unwrap();
    let registry = Arc::new(ServerRegistry::default());
    let gateway = GatewayServer::new();
    let status = gateway
        .start(
            GatewayContext {
                registry: registry.clone(),
                runtime: Arc::new(harness_core::runtime::RuntimeManager::default()),
                gate: Arc::new(harness_core::operation_gate::OperationGate::default()),
                data_dir: data.path().into(),
                downloads: None,
            },
            &GatewaySettings {
                enabled: true,
                port: 12480,
                request_timeout_secs: 600,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let outcome = std::panic::AssertUnwindSafe(async {
        let ffmpeg = harness_core::audio::ffmpeg_path().expect("ffmpeg");
        let clip_path = data.path().join("colors.webm");
        let mut cmd = tokio::process::Command::new(ffmpeg);
        cmd.args(["-v", "error", "-f", "lavfi", "-i", "color=red:size=64x64:rate=8:duration=2", "-threads", "1", "-c:v", "libvpx-vp9", clip_path.to_str().unwrap()])
            .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000);
        let clip = cmd.output().await.unwrap();
        assert!(clip.status.success(), "{}", String::from_utf8_lossy(&clip.stderr));
        let base = status.endpoint.unwrap().host_url;
        let http = reqwest::Client::builder().timeout(Duration::from_secs(600)).build().unwrap();
        let response = http.post(format!("{base}/v1/chat/completions")).json(&json!({
            "model":"video-qwen", "max_tokens":64, "temperature":0, "stream":false,
            "messages":[{"role":"user", "content":[{"type":"text", "text":"What color fills this video? Answer with one word."}, {"type":"video_url", "video_url":{"url":format!("data:video/webm;base64,{}", STANDARD.encode(std::fs::read(&clip_path).unwrap()))}}]}]
        })).send().await.unwrap();
        let code = response.status();
        let body: Value = response.json().await.unwrap();
        eprintln!("Video response: {code} {body}");
        assert_eq!(code, 200, "{body}");
        let answer = body["choices"][0]["message"]["content"].as_str().unwrap().to_lowercase();
        assert!(answer.contains("red"), "{answer}");
        // Eight 512px Qwen frames consume over 2,000 prompt tokens. A silently dropped
        // video or a single-image request must not pass the transport smoke test.
        assert!(body["usage"]["prompt_tokens"].as_u64().unwrap() > 2000, "{body}");
        let response = http.post(format!("{base}/v1/chat/completions")).json(&json!({"model":"video-qwen", "messages":[{"role":"user", "content":[{"type":"video_url", "video_url":{"url":"http://127.0.0.1:9/secret"}}]}]})).send().await.unwrap();
        assert_eq!(response.status(), 400);
    }).catch_unwind().await;
    gateway.stop().await;
    registry.stop_all().await;
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}
