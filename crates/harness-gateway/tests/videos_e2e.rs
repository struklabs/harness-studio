//! Video generation through the gateway with a real sd-server and a real Wan model (no mocks):
//!
//! ```text
//! HS_SD_SERVER=<sd-server> HS_VIDEO_LIBRARY=<folder in the download layout with a Wan2.1 T2V
//! 1.3B file, wan_2.1_vae and umt5-xxl> cargo test -p harness-gateway --test videos_e2e -- --ignored --nocapture
//! ```
//!
//! The library folder is scanned like the app's, so the Wan bundle makes the definition (VAE and
//! text encoder found in the layout). Temporary data root (never the user's), in-memory
//! secrets. Small and short (416x240, 9 frames, 10 steps): about 9 minutes on an Apple M1.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use harness_core::gateway::GatewaySettings;
use harness_core::servers::ServerRegistry;
use harness_gateway::{GatewayContext, GatewayServer};
use serde_json::{json, Value};

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|p| p.exists())
}

#[tokio::test]
#[ignore]
async fn e2e_video_job() {
    let (Some(server), Some(library)) = (env_path("HS_SD_SERVER"), env_path("HS_VIDEO_LIBRARY"))
    else {
        eprintln!("HS_SD_SERVER / HS_VIDEO_LIBRARY not set, skipping");
        return;
    };
    harness_core::secrets::use_memory_store();
    let dir = tempfile::Builder::new()
        .prefix(&harness_core::paths::test_dir_prefix("gw-video"))
        .tempdir()
        .unwrap()
        .keep();
    harness_core::paths::init(dir.clone());
    let mut s = harness_core::config::AppSettings::load();
    s.models_dir = library.to_string_lossy().to_string();
    s.server.port = 18860;
    s.control_api.port = 18869;
    s.save().unwrap();
    harness_core::runtimes::RuntimeRegistry::global()
        .add_local(&harness_core::runtimes::AddLocal {
            path: server.parent().unwrap().to_path_buf(),
            name: "sd e2e".into(),
            id: None,
            copy: false,
            family: "stable-diffusion.cpp".into(),
        })
        .unwrap();

    // The library scan and the Wan bundle make the definition.
    harness_core::models::ModelManager.scan_models();
    let made = harness_core::image_bundles::sync_library_definitions().unwrap();
    eprintln!("definitions: {made:?}");
    let defs = harness_core::domain::store::Domain::global()
        .definitions()
        .unwrap();
    let def = defs
        .iter()
        .find(|d| {
            d.base
                .local_path()
                .is_some_and(|p| p.to_string_lossy().to_lowercase().contains("wan"))
        })
        .expect("a Wan definition")
        .clone();
    eprintln!("{}: {:?}", def.alias, def.load);
    for k in ["vae", "t5xxl"] {
        assert!(def.load.contains_key(k), "the bundle gave {k}");
    }

    let registry = Arc::new(ServerRegistry::default());
    let ctx = GatewayContext {
        registry: registry.clone(),
        runtime: Arc::new(harness_core::runtime::RuntimeManager::default()),
        gate: Arc::new(harness_core::operation_gate::OperationGate::default()),
        data_dir: dir.clone(),
        downloads: None,
    };
    let gw = GatewayServer::new();
    let status = gw
        .start(
            ctx,
            &GatewaySettings {
                enabled: true,
                port: 18860,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let base = status.endpoint.unwrap().host_url;
    let http = reqwest::Client::new();

    // The job answers at once.
    let t = Instant::now();
    let r = http
        .post(format!("{base}/v1/videos"))
        .json(
            &json!({"model": def.alias, "prompt": "a red fox running through snow, cinematic",
                      "size": "416x240", "frames": 9, "steps": 10, "seed": 7}),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 200);
    let job: Value = r.json().await.unwrap();
    eprintln!("created in {:?}: {job}", t.elapsed());
    assert!(t.elapsed() < Duration::from_secs(5));
    assert_eq!(job["object"], "video");
    assert_eq!(job["status"], "queued");
    let id = job["id"].as_str().unwrap().to_string();

    // Not ready yet.
    let r = http
        .get(format!("{base}/v1/videos/{id}/content"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 409);

    // A second job waits in the queue and is deleted there.
    let r = http
        .post(format!("{base}/v1/videos"))
        .json(&json!({"model": def.alias, "prompt": "a cat", "frames": 5, "steps": 2}))
        .send()
        .await
        .unwrap();
    let second: Value = r.json().await.unwrap();
    let second_id = second["id"].as_str().unwrap();
    tokio::time::sleep(Duration::from_secs(2)).await;
    let v: Value = http
        .get(format!("{base}/v1/videos/{second_id}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(v["status"], "queued", "one video at a time");
    let r = http
        .delete(format!("{base}/v1/videos/{second_id}"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 200);
    let r = http
        .get(format!("{base}/v1/videos/{second_id}"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 404);

    // Poll the first one to its end.
    let deadline = Instant::now() + Duration::from_secs(1800);
    let mut last = 0;
    let done = loop {
        tokio::time::sleep(Duration::from_secs(5)).await;
        let v: Value = http
            .get(format!("{base}/v1/videos/{id}"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let p = v["progress"].as_u64().unwrap_or(0);
        if p != last {
            eprintln!("{:?} {} {p}%", t.elapsed(), v["status"]);
            last = p;
        }
        match v["status"].as_str().unwrap() {
            "completed" | "failed" => break v,
            _ => assert!(Instant::now() < deadline, "the video took too long"),
        }
    };
    eprintln!("finished in {:?}: {done}", t.elapsed());
    assert_eq!(done["status"], "completed", "{done}");
    assert_eq!(done["progress"], 100);
    let r = http
        .get(format!("{base}/v1/videos/{id}/content"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 200);
    assert_eq!(r.headers()["content-type"], "video/webm");
    let bytes = r.bytes().await.unwrap();
    // A WebM (EBML) file.
    assert_eq!(&bytes[..4], b"\x1a\x45\xdf\xa3");
    if let Ok(keep) = std::env::var("HS_VIDEO_OUT") {
        std::fs::write(keep, &bytes).unwrap();
    }
    let list: Value = http
        .get(format!("{base}/v1/videos"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(list["data"].as_array().unwrap().len(), 1);

    // The file is in the data folder; DELETE removes it.
    let file = dir.join("videos").join(format!("{id}.webm"));
    assert!(file.is_file());
    let r = http
        .delete(format!("{base}/v1/videos/{id}"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 200);
    assert!(!file.exists());

    gw.stop().await;
    registry.stop_all().await;
}
