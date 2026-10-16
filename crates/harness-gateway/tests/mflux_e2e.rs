//! The MLX image engine (mflux) end to end, on Apple Silicon (no mocks): the managed `mflux`
//! environment is built from its hashed lock, the library finds a FLUX.2 [klein] folder in mflux's
//! format, and `/v1/images/generations` and `/v1/images/edits` go through the gateway to the
//! worker it starts.
//!
//! ```text
//! HS_MFLUX_MODEL=<a FLUX.2 [klein] folder in mflux's format, e.g.
//!   <models>/mlx-community/FLUX.2-Klein-4B-3bit> \
//!   cargo test -p harness-gateway --test mflux_e2e -- --ignored --nocapture
//! ```
//!
//! `HS_MFLUX_E2E_ROOT=<folder>` keeps the data root (and the environment) between runs; by
//! default it is a temporary folder (uv, Python and the packages: about 1.1 GB).

#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine as _;
use harness_core::domain::{ModelDefinition, ModelRef};
use harness_core::gateway::GatewaySettings;
use harness_core::servers::ServerRegistry;
use harness_gateway::{GatewayContext, GatewayServer};
use serde_json::{json, Value};

fn png_size(b: &[u8]) -> (u32, u32) {
    assert_eq!(&b[..8], b"\x89PNG\r\n\x1a\n", "not a PNG");
    (
        u32::from_be_bytes(b[16..20].try_into().unwrap()),
        u32::from_be_bytes(b[20..24].try_into().unwrap()),
    )
}

