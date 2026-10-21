//! The mflux engine beyond FLUX.2 [klein]'s text to image, on Apple Silicon (no mocks): a LoRA
//! adapter on a klein folder, and another family (Z-Image) drawing from text and from an image,
//! through the gateway's `/v1/images/generations` and `/v1/images/edits`.
//!
//! ```text
//! HS_MFLUX_MODEL=<a FLUX.2 [klein] 4B folder in mflux's format> \
//! HS_MFLUX_LORA=<a FLUX.2 [klein] 4B LoRA .safetensors> \
//! HS_MFLUX_ZIMAGE=<a Z-Image-Turbo folder in mflux's format> \
//!   cargo test -p harness-gateway --test mflux_breadth_e2e -- --ignored --nocapture --test-threads=1
//! ```
//!
//! All model folders must be in the same models folder (`<root>/<owner>/<repo>`). Each test
//! skips when its variables are not set. `HS_MFLUX_E2E_ROOT=<folder>` keeps the data root (and
//! the environment) between runs.

#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use base64::Engine as _;
use harness_core::domain::{AdapterRef, ModelDefinition, ModelRef};
use harness_core::gateway::GatewaySettings;
use harness_core::servers::ServerRegistry;
use harness_gateway::{GatewayContext, GatewayServer};
use serde_json::{json, Value};

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

fn png_size(b: &[u8]) -> (u32, u32) {
    assert_eq!(&b[..8], b"\x89PNG\r\n\x1a\n", "not a PNG");
    (
        u32::from_be_bytes(b[16..20].try_into().unwrap()),
        u32::from_be_bytes(b[20..24].try_into().unwrap()),
    )
}

fn folder(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.exists())
}

/// The data root, the mflux environment and the models folder of `model`; the definitions
/// given replace earlier ones of the same alias.
async fn setup(model: &Path, defs: Vec<ModelDefinition>) -> PathBuf {
    harness_core::secrets::use_memory_store();
    let dir = match std::env::var_os("HS_MFLUX_E2E_ROOT") {
        Some(d) => {
            let d = PathBuf::from(d);
            std::fs::create_dir_all(&d).unwrap();
            d
        }
        None => tempfile::Builder::new()
            .prefix(&harness_core::paths::test_dir_prefix("mflux-breadth-e2e"))
            .tempdir()
            .unwrap()
            .keep(),
    };
    harness_core::paths::init(dir.clone());
    let mut settings = harness_core::config::AppSettings::load();
    settings.models_dir = model
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_string_lossy()
        .to_string();
    settings.control_api.port = 19893;
    settings.save().unwrap();
    let t = Instant::now();
    let env = harness_core::pyenv::envs::install("mflux").await.unwrap();
    eprintln!("environment in {:?}: {} bytes", t.elapsed(), env.size_bytes);
    assert!(env.ready, "{env:?}");
    let store = harness_core::domain::store::Domain::global();
    for def in defs {
        for old in store.definitions().unwrap() {
            if old.alias == def.alias {
                store.delete_definition(&old.id).unwrap();
            }
        }
        store.save_definition(def).unwrap();
    }
    dir
}

