//! Opt-in CPU integration: HS_VIDEO_E2E_ROOT contains ltx/ and smol/ downloads and
//! data/bin/pyenv/envs/hf-cpu/venv installed from the current lock. All state stays there.
//! Run with --ignored --nocapture. Never points at an existing user data directory.
use base64::Engine as _;
use futures_util::FutureExt;
use harness_core::{
    domain::{ModelDefinition, ModelRef},
    gateway::GatewaySettings,
    params::ParamValue,
    servers::ServerRegistry,
};
use harness_gateway::{GatewayContext, GatewayServer};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

#[tokio::test]
#[ignore = "downloads and a dedicated CPU Python environment required"]
async fn python_video_paths() {
    let root = PathBuf::from(std::env::var_os("HS_VIDEO_E2E_ROOT").expect("HS_VIDEO_E2E_ROOT"));
    assert!(
        root.starts_with(std::env::temp_dir()),
        "isolated temp directory required"
    );
    let data = root.join("data");
    harness_core::paths::init(data.clone());
    harness_core::secrets::use_memory_store();
    let spec = &harness_core::pyenv::envs::HF_CPU;
    let env = harness_core::pyenv::envs::env_dir_in(&harness_core::pyenv::envs::root(), spec.name);
    assert!(
        harness_core::pyenv::envs::python_in(&harness_core::pyenv::envs::root(), spec.name)
            .is_file()
    );
    std::fs::write(
        env.join("manifest.json"),
        serde_json::to_vec(&harness_core::pyenv::envs::Manifest {
            name: spec.name.into(),
            uv_version: harness_core::pyenv::UV_VERSION.into(),
            python: "Python 3.12.13".into(),
            lock_sha256: harness_core::pyenv::envs::lock_digest(spec),
            installed_at: 0,
        })
        .unwrap(),
    )
    .unwrap();
    let mut settings = harness_core::config::AppSettings::load();
    settings.server.load_timeout_secs = 600;
    settings.server.port = 11883;
    settings.control_api.port = 11884;
    settings.save().unwrap();
    for (alias, folder) in [("video", "ltx"), ("questions", "smol")] {
        let mut def = ModelDefinition::new(
            alias,
            ModelRef::Local {
                path: root.join(folder),
            },
        );
        def.load
            .insert("device".into(), ParamValue::Text("cpu".into()));
        def.load.insert("threads".into(), ParamValue::Int(6));
        harness_core::domain::store::Domain::global()
            .save_definition(def)
            .unwrap();
    }
    let registry = Arc::new(ServerRegistry::default());
    let gw = GatewayServer::new();
    let status = gw
        .start(
            GatewayContext {
                registry: registry.clone(),
                runtime: Arc::new(harness_core::runtime::RuntimeManager::default()),
                gate: Arc::new(harness_core::operation_gate::OperationGate::default()),
                data_dir: data,
                downloads: None,
            },
            &GatewaySettings {
                enabled: true,
                port: 11880,
                request_timeout_secs: 1200,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let outcome = std::panic::AssertUnwindSafe(async {
    let base = status.endpoint.unwrap().host_url;
    let http = reqwest::Client::builder().timeout(Duration::from_secs(1200)).build().unwrap();
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(64, 64).write_to(&mut png, image::ImageFormat::Png).unwrap();
    let image = format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png.into_inner()));
    let db = harness_core::state::StateDb::open_default().unwrap();
    let project_dir = root.join("project");
    std::fs::create_dir_all(&project_dir).unwrap();
    let project = db.create_project(&harness_core::state::ProjectInput {
        name: "Video integration".into(), folder: Some(project_dir.to_string_lossy().into()), ..Default::default()
    }).unwrap();
    let mut video = Vec::new();
    for (format, start_image) in [("mp4", None), ("webm", Some(image))] {
        let started = Instant::now();
        let mut body = json!({"model":"video","prompt":"A red ball moves slowly", "size":"64x64","frames":9,"steps":2,"cfg_scale":1.0,"seed":7,"output_format":format});
        if let Some(image) = start_image { body["input_reference"] = json!(image); }
        let response = http.post(format!("{base}/v1/videos")).json(&body).send().await.unwrap();
        let status = response.status(); let job: Value = response.json().await.unwrap();
        assert_eq!(status.as_u16(), 200, "{job}");
        let id = job["id"].as_str().unwrap();
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let state: Value = http.get(format!("{base}/v1/videos/{id}")).send().await.unwrap().json().await.unwrap();
            if state["status"] == "completed" { break; }
            assert_ne!(state["status"], "failed", "{state}");
            assert!(started.elapsed() < Duration::from_secs(1200));
        }
        let bytes = http.get(format!("{base}/v1/videos/{id}/content")).send().await.unwrap().bytes().await.unwrap();
        if format == "mp4" { assert_eq!(&bytes[4..8], b"ftyp"); video = bytes.to_vec(); }
        else { assert_eq!(&bytes[..4], b"\x1a\x45\xdf\xa3"); }
        let saved = harness_core::projects::save_output(&db, &project.id, &format!("clip.{format}"), &bytes,
            &harness_core::projects::OutputSource { source: "manual".into(), model: Some("video".into()), ..Default::default() }).unwrap();
        assert_eq!(std::fs::read(saved.path).unwrap(), bytes.as_ref());
        eprintln!("{format} CPU generation: {:?}, {} bytes", started.elapsed(), bytes.len());
    }
    let started = Instant::now();
    let video = format!("data:video/mp4;base64,{}", base64::engine::general_purpose::STANDARD.encode(video));
    let response = http.post(format!("{base}/v1/chat/completions")).json(&json!({"model":"questions","stream":false,"max_tokens":24,"messages":[{"role":"user","content":[{"type":"text","text":"Describe this video."},{"type":"video_url","video_url":{"url":video}}]}]})).send().await.unwrap();
    let status = response.status(); let body: Value = response.json().await.unwrap();

    assert_eq!(status.as_u16(), 200, "{body}");
    assert!(!body["choices"][0]["message"]["content"].as_str().unwrap().is_empty());
    eprintln!("CPU video question: {:?}: {body}", started.elapsed());
    }).catch_unwind().await;
    gw.stop().await;
    registry.stop_all().await;
    if let Err(error) = outcome {
        std::panic::resume_unwind(error);
    }
}