#[tokio::test]
#[ignore = "builds the mflux environment (~1.1 GB) and needs HS_MFLUX_MODEL"]
async fn an_mflux_model_draws_and_edits_through_the_gateway() {
    let Some(model) = std::env::var_os("HS_MFLUX_MODEL")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
    else {
        eprintln!("HS_MFLUX_MODEL not set, skipping");
        return;
    };
    harness_core::secrets::use_memory_store();
    let dir = match std::env::var_os("HS_MFLUX_E2E_ROOT") {
        Some(d) => {
            let d = PathBuf::from(d);
            std::fs::create_dir_all(&d).unwrap();
            d
        }
        None => tempfile::Builder::new()
            .prefix(&harness_core::paths::test_dir_prefix("mflux-e2e"))
            .tempdir()
            .unwrap()
            .keep(),
    };
    harness_core::paths::init(dir.clone());
    let mut settings = harness_core::config::AppSettings::load();
    // `<root>/<owner>/<repo>`, as a download lays it out.
    settings.models_dir = model
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_string_lossy()
        .to_string();
    settings.control_api.port = 19892;
    settings.save().unwrap();

    // The engine is missing until its environment is built (a kept root may have it already).
    if let Err(missing) = harness_core::engine::mlx::mflux_binary() {
        assert!(
            missing.contains("runtime.family_not_installed"),
            "{missing}"
        );
    }
    let t = Instant::now();
    let env = harness_core::pyenv::envs::install("mflux").await.unwrap();
    eprintln!(
        "environment in {:?}: {} bytes, {:?}",
        t.elapsed(),
        env.size_bytes,
        env.manifest.as_ref().map(|m| &m.python)
    );
    assert!(
        env.ready && env.supported && env.refusal.is_none(),
        "{env:?}"
    );
    harness_core::engine::mlx::mflux_binary().unwrap();

    // The library lists the folder as an mflux text-to-image pipeline model.
    let library =
        harness_core::models::ModelManager.scan_directories(&[settings.primary_models_dir()]);
    let found = library
        .iter()
        .find(|m| m.path == model)
        .unwrap_or_else(|| panic!("{:?}", library.iter().map(|m| &m.path).collect::<Vec<_>>()));
    eprintln!(
        "library: {} {} {} {:?}",
        found.name, found.quantization, found.size_bytes, found.task
    );
    assert_eq!(found.kind, harness_core::domain::ModelKind::Pipeline);
    assert_eq!(found.task.as_deref(), Some("text-to-image"));
    assert!(harness_core::engine::mlx::is_mflux_dir(&model));
    assert_eq!(
        harness_core::engine::mlx::family_of(&model),
        Some(harness_core::engine::MFLUX)
    );

    let store = harness_core::domain::store::Domain::global();
    for old in store.definitions().unwrap() {
        if old.alias.starts_with("mflux-e2e") {
            store.delete_definition(&old.id).unwrap();
        }
    }
    store
        .save_definition(ModelDefinition::new(
            "mflux-e2e",
            ModelRef::Local {
                path: model.clone(),
            },
        ))
        .unwrap();
    // The same folder with a 5 s time limit (`--max-seconds`).
    let mut limited = ModelDefinition::new(
        "mflux-e2e-limited",
        ModelRef::Local {
            path: model.clone(),
        },
    );
    limited.load.insert(
        "max_seconds".into(),
        harness_core::params::ParamValue::Int(5),
    );
    store.save_definition(limited).unwrap();

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
                port: 19882,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let base = status.endpoint.unwrap().host_url;
    let http = reqwest::Client::new();
    let post = |route: &str, body: Value| {
        http.post(format!("{base}/v1/images/{route}"))
            .json(&body)
            .send()
    };
    let image_of = |v: &Value| {
        base64::engine::general_purpose::STANDARD
            .decode(v["data"][0]["b64_json"].as_str().unwrap())
            .unwrap()
    };

    // Text to image.
    let t = Instant::now();
    let res = post(
        "generations",
        json!({"model": "mflux-e2e", "prompt": "a red apple on a wooden table, photo",
               "size": "256x256", "steps": 2, "seed": 7}),
    )
    .await
    .unwrap();
    let code = res.status();
    let v: Value = res.json().await.unwrap();
    eprintln!("generation in {:?} (load included): {code}", t.elapsed());
    assert_eq!(code, 200, "{v}");
    let drawn = image_of(&v);
    assert_eq!(png_size(&drawn), (256, 256));

    // An edit of that image, without a size: the image's own shape.
    let t = Instant::now();
    let res = post(
        "edits",
        json!({"model": "mflux-e2e", "prompt": "make the apple green",
               "image": base64::engine::general_purpose::STANDARD.encode(&drawn),
               "steps": 2, "seed": 7}),
    )
    .await
    .unwrap();
    let code = res.status();
    let v: Value = res.json().await.unwrap();
    eprintln!("edit in {:?}: {code}", t.elapsed());
    assert_eq!(code, 200, "{v}");
    assert_eq!(png_size(&image_of(&v)), (256, 256));

    // Out of range or not for this model: 400 before any work.
    for (body, field) in [
        (json!({"prompt": "x", "steps": 51}), "steps"),
        (json!({"prompt": "x", "n": 5}), "n"),
        (json!({"prompt": "x", "size": "1000x1000"}), "size"),
        (
            json!({"prompt": "x", "negative_prompt": "y"}),
            "negative_prompt",
        ),
        (json!({"prompt": "x", "cfg_scale": 3.5}), "cfg_scale"),
        (json!({"prompt": "x", "sampler": "euler"}), "sampler"),
    ] {
        let mut body = body;
        body["model"] = json!("mflux-e2e");
        let res = post("generations", body.clone()).await.unwrap();
        let code = res.status();
        let v: Value = res.json().await.unwrap();
        assert_eq!(code, 400, "{body}: {v}");
        assert_eq!(v["error"]["param"], field, "{v}");
    }
    // Larger than this Mac can hold (an 8 GB M1 has room for about 512x512 with a 3-bit 4B):
    // the worker names the largest that fits.
    if harness_core::hardware::detect_hardware().total_ram_bytes <= 8 << 30 {
        let res = post(
            "generations",
            json!({"model": "mflux-e2e", "prompt": "x", "size": "2048x2048"}),
        )
        .await
        .unwrap();
        let code = res.status();
        let v: Value = res.json().await.unwrap();
        eprintln!("2048x2048: {code} {v}");
        assert_eq!(code, 400, "{v}");
        assert!(
            v["error"]["message"]
                .as_str()
                .unwrap()
                .contains("the largest that fits"),
            "{v}"
        );
    }

    // A client that goes away stops its image: the next request is not kept waiting for it.
    let slow = http
        .post(format!("{base}/v1/images/generations"))
        .timeout(Duration::from_secs(8))
        .json(
            &json!({"model": "mflux-e2e", "prompt": "a city at night", "size": "512x512",
                      "steps": 40}),
        )
        .send()
        .await;
    assert!(
        slow.is_err(),
        "the long request ended before the client left"
    );
    let t = Instant::now();
    let res = post(
        "generations",
        json!({"model": "mflux-e2e", "prompt": "a blue cup", "size": "256x256", "steps": 1}),
    )
    .await
    .unwrap();
    let after = t.elapsed();
    eprintln!("next request {:?} after the client left", after);
    assert_eq!(res.status(), 200);
    // 40 steps at 512x512 take several minutes on an M1; a stopped run frees the worker within
    // a step or two.
    assert!(after < Duration::from_secs(90), "{after:?}");

    // The worker's own time limit: a 504 between steps.
    let t = Instant::now();
    let res = post(
        "generations",
        json!({"model": "mflux-e2e-limited", "prompt": "a forest", "size": "512x512",
               "steps": 40}),
    )
    .await
    .unwrap();
    let code = res.status();
    let v: Value = res.json().await.unwrap();
    eprintln!("time limit: {code} after {:?}: {v}", t.elapsed());
    assert_eq!(code, 504, "{v}");

    server.stop().await;
    registry.stop_all().await;
}