async fn gateway(dir: PathBuf) -> (GatewayServer, Arc<ServerRegistry>, String) {
    let registry = Arc::new(ServerRegistry::default());
    let server = GatewayServer::new();
    let status = server
        .start(
            GatewayContext {
                registry: registry.clone(),
                runtime: Arc::new(harness_core::runtime::RuntimeManager::default()),
                gate: Arc::new(harness_core::operation_gate::OperationGate::default()),
                data_dir: dir,
                downloads: None,
            },
            &GatewaySettings {
                enabled: true,
                port: 19883,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    (server, registry, status.endpoint.unwrap().host_url)
}

async fn post(base: &str, route: &str, body: Value) -> (u16, Value) {
    let res = reqwest::Client::new()
        .post(format!("{base}/v1/images/{route}"))
        .json(&body)
        .send()
        .await
        .unwrap();
    let code = res.status().as_u16();
    (code, res.json().await.unwrap())
}

fn image_of(v: &Value) -> Vec<u8> {
    B64.decode(v["data"][0]["b64_json"].as_str().unwrap())
        .unwrap()
}

#[tokio::test]
#[ignore = "builds the mflux environment and needs HS_MFLUX_MODEL and HS_MFLUX_LORA"]
async fn a_lora_adapter_changes_what_klein_draws() {
    let (Some(model), Some(lora)) = (folder("HS_MFLUX_MODEL"), folder("HS_MFLUX_LORA")) else {
        eprintln!("HS_MFLUX_MODEL / HS_MFLUX_LORA not set, skipping");
        return;
    };
    let plain = ModelDefinition::new(
        "mflux-e2e-plain",
        ModelRef::Local {
            path: model.clone(),
        },
    );
    let mut styled = ModelDefinition::new(
        "mflux-e2e-lora",
        ModelRef::Local {
            path: model.clone(),
        },
    );
    styled.adapters = vec![AdapterRef {
        path: lora.clone(),
        scale: Some(1.0),
    }];
    let dir = setup(&model, vec![plain, styled]).await;
    let (server, registry, base) = gateway(dir).await;

    let ask = |alias: &str| {
        json!({"model": alias, "prompt": "pixel art of a castle on a hill, 16-bit game sprite",
               "size": "256x256", "steps": 4, "seed": 11})
    };
    let mut drawn = Vec::new();
    for alias in ["mflux-e2e-plain", "mflux-e2e-lora"] {
        let t = Instant::now();
        let (code, v) = post(&base, "generations", ask(alias)).await;
        eprintln!("{alias}: {code} in {:?} (load included)", t.elapsed());
        assert_eq!(code, 200, "{v}");
        let png = image_of(&v);
        assert_eq!(png_size(&png), (256, 256));
        if let Some(out) = std::env::var_os("HS_MFLUX_E2E_OUT") {
            std::fs::write(PathBuf::from(out).join(format!("{alias}.png")), &png).unwrap();
        }
        drawn.push(png);
        // One model in memory at a time on a small Mac.
        registry.stop_all().await;
    }
    // Same seed and prompt: the adapter is what makes the pictures differ.
    assert_ne!(drawn[0], drawn[1]);

    server.stop().await;
    registry.stop_all().await;
}

#[tokio::test]
#[ignore = "builds the mflux environment and needs HS_MFLUX_ZIMAGE"]
async fn z_image_draws_from_text_and_from_an_image() {
    let Some(model) = folder("HS_MFLUX_ZIMAGE") else {
        eprintln!("HS_MFLUX_ZIMAGE not set, skipping");
        return;
    };
    assert_eq!(
        harness_core::engine::mlx::mflux_model(&model),
        Some("z-image-turbo")
    );
    assert!(harness_core::engine::mlx::is_mflux_dir(&model));
    let dir = setup(
        &model,
        vec![ModelDefinition::new(
            "mflux-e2e-zimage",
            ModelRef::Local {
                path: model.clone(),
            },
        )],
    )
    .await;
    let library = harness_core::models::ModelManager.scan_directories(&[model
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()]);
    let found = library.iter().find(|m| m.path == model).unwrap();
    eprintln!(
        "library: {} {} {:?} {:?}",
        found.name, found.quantization, found.task, found.architecture
    );
    assert_eq!(found.architecture.as_deref(), Some("z-image-turbo"));
    let (server, registry, base) = gateway(dir).await;

    let t = Instant::now();
    let (code, v) = post(
        &base,
        "generations",
        json!({"model": "mflux-e2e-zimage", "prompt": "a red apple on a wooden table, photo",
               "size": "256x256", "steps": 4, "seed": 7}),
    )
    .await;
    eprintln!("text to image: {code} in {:?} (load included)", t.elapsed());
    assert_eq!(code, 200, "{v}");
    let apple = image_of(&v);
    assert_eq!(png_size(&apple), (256, 256));

    // Image to image from that picture, without a size: its own.
    let t = Instant::now();
    let (code, v) = post(
        &base,
        "edits",
        json!({"model": "mflux-e2e-zimage", "prompt": "a green apple on a wooden table, photo",
               "image": B64.encode(&apple), "strength": 0.5, "steps": 4, "seed": 7}),
    )
    .await;
    eprintln!("image to image: {code} in {:?}", t.elapsed());
    assert_eq!(code, 200, "{v}");
    let green = image_of(&v);
    assert_eq!(png_size(&green), (256, 256));
    assert_ne!(green, apple);
    if let Some(out) = std::env::var_os("HS_MFLUX_E2E_OUT") {
        let out = PathBuf::from(out);
        std::fs::write(out.join("zimage-apple.png"), &apple).unwrap();
        std::fs::write(out.join("zimage-green.png"), &green).unwrap();
    }

    // Two start images, or a mask: Z-Image takes neither.
    let (code, v) = post(
        &base,
        "edits",
        json!({"model": "mflux-e2e-zimage", "prompt": "x",
               "image": [B64.encode(&apple), B64.encode(&apple)], "steps": 1}),
    )
    .await;
    assert_eq!(code, 400, "{v}");
    let (code, v) = post(
        &base,
        "edits",
        json!({"model": "mflux-e2e-zimage", "prompt": "x", "image": B64.encode(&apple),
               "mask": B64.encode(&apple), "steps": 1}),
    )
    .await;
    assert_eq!(code, 400, "{v}");
    assert_eq!(v["error"]["param"], "mask", "{v}");

    server.stop().await;
    registry.stop_all().await;
}
