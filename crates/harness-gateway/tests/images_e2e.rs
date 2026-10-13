//! Image generation through the gateway with a real sd-server and a real model (no mocks):
//!
//! ```text
//! HS_SD_SERVER=<sd-server.exe> HS_SD_MODEL=<sd_turbo-*.gguf | *.safetensors>
//! cargo test -p harness-gateway --test images_e2e -- --ignored --nocapture
//! ```
//!
//! Temporary data root (never the user's), in-memory secrets. The model is expected to be a
//! small single-file checkpoint (SD-Turbo: 4 steps at CFG 1).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine as _;
use harness_core::domain::{ModelDefinition, ModelRef};
use harness_core::gateway::GatewaySettings;
use harness_core::params::ParamValue;
use harness_core::servers::ServerRegistry;
use harness_gateway::{GatewayContext, GatewayServer};
use serde_json::{json, Value};

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|p| p.is_file())
}

/// Width and height of a PNG, from its IHDR chunk.
fn png_size(bytes: &[u8]) -> (u32, u32) {
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "not a PNG");
    let w = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let h = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    (w, h)
}

#[tokio::test]
#[ignore]
async fn e2e_image_generation_jit_ttl() {
    let (Some(server), Some(model)) = (env_path("HS_SD_SERVER"), env_path("HS_SD_MODEL")) else {
        eprintln!("HS_SD_SERVER / HS_SD_MODEL not set, skipping");
        return;
    };
    harness_core::secrets::use_memory_store();
    let dir = tempfile::Builder::new()
        .prefix(&harness_core::paths::test_dir_prefix("gw-image"))
        .tempdir()
        .unwrap()
        .keep();
    harness_core::paths::init(dir.clone());
    let mut s = harness_core::config::AppSettings::load();
    s.server.port = 18830;
    s.control_api.port = 18839;
    s.save().unwrap();

    let entry = harness_core::runtimes::RuntimeRegistry::global()
        .add_local(&harness_core::runtimes::AddLocal {
            path: server.parent().unwrap().to_path_buf(),
            name: "sd e2e".into(),
            id: None,
            copy: false,
            family: "stable-diffusion.cpp".into(),
        })
        .unwrap();
    eprintln!(
        "runtime: {} family={} backend={:?}",
        entry.id, entry.family, entry.backend
    );

    let mut def = ModelDefinition::new(
        "gw-image",
        ModelRef::Local {
            path: model.clone(),
        },
    );
    def.ttl_seconds = Some(8);
    for (k, v) in [("width", 512), ("height", 512), ("steps", 4)] {
        def.load.insert(k.into(), ParamValue::Int(v));
    }
    def.load.insert("cfg_scale".into(), ParamValue::Float(1.0));
    harness_core::domain::store::Domain::global()
        .save_definition(def)
        .unwrap();

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
                port: 18830,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let base = status.endpoint.unwrap().host_url;
    let http = reqwest::Client::new();
    let running = || {
        registry
            .all()
            .into_iter()
            .filter(|(_, pm)| pm.get_status() == harness_core::process::ServerStatus::Running)
            .count()
    };
    assert_eq!(running(), 0);

    // JIT load, then a 512x512 image with the model's own settings.
    let t = Instant::now();
    let r = http
        .post(format!("{base}/v1/images/generations"))
        .json(&json!({"model": "gw-image", "prompt": "a red fox in a snowy forest, photo", "seed": 7}))
        .send()
        .await
        .unwrap();
    let st = r.status().as_u16();
    let h = r.headers().clone();
    let body = r.text().await.unwrap();
    eprintln!(
        "first request {st} in {:?} ({} bytes)",
        t.elapsed(),
        body.len()
    );
    assert_eq!(st, 200, "{}", &body[..body.len().min(400)]);
    assert!(h.contains_key("x-harness-load-ms"), "loaded just in time");
    assert_eq!(running(), 1);
    let v: Value = serde_json::from_str(&body).unwrap();
    let png = base64::engine::general_purpose::STANDARD
        .decode(v["data"][0]["b64_json"].as_str().unwrap())
        .unwrap();
    assert_eq!(png_size(&png), (512, 512));
    let out = dir.join("fox.png");
    std::fs::write(&out, &png).unwrap();
    eprintln!("wrote {} ({} bytes)", out.display(), png.len());
    if let Ok(keep) = std::env::var("HS_SD_OUT") {
        std::fs::copy(&out, keep).unwrap();
    }

    // Image-to-image: the fox as OpenAI's multipart edit form (the canvas is the image's size),
    // then as JSON with a data: URL, a mask and another size.
    let boundary = "harness-e2e";
    let mut form = Vec::new();
    for (head, data) in [
        (
            "name=\"image[]\"; filename=\"fox.png\"\r\nContent-Type: image/png",
            png.as_slice(),
        ),
        ("name=\"model\"", b"gw-image".as_slice()),
        (
            "name=\"prompt\"",
            b"a red fox at night, moonlight".as_slice(),
        ),
        ("name=\"strength\"", b"0.5".as_slice()),
        ("name=\"seed\"", b"7".as_slice()),
    ] {
        form.extend(
            format!("--{boundary}\r\nContent-Disposition: form-data; {head}\r\n\r\n").bytes(),
        );
        form.extend(data);
        form.extend(b"\r\n");
    }
    form.extend(format!("--{boundary}--\r\n").bytes());
    let t = Instant::now();
    let r = http
        .post(format!("{base}/v1/images/edits"))
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(form)
        .send()
        .await
        .unwrap();
    let st = r.status().as_u16();
    let body = r.text().await.unwrap();
    eprintln!("edit (multipart) {st} in {:?}", t.elapsed());
    assert_eq!(st, 200, "{}", &body[..body.len().min(400)]);
    let v: Value = serde_json::from_str(&body).unwrap();
    let night = base64::engine::general_purpose::STANDARD
        .decode(v["data"][0]["b64_json"].as_str().unwrap())
        .unwrap();
    assert_eq!(png_size(&night), (512, 512));
    assert_ne!(night, png, "the edit changed the picture");
    if let Ok(keep) = std::env::var("HS_SD_OUT") {
        std::fs::write(format!("{keep}.edit.png"), &night).unwrap();
    }
    let data_url = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&png)
    );
    let t = Instant::now();
    let r = http
        .post(format!("{base}/v1/images/edits"))
        .json(
            &json!({"model": "gw-image", "prompt": "a red fox, oil painting", "image": data_url,
                      "mask": data_url, "size": "256x256", "strength": 0.6, "steps": 2}),
        )
        .send()
        .await
        .unwrap();
    let st = r.status().as_u16();
    let body = r.text().await.unwrap();
    eprintln!("edit (json, mask, 256x256) {st} in {:?}", t.elapsed());
    assert_eq!(st, 200, "{}", &body[..body.len().min(400)]);
    let v: Value = serde_json::from_str(&body).unwrap();
    let small = base64::engine::general_purpose::STANDARD
        .decode(v["data"][0]["b64_json"].as_str().unwrap())
        .unwrap();
    assert_eq!(png_size(&small), (256, 256));
    // An edit without an image names the field.
    let r = http
        .post(format!("{base}/v1/images/edits"))
        .json(&json!({"model": "gw-image", "prompt": "x"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 400);
    assert!(r.text().await.unwrap().contains("image"));

    // Same seed, same picture; another size, n = 2 and a data URL.
    let r = http
        .post(format!("{base}/v1/images/generations"))
        .json(&json!({"model": "gw-image", "prompt": "a red fox in a snowy forest, photo", "seed": 7}))
        .send()
        .await
        .unwrap();
    let v: Value = r.json().await.unwrap();
    let again = base64::engine::general_purpose::STANDARD
        .decode(v["data"][0]["b64_json"].as_str().unwrap())
        .unwrap();
    assert_eq!(png_size(&again), (512, 512));
    let r = http
        .post(format!("{base}/v1/images/generations"))
        .json(
            &json!({"model": "gw-image", "prompt": "a lighthouse", "n": 2, "size": "256x384",
                      "response_format": "url", "steps": 2, "negative_prompt": "text"}),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 200);
    let v: Value = r.json().await.unwrap();
    let data = v["data"].as_array().unwrap();
    assert_eq!(data.len(), 2);
    let url = data[0]["url"].as_str().unwrap();
    let b64 = url.strip_prefix("data:image/png;base64,").unwrap();
    let small = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .unwrap();
    assert_eq!(png_size(&small), (256, 384));

    // Errors: no prompt, a chat model that is not an image model, chat on an image model.
    let r = http
        .post(format!("{base}/v1/images/generations"))
        .json(&json!({"model": "gw-image"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 400);
    let r = http
        .post(format!("{base}/v1/chat/completions"))
        .json(&json!({"model": "gw-image", "messages": [{"role": "user", "content": "hi"}]}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 400);
    let body = r.text().await.unwrap();
    assert!(body.contains("model_is_image"), "{body}");

    // TTL: the idle model is unloaded, and the next request loads it again.
    let deadline = Instant::now() + Duration::from_secs(40);
    while running() > 0 && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    assert_eq!(running(), 0, "the TTL unloaded the image model");
    let r = http
        .post(format!("{base}/v1/images/generations"))
        .json(&json!({"model": "gw-image", "prompt": "a cat", "steps": 2}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status().as_u16(), 200);
    assert!(r.headers().contains_key("x-harness-load-ms"));

    gw.stop().await;
    registry.stop_all().await;
    assert_eq!(running(), 0);
}
